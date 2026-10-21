//! Core: the elaborated, fully explicit term language the type checker works
//! with.
//!
//! Unlike [`idyllic_hir`], Core uses de Bruijn *indices* and carries no node
//! ids — it is produced by elaboration and by quoting values back from the
//! semantic domain ([`super::value`]).  Because binders are nameless,
//! structural equality on [`Core`] *is* alpha-equality: the name in `Var` /
//! `Lam` / `Pi` / `Let` is only a display hint and is ignored by `PartialEq`.
//!
//! A Core term is closed up to the context it was elaborated in: every index
//! refers either to a binder inside the term (`Lam`, `Pi`, `Let`) or to an
//! entry of that context.  In particular a `let` is a real binder here, so a
//! term containing one can be evaluated anywhere its context is available.
//!
//! Grades are Core terms too, of type `Grade`: literals, `+`, `*`, `..`, and
//! anything else of that type (a variable, a metavariable).  Implicit
//! arguments are not distinguished in `App` and `Lam`: by the time a term is
//! Core every argument is there.

use idyllic_ast::{Icit, Lit};
use idyllic_builtins::{Builtin, Prim};
use crate::grade::Grade;
use std::fmt;
use std::rc::Rc;

/// de Bruijn index: 0 is the innermost binder.  Used in [`Core`].
pub type Idx = usize;
/// de Bruijn level: 0 is the outermost binder.  Used in values.
pub type Lvl = usize;
/// Identifies an entry in the metacontext.
pub type MetaVar = usize;
/// A universe level: `Type n` lives in `Type (n + 1)`.
pub type Level = usize;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Core {
    /// A universe, `Type n`.  `Type` is `Type 0`, `Type n : Type (n + 1)`,
    /// and a type in one universe is also in every larger one.
    Type(Level),
    /// A primitive type constant (`Int`, `Bool`, `Grade`, ...).
    LitTy(Builtin),
    Lit(Lit),
    /// A primitive operation (`+`), unapplied.  Arguments arrive by `App`.
    Prim(Prim),
    /// A variable, by de Bruijn index, plus a name hint for display.
    Var(Idx, String),
    /// A recursive definition, by the unique of its binder.  It never
    /// unfolds, so it needs no context slot: it is a constant.
    Opaque(usize, String),
    /// A metavariable: a not-yet-known term, to be solved by unification.
    /// Metas are always *closed*; the elaborator inserts them applied to the
    /// bound variables in scope, so this constructor never captures.
    Meta(MetaVar),
    /// `(x : (grade, in_type) dom) -> cod`: a function using its argument
    /// `grade` times, whose result type mentions it `in_type` times.  The
    /// hint is `"_"` for a non-dependent arrow.
    Pi { name: String, icit: Icit, grade: Rc<Core>, in_type: Rc<Core>, dom: Rc<Core>, cod: Rc<Core> },
    Lam(String, Rc<Core>),
    App(Rc<Core>, Rc<Core>),
    /// The graded modality `a [r]`: grade, then type.
    BoxTy(Rc<Core>, Rc<Core>),
    /// Promotion `[e]`.
    BoxIntro(Rc<Core>),
    /// The eliminator behind `let [x] = e in ..`, which elaborates to
    /// `let x = unbox e in ..`.
    Unbox(Rc<Core>),
    /// `let x = e in body`; index 0 in `body` is `x`.
    Let(String, Rc<Core>, Rc<Core>),
    /// A constant grade, `2` or `0..1`.
    GradeLit(Grade),
    GradeAdd(Rc<Core>, Rc<Core>),
    GradeMul(Rc<Core>, Rc<Core>),
    /// `lo..hi` between grades that are not (yet) constants.
    GradeRange(Rc<Core>, Rc<Core>),
    /// `case e of { C x y -> a; .. }`.  Each alternative's body binds its
    /// variables, the last innermost.
    Case(Rc<Core>, Rc<Vec<CaseAlt>>),
    /// A note for whoever runs the program, invisible to the type theory:
    /// the term means exactly what it would without it.
    Runtime(Note, Rc<Core>),
    /// `b ! e`, the type of a computation giving a `b` with effects `e`:
    /// what a function type's codomain is when calling it has effects.  A
    /// pure one is just `b`.
    Comp(Rc<Core>, Rc<Core>),
    /// An effect row: the effects, then the row variables for the rest.
    Row(Vec<Rc<Core>>, Vec<Rc<Core>>),
    /// `handle body with clauses`: the body, the `return` clause (a lambda),
    /// and a clause for each of the effect's operations, in order.
    Handle(Rc<Core>, Rc<Core>, Rc<Vec<OpClause>>),
    /// Stands in for a subterm that failed to elaborate, so that one error
    /// does not cascade into a hundred.
    Error,
}

