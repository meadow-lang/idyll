//! Name resolution: surface [`crate::ast`] to [`crate::hir`].
//!
//! Responsibilities:
//!
//! * Resolve every variable to an [`hir::Ident`] with a scope-unique integer;
//!   report an unbound name for anything not in scope.
//!
//! * Introduce fresh uniques for every binder (lambda / Pi / definition), with
//!   the usual lexical scoping (a Pi/lambda domain does not see the name it
//!   binds; later telescope entries do; declaration groups are recursive).
//!
//! * Pair signatures with their equations, and desugar equations and
//!   patterns: `f x [y] = e` becomes `f = \x b -> let [y] = b in e`.
//!
//! * Allocate a fresh [`hir::NodeId`] for every [`hir::Term`] and
//!   [`hir::Bind`], recording its source span in the [`hir::SpanTable`] side
//!   table.
//!
//! `let` / top-level groups are emitted as one conservative
//! [`hir::BindGroup::Rec`] group; [`crate::scc`] refines that into
//! dependency-ordered SCCs.

use crate::ast::{self, Icit, LTerm};
use crate::builtins::{Builtin, Prim, FIRST_USER_UNIQUE};
use crate::diagnostics::Diagnostic;
use crate::hir::{self, Ident, NodeId, SpanTable};
use crate::span::Span;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameError {
    /// A name with no binding in scope.
    Unbound { name: String, span: Span },
    /// Two bindings (or two signatures) of the same name in one group / scope.
    Duplicate { name: String, first: Span, again: Span },
    /// `f : a` with no equation for `f` beside it.
    SignatureWithoutDefinition { name: String, span: Span },
}

impl RenameError {
    pub fn to_diagnostic(&self) -> Diagnostic {
        match self {
            RenameError::Unbound { name, span } => {
                Diagnostic::error(format!("name not in scope: {name}")).label(*span, "used here")
            }
            RenameError::Duplicate { name, first, again } => {
                Diagnostic::error(format!("duplicate binding of {name}"))
                    .label(*again, "bound again here")
                    .label(*first, "first bound here")
            }
            RenameError::SignatureWithoutDefinition { name, span } => {
                Diagnostic::error(format!("the signature for {name} has no definition"))
                    .label(*span, "declared here, but never defined")
                    .help(format!("add an equation, as in {name} = ..; to annotate an expression instead, parenthesise it: ({name} : ..)"))
            }
        }
    }
}

pub struct RenameResult {
    pub module: hir::Module,
    pub spans: SpanTable,
    pub errors: Vec<RenameError>,
}

/// One lexical scope: surface spelling to the unique it was bound with, and
/// where it was bound (so a later duplicate can point back at the original).
type Scope = HashMap<String, (usize, Span)>;

struct Renamer {
    /// innermost last; never empty (the first is the global scope)
    scopes: Vec<Scope>,
    next_unique: usize,
    spans: SpanTable,
    errors: Vec<RenameError>,
}

/// Rename a whole program: its declarations become the top-level bindings and
/// its trailing expression, if any, the body.  For the sake of one-liners, a
/// program that is nothing but `let ... in body` is treated the same way: the
/// `let`'s declarations are the top-level ones.
pub fn rename_module(prog: &ast::Program, prelude: &[ast::LData]) -> RenameResult {
    let mut r = Renamer::new();
    // user names get a scope of their own, so that redefining a builtin
    // shadows it rather than clashing with it
    r.scopes.push(Scope::new());
    // data types first: their names and constructors are in scope everywhere
    let all: Vec<&ast::LData> = prelude.iter().chain(&prog.datas).collect();
    let datas = r.rename_datas(&all);
    let module = match (&prog.decls[..], prog.body.as_ref().map(|b| &b.node)) {
        ([], Some(ast::Term::Let(decls, body))) => {
            let bs = r.rename_decls(decls);
            let body = r.rename_term(body);
            hir::Module { datas, groups: vec![hir::BindGroup::Rec(bs)], body: Some(body) }
        }
        (decls, _) => {
            let bs = r.rename_decls(decls);
            let body = prog.body.as_ref().map(|b| r.rename_term(b));
            let groups = if bs.is_empty() { Vec::new() } else { vec![hir::BindGroup::Rec(bs)] };
            hir::Module { datas, groups, body }
        }
    };
    RenameResult { module, spans: r.spans, errors: r.errors }
}