/// What an alternative matches: a declared constructor, by the unique of its
/// name, or one of the two `Bool` literals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Head {
    Con(usize),
    Bool(bool),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CaseAlt {
    pub head: Head,
    /// the constructor's name, for display
    pub name: String,
    /// hints for the variables the body binds, one per field
    pub vars: Vec<String>,
    pub body: Rc<Core>,
}

/// A handler's clause for an operation: its body is a lambda for each of
/// the operation's own implicit arguments (`erased` of them, which are
/// never passed), then for the argument, then for the resumption.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct OpClause {
    /// the operation, by the unique of its name
    pub op: usize,
    pub name: String,
    pub erased: usize,
    pub body: Rc<Core>,
}

/// What the elaborator knew, and erasure would otherwise forget.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Note {
    /// An argument to a grade-`0` parameter, or the contents of a grade-`0`
    /// box: never used at run time, so never computed.
    Erased,
    /// On a lambda or an unboxing: the variable it binds, as the elaborator
    /// knew it.  Its grade, if it is a constant, which says how often the
    /// program uses it; and its type (in the context the note is in, outside
    /// the binder), from which the lowering works out how the variable is
    /// represented at run time.
    Binds { grade: Option<Grade>, ty: Option<Rc<Core>> },
    /// On a `case` on an erased value whose constructor its type forces:
    /// there is nothing to look at, and nothing to choose, so its one
    /// alternative (or none) is taken without evaluating the scrutinee, its
    /// variables all erased.
    Forced,
}

impl Core {
    /// Whether the term mentions the variable with de Bruijn index `i`.
    pub fn mentions(&self, i: usize) -> bool {
        crate::matching::mentions(self, i)
    }

    /// Whether a definition of this term, unused, can be left out: whether
    /// computing it does nothing but make a value.  A function, a literal, a
    /// variable, and a constructor applied to such terms; `ctor` says which
    /// constants are constructors.
    pub fn is_pure_value(&self, ctor: &dyn Fn(usize) -> bool) -> bool {
        match self {
            Core::Lam(..) | Core::Lit(_) | Core::Var(..) | Core::Prim(_) | Core::Opaque(..) => true,
            Core::Runtime(Note::Erased, _) => true,
            Core::Runtime(_, inner) | Core::BoxIntro(inner) => inner.is_pure_value(ctor),
            Core::App(..) => {
                let mut head = self;
                let mut args = Vec::new();
                while let Core::App(f, a) = head {
                    args.push(a);
                    head = f;
                }
                matches!(head, Core::Opaque(u, _) if ctor(*u)) && args.iter().all(|a| a.is_pure_value(ctor))
            }
            _ => false,
        }
    }

    pub fn grade(g: Grade) -> Rc<Core> {
        Rc::new(Core::GradeLit(g))
    }

    fn is_grade(&self, g: Grade) -> bool {
        matches!(self, Core::GradeLit(h) if *h == g)
    }
}

/// Alpha-equality: name hints are display-only and do not participate.
impl PartialEq for Core {
    fn eq(&self, other: &Self) -> bool {
        use Core::*;
        match (self, other) {
            (Error, Error) => true,
            (Type(a), Type(b)) => a == b,
            (LitTy(a), LitTy(b)) => a == b,
            (Lit(a), Lit(b)) => a == b,
            (Prim(a), Prim(b)) => a == b,
            (Var(i, _), Var(j, _)) => i == j,
            (Opaque(a, _), Opaque(b, _)) => a == b,
            (Meta(a), Meta(b)) => a == b,
            (
                Pi { icit, grade, in_type, dom, cod, .. },
                Pi { icit: icit2, grade: grade2, in_type: in_type2, dom: dom2, cod: cod2, .. },
            ) => icit == icit2 && grade == grade2 && in_type == in_type2 && dom == dom2 && cod == cod2,
            (Lam(_, b), Lam(_, b2)) => b == b2,
            (GradeLit(a), GradeLit(b)) => a == b,
            (App(f, a), App(g, b))
            | (BoxTy(f, a), BoxTy(g, b))
            | (GradeAdd(f, a), GradeAdd(g, b))
            | (GradeMul(f, a), GradeMul(g, b))
            | (GradeRange(f, a), GradeRange(g, b)) => f == g && a == b,
            (BoxIntro(a), BoxIntro(b)) | (Unbox(a), Unbox(b)) => a == b,
            (Runtime(n, a), Runtime(m, b)) => n == m && a == b,
            (Comp(a, e), Comp(b, f)) => a == b && e == f,
            (Row(l, t), Row(l2, t2)) => l == l2 && t == t2,
            (Handle(b, r, ops), Handle(b2, r2, ops2)) => {
                b == b2
                    && r == r2
                    && ops.len() == ops2.len()
                    && ops.iter().zip(ops2.iter()).all(|(x, y)| x.op == y.op && x.body == y.body)
            }
            (Case(s, alts), Case(s2, alts2)) => {
                s == s2
                    && alts.len() == alts2.len()
                    && alts.iter().zip(alts2.iter()).all(|(a, b)| a.head == b.head && a.body == b.body)
            }
            (Let(_, a, b), Let(_, a2, b2)) => a == a2 && b == b2,
            _ => false,
        }
    }
}

impl Eq for Core {}

/// The primitive type of a literal.
pub fn lit_builtin(l: &Lit) -> Builtin {
    match l {
        Lit::Unit => Builtin::Unit,
        Lit::Int(_) => Builtin::Int,
        Lit::Bool(_) => Builtin::Bool,
        Lit::Str(_) => Builtin::Str,
        Lit::Char(_) => Builtin::Char,
    }
}