/// A pattern whose names have been bound: what is left to do is wrap the
/// lambda around the body.
struct BoundParam {
    span: Span,
    name: Ident,
    /// for an unboxing pattern `[x]`, the binder of the box itself
    boxed: Option<Ident>,
    grade: Option<hir::GradeAnn>,
    ty: Option<hir::Term>,
}

impl Renamer {
    /// The starting environment: a single global scope pre-populated with the
    /// primitive types and operations, holding the uniques they reserve.
    fn new() -> Self {
        let builtins = Builtin::ALL.iter().map(|b| (b.name(), b.unique()));
        let prims = Prim::ALL.iter().map(|p| (p.name(), p.unique()));
        let global: Scope =
            builtins.chain(prims).map(|(n, u)| (n.to_string(), (u, Span::at(0)))).collect();
        Renamer {
            scopes: vec![global],
            next_unique: FIRST_USER_UNIQUE,
            spans: SpanTable::default(),
            errors: Vec::new(),
        }
    }

    fn fresh_unique(&mut self) -> usize {
        let u = self.next_unique;
        self.next_unique += 1;
        u
    }

    /// Allocate a node id and record its span in the side table.
    fn fresh_node(&mut self, span: Span) -> NodeId {
        self.spans.push(span)
    }

    fn node(&mut self, span: Span, kind: hir::TermKind) -> hir::Term {
        hir::Term { id: self.fresh_node(span), kind }
    }