thread_local! {
    /// Whether types are printed for a linear module, where an unannotated
    /// binder is linear: there `1` goes unsaid and `[]` is written, and
    /// elsewhere the other way round.
    static LINEAR_PRINTING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Print types from here on as a linear module (or not) reads them; what
/// it was before.
pub fn print_linear(linear: bool) -> bool {
    LINEAR_PRINTING.with(|l| l.replace(linear))
}

/// What an unannotated explicit binder has, as types are printed now.
fn printed_default() -> Grade {
    if LINEAR_PRINTING.with(std::cell::Cell::get) { Grade::ONE } else { Grade::DEFAULT }
}

/// Every constant a term mentions, with the name it is printed with.
pub fn opaques(c: &Rc<Core>) -> Vec<(usize, String)> {
    let found = std::cell::RefCell::new(Vec::new());
    c.rewrite(0, &|c, _| {
        if let Core::Opaque(u, name) = c {
            let mut found = found.borrow_mut();
            if !found.iter().any(|(v, _)| v == u) {
                found.push((*u, name.clone()));
            }
        }
        None
    });
    found.into_inner()
}

/// `a`, if `g` is `Pair a`: a pair's type, waiting for its second.
fn pair_of(g: &Core) -> Option<&Rc<Core>> {
    match g {
        Core::App(h, a) if matches!(&**h, Core::Opaque(_, name) if name == "Pair") => Some(a),
        _ => None,
    }
}

impl fmt::Display for Core {
    /// Render for error messages, in the surface syntax.  Binders are shown
    /// using their name hints, so shadowed names can look ambiguous; that is
    /// a display nicety only.  Grades are shown only when they differ from
    /// the defaults, the way they are written: in a linear module's way, if
    /// [`print_linear`] says so.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        /// `x : a`, `x : [s] a` or `x : (s, r) a`.
        fn binder(
            x: &str,
            (grade, in_type): (&Core, &Core),
            default: Grade,
            dom: &Core,
            f: &mut fmt::Formatter<'_>,
        ) -> fmt::Result {
            write!(f, "{x} : ")?;
            if !in_type.is_grade(Grade::DEFAULT_IN_TYPE) {
                write!(f, "(")?;
                go(grade, 0, f)?;
                write!(f, ", ")?;
                go(in_type, 0, f)?;
                write!(f, ") ")?;
            } else if grade.is_grade(default) {
                // (what an unannotated binder has goes unsaid)
            } else if grade.is_grade(Grade::ANY) {
                write!(f, "[] ")?;
            } else {
                write!(f, "[")?;
                go(grade, 0, f)?;
                write!(f, "] ")?;
            }
            go(dom, 0, f)
        }
        fn infix(
            (a, op, b): (&Core, &str, &Core),
            level: u8,
            prec: u8,
            f: &mut fmt::Formatter<'_>,
        ) -> fmt::Result {
            paren(prec > level, f, |f| {
                go(a, level, f)?;
                write!(f, "{op}")?;
                go(b, level + 1, f)
            })
        }
        fn go(c: &Core, prec: u8, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match c {
                Core::Type(0) => write!(f, "Type"),
                // written like an application, which is how it is parsed
                Core::Type(n) => paren(prec > 10, f, |f| write!(f, "Type {n}")),
                // the unit type is written as its value is, as in Haskell
                Core::LitTy(Builtin::Unit) => write!(f, "()"),
                Core::LitTy(b) => write!(f, "{}", b.name()),
                Core::Lit(l) => write!(f, "{l}"),
                Core::Var(_, n) | Core::Opaque(_, n) => write!(f, "{n}"),
                Core::Meta(m) => write!(f, "?{m}"),
                Core::Error => write!(f, "?!"),
                Core::Runtime(_, inner) => go(inner, prec, f),
                // looser than application, tighter than an arrow
                Core::Comp(t, e) => paren(prec > 1, f, |f| {
                    go(t, 2, f)?;
                    write!(f, " ! ")?;
                    // an application needs no parentheses: `! State s`
                    go(e, 10, f)
                }),
                Core::Handle(body, ret, ops) => paren(prec > 0, f, |f| {
                    write!(f, "handle ")?;
                    go(body, 0, f)?;
                    write!(f, " with {{ return ")?;
                    go(ret, 11, f)?;
                    for op in ops.iter() {
                        write!(f, "; {} ", op.name)?;
                        go(&op.body, 11, f)?;
                    }
                    write!(f, " }}")
                }),
                Core::Row(labels, tails) if labels.len() + tails.len() == 1 => {
                    go(labels.first().or(tails.first()).expect("one"), prec, f)
                }
                Core::Row(labels, tails) => {
                    write!(f, "{{")?;
                    for (i, l) in labels.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        go(l, 0, f)?;
                    }
                    for (i, t) in tails.iter().enumerate() {
                        write!(f, "{}", if i == 0 && !labels.is_empty() { " | " } else if i > 0 { ", " } else { "| " })?;
                        go(t, 0, f)?;
                    }
                    write!(f, "}}")
                }
                Core::Case(scrutinee, alts) => paren(prec > 0, f, |f| {
                    write!(f, "case ")?;
                    go(scrutinee, 0, f)?;
                    write!(f, " of {{ ")?;
                    for (i, alt) in alts.iter().enumerate() {
                        if i > 0 {
                            write!(f, "; ")?;
                        }
                        write!(f, "{}", alt.name)?;
                        for x in &alt.vars {
                            write!(f, " {x}")?;
                        }
                        write!(f, " -> ")?;
                        go(&alt.body, 0, f)?;
                    }
                    write!(f, " }}")
                }),
                Core::BoxIntro(e) => {
                    write!(f, "[")?;
                    go(e, 0, f)?;
                    write!(f, "]")
                }
                // the modality is postfix, and binds like an application
                Core::BoxTy(g, a) => paren(prec > 10, f, |f| {
                    go(a, 10, f)?;
                    if g.is_grade(Grade::ANY) {
                        return write!(f, " []");
                    }
                    write!(f, " [")?;
                    go(g, 0, f)?;
                    write!(f, "]")
                }),
                Core::Unbox(e) => paren(prec > 10, f, |f| {
                    write!(f, "unbox ")?;
                    go(e, 11, f)
                }),
                Core::GradeLit(g) => paren(!g.is_exact() && prec > 5, f, |f| write!(f, "{g}")),
                Core::GradeAdd(a, b) => infix((a, " + ", b), 6, prec, f),
                Core::GradeMul(a, b) => infix((a, " * ", b), 7, prec, f),
                Core::GradeRange(a, b) => paren(prec > 5, f, |f| {
                    go(a, 6, f)?;
                    write!(f, "..")?;
                    go(b, 6, f)
                }),
                // a bare or partially applied operator is written sectioned
                Core::Prim(p) => write!(f, "({})", p.name()),
                // a saturated binary primitive reads infix: left associative,
                // tighter than `->` and `:`, looser than application
                Core::App(g, b) if matches!(&**g, Core::App(h, _) if matches!(**h, Core::Prim(_))) => {
                    let Core::App(h, a) = &**g else { unreachable!() };
                    let Core::Prim(p) = **h else { unreachable!() };
                    infix((a, &format!(" {} ", p.name()), b), p.precedence(), prec, f)
                }
                // the prelude's pair is a tuple, `(Int, Bool)`, and one nested
                // to the right a longer one, `(Int, Bool, String)`
                Core::App(g, b) if pair_of(g).is_some() => {
                    let mut items = vec![pair_of(g).expect("checked")];
                    let mut rest = b;
                    while let Core::App(g, b) = &**rest {
                        let Some(a) = pair_of(g) else { break };
                        items.push(a);
                        rest = b;
                    }
                    write!(f, "(")?;
                    for a in items {
                        go(a, 0, f)?;
                        write!(f, ", ")?;
                    }
                    go(rest, 0, f)?;
                    write!(f, ")")
                }
                Core::App(g, a) => paren(prec > 10, f, |f| {
                    go(g, 10, f)?;
                    write!(f, " ")?;
                    go(a, 11, f)
                }),
                Core::Lam(x, b) => paren(prec > 0, f, |f| {
                    write!(f, "\\{x} -> ")?;
                    go(b, 0, f)
                }),
                Core::Let(x, e, b) => paren(prec > 0, f, |f| {
                    write!(f, "let {x} = ")?;
                    go(e, 0, f)?;
                    write!(f, " in ")?;
                    go(b, 0, f)
                }),
                // an implicit binder is written like an explicit one, in
                // braces; it is erased unless it says otherwise
                Core::Pi { name, icit: Icit::Implicit, grade, in_type, dom, cod } => paren(prec > 0, f, |f| {
                    write!(f, "{{")?;
                    binder(name, (grade, in_type), Grade::ZERO, dom, f)?;
                    write!(f, "}} -> ")?;
                    go(cod, 0, f)
                }),
                // an anonymous, default-graded binder is just an arrow
                Core::Pi { name, grade, in_type, dom, cod, .. }
                    if name == "_"
                        && grade.is_grade(printed_default())
                        && in_type.is_grade(Grade::DEFAULT_IN_TYPE) =>
                {
                    paren(prec > 0, f, |f| {
                        go(dom, 1, f)?;
                        write!(f, " -> ")?;
                        go(cod, 0, f)
                    })
                }
                Core::Pi { name, grade, in_type, dom, cod, .. } => paren(prec > 0, f, |f| {
                    write!(f, "(")?;
                    binder(name, (grade, in_type), printed_default(), dom, f)?;
                    write!(f, ") -> ")?;
                    go(cod, 0, f)
                }),
            }
        }
        fn paren(
            yes: bool,
            f: &mut fmt::Formatter<'_>,
            inner: impl FnOnce(&mut fmt::Formatter<'_>) -> fmt::Result,
        ) -> fmt::Result {
            if yes {
                write!(f, "(")?;
                inner(f)?;
                write!(f, ")")
            } else {
                inner(f)
            }
        }
        go(self, 0, f)
    }
}

impl Core {
    /// The same term with its leaves rewritten: `f` is given each `Var`,
    /// `Opaque` and `Meta`, with how many binders are around it there, and
    /// what it returns replaces it (`None` keeps it as it is).
    pub fn rewrite(self: &Rc<Core>, depth: usize, f: &dyn Fn(&Core, usize) -> Option<Rc<Core>>) -> Rc<Core> {
        let go = |c: &Rc<Core>, d: usize| c.rewrite(d, f);
        match &**self {
            Core::Var(..) | Core::Opaque(..) | Core::Meta(_) => f(self, depth).unwrap_or_else(|| self.clone()),
            Core::Type(_) | Core::LitTy(_) | Core::Lit(_) | Core::Prim(_) | Core::GradeLit(_) | Core::Error => self.clone(),
            Core::Pi { name, icit, grade, in_type, dom, cod } => Rc::new(Core::Pi {
                name: name.clone(),
                icit: *icit,
                grade: go(grade, depth),
                in_type: go(in_type, depth),
                dom: go(dom, depth),
                cod: go(cod, depth + 1),
            }),
            Core::Lam(x, b) => Rc::new(Core::Lam(x.clone(), go(b, depth + 1))),
            Core::App(a, b) => Rc::new(Core::App(go(a, depth), go(b, depth))),
            Core::BoxTy(a, b) => Rc::new(Core::BoxTy(go(a, depth), go(b, depth))),
            Core::BoxIntro(a) => Rc::new(Core::BoxIntro(go(a, depth))),
            Core::Unbox(a) => Rc::new(Core::Unbox(go(a, depth))),
            Core::Let(x, e, b) => Rc::new(Core::Let(x.clone(), go(e, depth), go(b, depth + 1))),
            Core::GradeAdd(a, b) => Rc::new(Core::GradeAdd(go(a, depth), go(b, depth))),
            Core::GradeMul(a, b) => Rc::new(Core::GradeMul(go(a, depth), go(b, depth))),
            Core::GradeRange(a, b) => Rc::new(Core::GradeRange(go(a, depth), go(b, depth))),
            Core::Case(s, alts) => {
                let alts = alts.iter().map(|a| CaseAlt { body: go(&a.body, depth + a.vars.len()), ..a.clone() }).collect();
                Rc::new(Core::Case(go(s, depth), Rc::new(alts)))
            }
            Core::Runtime(n, a) => {
                let n = match n {
                    Note::Binds { grade, ty: Some(t) } => Note::Binds { grade: *grade, ty: Some(go(t, depth)) },
                    other => other.clone(),
                };
                Rc::new(Core::Runtime(n, go(a, depth)))
            }
            Core::Comp(a, e) => Rc::new(Core::Comp(go(a, depth), go(e, depth))),
            Core::Row(labels, tails) => Rc::new(Core::Row(
                labels.iter().map(|l| go(l, depth)).collect(),
                tails.iter().map(|t| go(t, depth)).collect(),
            )),
            Core::Handle(b, r, ops) => {
                let ops = ops.iter().map(|op| OpClause { body: go(&op.body, depth), ..op.clone() }).collect();
                Rc::new(Core::Handle(go(b, depth), go(r, depth), Rc::new(ops)))
            }
        }
    }
}