    /// Run `f` in a fresh innermost scope.
    fn scoped<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.push(Scope::new());
        let x = f(self);
        self.scopes.pop();
        x
    }

    /// Bind `name` in the innermost scope, returning its fresh `Ident`.
    /// Records a duplicate-binding error if the name is already bound in that
    /// same scope.
    fn bind_name(&mut self, name: &ast::Ident) -> Ident {
        let u = self.fresh_unique();
        let scope = self.scopes.last_mut().expect("scope stack is never empty");
        if let Some(&(_, first)) = scope.get(&name.node) {
            self.errors.push(RenameError::Duplicate {
                name: name.node.clone(),
                first,
                again: name.span,
            });
        }
        scope.insert(name.node.clone(), (u, name.span));
        Ident { unique: Some(u), text: name.node.clone() }
    }

    /// Resolve a use of `name`.  On failure, log an error and return an
    /// `Ident` with no unique so downstream passes still get a well-formed
    /// tree.
    fn use_name(&mut self, name: &ast::Ident) -> Ident {
        let found = self.scopes.iter().rev().find_map(|s| s.get(&name.node)).map(|&(u, _)| u);
        if found.is_none() {
            self.errors.push(RenameError::Unbound { name: name.node.clone(), span: name.span });
        }
        Ident { unique: found, text: name.node.clone() }
    }

    fn rename_grades(&mut self, g: &ast::GradeAnn) -> hir::GradeAnn {
        hir::GradeAnn {
            subject: Box::new(self.rename_term(&g.subject)),
            in_type: g.in_type.as_ref().map(|r| Box::new(self.rename_term(r))),
        }
    }

    fn rename_term(&mut self, t: &LTerm) -> hir::Term {
        use ast::Term as A;
        use hir::TermKind as H;
        let sp = t.span;
        match &t.node {
            A::Type => self.node(sp, H::Type),
            A::Hole => self.node(sp, H::Hole),
            A::Inf => self.node(sp, H::Inf),
            A::Lit(l) => self.node(sp, H::Lit(l.clone())),
            A::Var(x) => {
                let i = self.use_name(x);
                self.node(sp, H::Var(i))
            }
            A::App(f, a) => {
                let f = self.rename_term(f);
                let a = self.rename_term(a);
                self.node(sp, H::App(Box::new(f), Box::new(a)))
            }
            A::Ann(e, a) => {
                let e = self.rename_term(e);
                let a = self.rename_term(a);
                self.node(sp, H::Ann(Box::new(e), Box::new(a)))
            }
            A::Range(lo, hi) => {
                let lo = self.rename_term(lo);
                let hi = self.rename_term(hi);
                self.node(sp, H::Range(Box::new(lo), Box::new(hi)))
            }
            A::Pi(mb, icit, grade, dom, cod) => {
                // neither the domain nor the grades can see the bound name
                let grade = grade.as_ref().map(|g| self.rename_grades(g));
                let dom = self.rename_term(dom);
                self.scoped(|r| {
                    let mb = mb.as_ref().map(|x| r.bind_name(x));
                    let cod = r.rename_term(cod);
                    r.node(sp, H::Pi(mb, *icit, grade, Box::new(dom), Box::new(cod)))
                })
            }
            A::Lam(param, body) => self.scoped(|r| {
                let p = r.bind_param(param);
                let body = r.rename_term(body);
                r.wrap_param(sp, p, body)
            }),
            A::BoxIntro(e) => {
                let e = self.rename_term(e);
                self.node(sp, H::BoxIntro(Box::new(e)))
            }
            A::LetBox(x, e, body) => {
                // not recursive: the boxed term cannot see the name it binds
                let e = self.rename_term(e);
                self.scoped(|r| {
                    let x = r.bind_name(x);
                    let body = r.rename_term(body);
                    r.node(sp, H::LetBox(x, Box::new(e), Box::new(body)))
                })
            }
            A::Let(decls, body) => self.scoped(|r| {
                let bs = r.rename_decls(decls);
                let body = r.rename_term(body);
                r.node(sp, H::Let(vec![hir::BindGroup::Rec(bs)], Box::new(body)))
            }),
            A::Case(scrutinee, alts) => {
                let scrutinee = self.rename_term(scrutinee);
                let alts = alts
                    .iter()
                    .map(|alt| {
                        // `True` and `False` are literals, not declared names
                        let head = match alt.ctor.node.as_str() {
                            "True" => hir::AltHead::Bool(true),
                            "False" => hir::AltHead::Bool(false),
                            _ => hir::AltHead::Con(self.use_name(&alt.ctor)),
                        };
                        let id = self.fresh_node(alt.ctor.span);
                        self.scoped(|r| {
                            let vars = alt.vars.iter().map(|x| r.bind_name(x)).collect();
                            hir::Alt { id, head, vars, body: r.rename_term(&alt.body) }
                        })
                    })
                    .collect();
                self.node(sp, H::Case(Box::new(scrutinee), alts))
            }
        }
    }

    /// Data declarations: every type and constructor name is bound first (a
    /// type may mention itself, and the ones after it), then the parameters
    /// of each scope over its constructors' fields.
    fn rename_datas(&mut self, datas: &[&ast::LData]) -> Vec<hir::DataDecl> {
        let names: Vec<(Ident, Vec<Ident>)> = datas
            .iter()
            .map(|d| {
                let name = self.bind_name(&d.node.name);
                (name, d.node.ctors.iter().map(|(c, _)| self.bind_name(c)).collect())
            })
            .collect();
        datas
            .iter()
            .zip(names)
            .map(|(d, (name, ctor_names))| {
                let id = self.fresh_node(d.span);
                self.scoped(|r| {
                    // a parameter's kind can mention the parameters before it
                    let params = d
                        .node
                        .params
                        .iter()
                        .map(|(x, kind)| {
                            let kind = kind.as_ref().map(|k| r.rename_term(k));
                            (r.bind_name(x), kind)
                        })
                        .collect();
                    let ctors = d
                        .node
                        .ctors
                        .iter()
                        .zip(ctor_names)
                        .map(|((c, fields), name)| hir::CtorDecl {
                            id: r.fresh_node(c.span),
                            name,
                            fields: fields.iter().map(|f| r.rename_term(f)).collect(),
                        })
                        .collect();
                    hir::DataDecl { id, name, params, ctors }
                })
            })
            .collect()
    }

    /// Bind a pattern's name in the innermost scope.  Its annotation is
    /// renamed first, so it cannot see the name being bound.
    fn bind_param(&mut self, p: &ast::LParam) -> BoundParam {
        let grade = p.node.grade.as_ref().map(|g| self.rename_grades(g));
        let ty = p.node.ty.as_ref().map(|t| self.rename_term(t));
        let name = self.bind_name(&p.node.name);
        // the box an unboxing pattern receives shares the name's spelling (it
        // is what shows up in an inferred type) but cannot be referred to
        let boxed = p
            .node
            .unbox
            .then(|| Ident { unique: Some(self.fresh_unique()), text: p.node.name.node.clone() });
        BoundParam { span: p.span, name, boxed, grade, ty }
    }

    /// `\x -> body`, or for an unboxing pattern
    /// `\[x] -> body  ~~>  \b -> let [x] = b in body`.
    fn wrap_param(&mut self, span: Span, p: BoundParam, body: hir::Term) -> hir::Term {
        use hir::TermKind as H;
        match p.boxed {
            None => self.node(span, H::Lam(p.name, p.grade, p.ty.map(Box::new), Box::new(body))),
            Some(boxed) => {
                let scrutinee = self.node(p.span, H::Var(boxed.clone()));
                let unboxed = self.node(span, H::LetBox(p.name, Box::new(scrutinee), Box::new(body)));
                self.node(span, H::Lam(boxed, None, None, Box::new(unboxed)))
            }
        }
    }

    /// Rename a block of declarations as one recursive group: every defined
    /// name is in scope for every right-hand side (and for the body, handled
    /// by the caller).  Signatures are paired up with their equations.
    fn rename_decls(&mut self, decls: &[ast::LDecl]) -> Vec<hir::Bind> {
        let mut sigs: HashMap<&str, (&ast::Ident, &LTerm)> = HashMap::new();
        for d in decls {
            if let ast::Decl::Sig(name, ty) = &d.node {
                if let Some((first, _)) = sigs.insert(&name.node, (name, ty)) {
                    self.errors.push(RenameError::Duplicate {
                        name: name.node.clone(),
                        first: first.span,
                        again: name.span,
                    });
                }
            }
        }
        // Pass 1: introduce all names so the group is mutually recursive.
        let defs: Vec<(&ast::LDecl, Ident)> = decls
            .iter()
            .filter_map(|d| match &d.node {
                ast::Decl::Def(name, ..) => Some((d, self.bind_name(name))),
                ast::Decl::Sig(..) => None,
            })
            .collect();
        // Pass 2: rename each equation, with its signature if it has one.
        let binds = defs
            .into_iter()
            .map(|(d, ident)| {
                let sig = sigs.remove(ident.text.as_str()).map(|(_, ty)| ty);
                self.rename_def(d, ident, sig)
            })
            .collect();
        let mut orphans: Vec<&ast::Ident> = sigs.into_values().map(|(name, _)| name).collect();
        orphans.sort_by_key(|name| name.span.start);
        for name in orphans {
            self.errors.push(RenameError::SignatureWithoutDefinition {
                name: name.node.clone(),
                span: name.span,
            });
        }
        binds
    }

    fn rename_def(&mut self, d: &ast::LDecl, name: Ident, sig: Option<&LTerm>) -> hir::Bind {
        let ast::Decl::Def(_, params, inline, rhs) = &d.node else {
            unreachable!("rename_def is only given equations")
        };
        let id = self.fresh_node(d.span);
        // `x : a = e` is a signature written inline; anywhere else an inline
        // type is the *result* type, an annotation on the right-hand side
        let (sig, result_ty) = match (sig, inline) {
            (None, Some(ty)) if params.is_empty() => (Some(ty), None),
            _ => (sig, inline.as_ref()),
        };
        let ty = sig.map(|t| self.rename_term(t));
        self.scoped(|r| {
            // The variables a signature quantifies over scope over the
            // equation, as type variables do in Granule (and in Haskell with
            // ScopedTypeVariables).
            let mut quantified = ty.as_ref();
            while let Some(hir::Term { id, kind: hir::TermKind::Pi(Some(x), Icit::Implicit, _, _, cod) }) =
                quantified
            {
                if let Some(u) = x.unique {
                    let span = r.spans.get(*id).unwrap_or(Span::at(0));
                    r.scopes.last_mut().expect("just pushed").insert(x.text.clone(), (u, span));
                }
                quantified = Some(cod);
            }
            // Patterns scope over each other's later annotations, the result
            // type, and the body.  They get a scope of their own: shadowing a
            // quantified variable is not a duplicate binding.
            let (params, body) = r.scoped(|r| {
                let ps: Vec<BoundParam> = params.iter().map(|p| r.bind_param(p)).collect();
                let mut body = r.rename_term(rhs);
                if let Some(ty) = result_ty {
                    let span = rhs.span.merge(ty.span);
                    let ty = r.rename_term(ty);
                    body = r.node(span, hir::TermKind::Ann(Box::new(body), Box::new(ty)));
                }
                (ps, body)
            });
            // f (x : a) y = e  ~~>  f = \x y -> e
            let rhs = params.into_iter().rev().fold(body, |body, p| r.wrap_param(d.span, p, body));
            hir::Bind { id, name, ty, rhs }
        })
    }
}
