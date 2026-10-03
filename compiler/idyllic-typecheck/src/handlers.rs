//! Effects of the program's own: declarations, operations and handlers.
//!
//! ```text
//! effect State (s : Type) where
//!   get : () -> s
//!   put : s -> ()
//! ```
//!
//! declares an effect, `State : Type -> Effect`, and an operation for each
//! line, whose type is the one written with the effect's parameters in front
//! and the effect on its last arrow: `put : {s : Type} -> s -> () ! State s`.
//! An operation takes as many arguments as its type has arrows, one at the
//! least: `write : String -> String -> ()` takes two, and is performed once
//! it has both.
//! Performing an operation is applying it, so there is nothing more to type:
//! the row of its arrow is what the application has.
//!
//! `handle e with clauses` runs `e` with one of the effects in its row taken
//! care of:
//!
//! ```text
//! handle e with
//!   return x -> r       -- what `e`'s value becomes (by default, itself)
//!   get _ k -> ..       -- what `get ()` does: `k` is the rest of `e`
//!   put s k -> ..
//! ```
//!
//! A handler handles every operation of one effect.  The effect's parameters
//! are read off the row of `e`, and each clause is checked with them: in
//! `op x k -> body`, `x` has the type of the operation's argument, and `k`,
//! the *resumption*, takes the operation's result and gives back what the
//! whole `handle` does.  Handlers are deep: resuming runs the rest of `e`
//! under the same handler, so `k` has the handle expression's effects, which
//! are those of `e` other than the one handled, and those of the clauses.
//!
//! How often a clause resumes is its resumption's grade, and it is not
//! written: it is inferred from the clause's use of `k`, as `[0]` (an
//! exception), `1`, `0..1`, or more.  More than once is allowed only for an
//! operation declared `@many`: unmarked, an operation is resumed at most
//! once, which is what lets the back end keep the continuation on a stack.
//! The declaration is on the operation because the back end needs it where
//! the operation is performed, which knows nothing of the handler.

use super::core::{Core, OpClause};
use super::value::{vvar, VTy, Value};
use super::{Checker, Ctx, TypeErrorKind, Usage};
use crate::grade::Grade;
use idyllic_ast::Icit;
use idyllic_builtins::Builtin;
use idyllic_hir::{self as hir, NodeId};
use std::rc::Rc;

/// What is known about a declared effect.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EffectInfo {
    pub name: String,
    /// the kinds of its parameters, each under the ones before
    pub kinds: Vec<Rc<Core>>,
    /// its operations, in order
    pub ops: Vec<usize>,
}

/// What is known about an operation.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct OpInfo {
    pub name: String,
    /// the unique of its effect
    pub effect: usize,
    /// its position among its effect's operations
    pub index: usize,
    /// how many erased arguments come before the ones it takes: its
    /// effect's parameters, then its own implicit ones
    pub erased: usize,
    /// how many arguments it takes, after those
    pub arity: usize,
    /// whether a handler may resume it more than once (`@many`); unmarked,
    /// at most once, which is what lets the back end keep its continuation
    /// on a stack
    pub many: bool,
}

/// The kind of a row that may hold operations resumed more than once,
/// `Effect []`: an effect with an `@many` operation has it, and a row
/// variable has it only if it says so.  `Effect` is the kind of a row whose
/// operations are resumed at most once, and is accepted wherever `Effect []`
/// is.  (The shared back end's IR calls it `ManyEffect`.)
pub(super) fn many_effect_kind() -> Rc<Core> {
    Rc::new(Core::BoxTy(Core::grade(Grade::ANY), Rc::new(Core::LitTy(Builtin::Effect))))
}

impl Checker {
    /// Which kind of row a type is: `Some(false)` for `Effect`, `Some(true)`
    /// for `Effect []`, and `None` for neither.
    pub(super) fn row_kind(&self, ty: &Rc<Value>) -> Option<bool> {
        match &*self.force(ty.clone()) {
            Value::LitTy(Builtin::Effect) => Some(false),
            Value::BoxTy(g, a) if matches!(*self.force(a.clone()), Value::LitTy(Builtin::Effect)) => {
                (self.to_poly(self.force(g.clone())).as_constant() == Some(Grade::ANY)).then_some(true)
            }
            _ => None,
        }
    }

    /// Whether a row, as a value, holds an operation that may be resumed
    /// more than once: an effect of kind `Effect []`, or a row variable of
    /// that kind.  `types` are the types of the variables in scope.
    pub(super) fn row_is_many(&self, types: &[(String, Rc<Value>)], row: &Rc<Value>) -> bool {
        let neutral = |this: &Self, mut ty: Rc<Value>, sp: &super::value::Spine| {
            for elim in sp {
                let forced = this.force(ty);
                ty = match (elim, &*forced) {
                    (super::value::Elim::App(a), Value::Pi { cod, .. }) => this.apply(cod, a.clone()),
                    _ => return false,
                };
            }
            this.row_kind(&ty) == Some(true)
        };
        match &*self.force(row.clone()) {
            Value::Row(r) => r.labels.iter().chain(&r.tails).any(|(_, v)| self.row_is_many(types, v)),
            Value::Opaque(u, _, sp) => self.constants.get(u).is_some_and(|ty| neutral(self, ty.clone(), sp)),
            Value::Rigid(k, _, sp) => types.get(*k).is_some_and(|(_, ty)| neutral(self, ty.clone(), sp)),
            Value::Flex(m, _) => match &self.metas[*m] {
                super::MetaEntry::Unsolved(info) | super::MetaEntry::Solved(_, info) => {
                    info.ty.as_ref().is_some_and(|t| self.row_kind(&t.ty) == Some(true))
                }
            },
            _ => false,
        }
    }

    /// Declare each effect: `E : (a : k) -> .. -> Effect`, or `Effect []`
    /// if a handler may resume one of its operations more than once.
    pub(super) fn declare_effects(&mut self, ctx: &Ctx, effects: &[hir::EffectDecl]) -> Ctx {
        let mut ctx = ctx.clone();
        for e in effects {
            let mut under = ctx.clone();
            let mut kinds = Vec::new();
            for (x, kind) in &e.params {
                let (k_c, k_v) = match kind {
                    Some(kind) => self.check_type(&under, kind),
                    None => (Rc::new(Core::Type(0)), Rc::new(Value::Type(0))),
                };
                kinds.push(k_c);
                under = under.bind(Some(x), &x.text, k_v);
            }
            let kind = if e.ops.iter().any(|op| op.many) { many_effect_kind() } else { Rc::new(Core::LitTy(Builtin::Effect)) };
            let ty = e.params.iter().zip(&kinds).rev().fold(kind, |acc, ((x, _), k)| {
                pi(&x.text, Grade::DEFAULT, Icit::Explicit, k.clone(), acc)
            });
            self.record(e.id, ty.clone());
            let ty = self.eval(&ctx.env, &ty);
            ctx = self.declare_constant(&ctx, &e.name, ty);
            if let Some(u) = e.name.unique {
                let ops = e.ops.iter().filter_map(|op| op.name.unique).collect();
                self.effects.insert(u, EffectInfo { name: e.name.text.clone(), kinds, ops });
            }
        }
        ctx
    }

    /// Then their operations, once the data types they may mention are
    /// declared too.
    pub(super) fn declare_ops(&mut self, ctx: &Ctx, effects: &[hir::EffectDecl]) -> Ctx {
        let mut ctx = ctx.clone();
        for e in effects {
            let Some(info) = e.name.unique.and_then(|u| self.effects.get(&u).cloned()) else { continue };
            let n = e.params.len();
            let mut params = ctx.clone();
            for ((x, _), k) in e.params.iter().zip(&info.kinds) {
                let k_v = self.eval(&params.env, k);
                params = params.bind(Some(x), &x.text, k_v);
            }
            for (index, op) in e.ops.iter().enumerate() {
                let (sig_c, _) = self.check_type(&params, &op.ty);
                // the effect, applied to its parameters, under `depth`
                // binders of the operation's own
                let label = |depth: usize| {
                    let head = Rc::new(Core::Opaque(e.name.unique.unwrap_or(usize::MAX), e.name.text.clone()));
                    let applied = (0..n).fold(head, |acc, j| {
                        Rc::new(Core::App(acc, Rc::new(Core::Var(depth + n - j - 1, e.params[j].0.text.clone()))))
                    });
                    Rc::new(Core::Row(vec![applied], Vec::new()))
                };
                let Some((ty, implicit, arity)) = with_row(&sig_c, 0, &label) else {
                    if !matches!(*sig_c, Core::Error) {
                        self.error(op.id, TypeErrorKind::OpSignature(op.name.text.clone()));
                    }
                    ctx = self.declare_constant(&ctx, &op.name, Rc::new(Value::Error));
                    continue;
                };
                let ty = e
                    .params
                    .iter()
                    .zip(&info.kinds)
                    .rev()
                    .fold(ty, |acc, ((x, _), k)| pi(&x.text, Grade::ZERO, Icit::Implicit, k.clone(), acc));
                let ty = self.eval(&ctx.env, &ty);
                let shown = self.quote(ctx.lvl, ty.clone());
                self.record(op.id, shown);
                ctx = self.declare_constant(&ctx, &op.name, ty);
                if let (Some(u), Some(effect)) = (op.name.unique, e.name.unique) {
                    let info = OpInfo { name: op.name.text.clone(), effect, index, erased: n + implicit, arity, many: op.many };
                    self.ops.insert(u, info);
                }
            }
        }
        ctx
    }

    /// `handle body with clauses`, against `expected` if there is one.
    pub(super) fn handle(
        &mut self,
        ctx: &Ctx,
        nid: NodeId,
        body: &hir::Term,
        clauses: &[hir::HandleClause],
        expected: Option<VTy>,
    ) -> (Rc<Core>, VTy, Usage) {
        let err = || (Rc::new(Core::Error), Rc::new(Value::Error), Usage::unknown());
        // which effect: the one all the operations belong to
        let mut effect: Option<usize> = None;
        for c in clauses {
            let Some(op) = &c.op else { continue };
            let Some(u) = op.unique else { return err() };
            let Some(info) = self.ops.get(&u).cloned() else {
                self.error(c.id, TypeErrorKind::NotAnOperation(op.text.clone()));
                return err();
            };
            match effect {
                None => effect = Some(info.effect),
                Some(e) if e != info.effect => {
                    self.error(c.id, TypeErrorKind::HandlerMixesEffects);
                    return err();
                }
                Some(_) => {}
            }
        }
        let Some(effect) = effect else {
            self.error(nid, TypeErrorKind::HandlerHandlesNothing);
            return err();
        };
        let info = self.effects[&effect].clone();
        let clause_for = |op: usize| clauses.iter().filter(move |c| c.op.as_ref().and_then(|x| x.unique) == Some(op));
        let missing: Vec<String> = info
            .ops
            .iter()
            .filter(|op| clause_for(**op).next().is_none())
            .map(|op| self.ops[op].name.clone())
            .collect();
        if !missing.is_empty() {
            self.error(nid, TypeErrorKind::MissingOperations(missing));
            return err();
        }
        for op in &info.ops {
            if let Some(again) = clause_for(*op).nth(1) {
                self.error(again.id, TypeErrorKind::DuplicateOperation(self.ops[op].name.clone()));
                return err();
            }
        }
        for c in clauses {
            let (op, expected) = match &c.op {
                None => ("return".to_string(), 1),
                Some(op) => (op.text.clone(), op.unique.map_or(1, |u| self.ops[&u].arity) + 1),
            };
            if c.params.len() != expected {
                self.error(c.id, TypeErrorKind::ClauseParams { op, expected });
                return err();
            }
        }

        // what is handled: the effect, at the arguments the body has it at
        let (body_c, body_ty, mut body_usage) = self.infer(ctx, body);
        let (body_c, body_ty) = self.insert(ctx, body.id, body_c, body_ty);
        let row = self.row(body_usage.take_effects().into_iter());
        let parts = self.row_parts(&row);
        let ours: Vec<&(String, Rc<Value>)> = parts
            .labels
            .iter()
            .filter(|(_, l)| matches!(&*self.force(l.clone()), Value::Opaque(u, ..) if *u == effect))
            .collect();
        let args: Vec<Rc<Value>> = match ours.as_slice() {
            [(_, label)] => match &*self.force(label.clone()) {
                Value::Opaque(_, _, sp) => sp
                    .iter()
                    .filter_map(|e| match e {
                        super::value::Elim::App(a) => Some(a.clone()),
                        _ => None,
                    })
                    .collect(),
                _ => unreachable!("matched above"),
            },
            // the body does not have the effect: the handler does nothing,
            // but is still checked, at arguments nothing determines yet
            [] => (0..info.kinds.len())
                .map(|_| {
                    let m = self.fresh_meta(ctx, nid, format!("a parameter of {}", info.name));
                    self.eval(&ctx.env, &m)
                })
                .collect(),
            _ => {
                let row = self.quote(ctx.lvl, row);
                self.error(nid, TypeErrorKind::AmbiguousHandler { effect: info.name.clone(), row });
                return err();
            }
        };
        let handled: Vec<String> = ours.iter().map(|(k, _)| k.clone()).collect();
        let rest = self.row(
            parts.labels.iter().filter(|(k, _)| !handled.contains(k)).chain(&parts.tails).map(|(_, v)| v.clone()),
        );

        let result = match expected {
            Some(ty) => ty,
            None => {
                let m = self.fresh_meta(ctx, nid, "the type of this handle");
                self.eval(&ctx.env, &m)
            }
        };
        // what resuming does: to be worked out once the clauses are known
        let resumed = {
            let m = self.fresh_meta(ctx, nid, "the effects of resuming");
            self.eval(&ctx.env, &m)
        };

        // `return x -> ..`
        let (ret_core, mut ret_usage) = match clauses.iter().find(|c| c.op.is_none()) {
            Some(c) => {
                let [x] = c.params.as_slice() else {
                    self.error(c.id, TypeErrorKind::ClauseParams { op: "return".into(), expected: 1 });
                    return err();
                };
                let one = super::value::Poly::constant(Grade::ONE).into_value();
                let inner = ctx.bind(Some(x), &x.text, body_ty.clone()).graded(&one);
                let (c_body, mut usage) = self.check(&inner, &c.body, result.clone());
                self.leave_binder(&inner, c.id, x, one.clone(), &mut usage, false);
                (self.lam(ctx, x.text.clone(), &one, &body_ty, c_body), usage)
            }
            None => {
                if !self.subtype(ctx.lvl, body_ty.clone(), result.clone()) {
                    let (expected, actual) = (self.quote(ctx.lvl, result.clone()), self.quote(ctx.lvl, body_ty.clone()));
                    self.error(body.id, TypeErrorKind::Mismatch { expected, actual });
                }
                let one = super::value::Poly::constant(Grade::ONE).into_value();
                (self.lam(ctx, "x".into(), &one, &body_ty, Rc::new(Core::Var(0, "x".into()))), Usage::none())
            }
        };

        // `op x k -> ..`, for each operation in turn
        let mut ops = Vec::new();
        let mut ops_usage = Usage::none();
        for &op in &info.ops {
            let c = clause_for(op).next().expect("checked above");
            let op_info = self.ops[&op].clone();
            // its arguments, then the resumption (counted above)
            let (xs, k) = c.params.split_at(op_info.arity);
            let k = &k[0];
            // the operation's type at the handled arguments, then its own
            // implicit arguments as variables the clause knows nothing of
            let mut ty = self.constants[&op].clone();
            for a in &args {
                let forced = self.force(ty);
                let Value::Pi { cod, .. } = &*forced else { return err() };
                ty = self.apply(cod, a.clone());
            }
            let mut inner = ctx.clone();
            // each binder of the clause, with the context it is bound in and
            // its type, for the lambdas it becomes
            let mut erased = Vec::new();
            loop {
                let forced = self.force(ty.clone());
                let Value::Pi { name, icit: Icit::Implicit, dom, cod, .. } = &*forced else { break };
                let lvl = inner.lvl;
                erased.push((name.clone(), inner.clone(), dom.clone()));
                inner = inner.bind(None, name, dom.clone());
                ty = self.apply(cod, vvar(lvl, name.clone()));
            }
            // each argument, of the type the operation gives it (which may
            // mention the ones before)
            let mut inner_x = inner;
            let mut x_grades = Vec::new();
            let mut x_binders = Vec::new();
            for x in xs {
                let forced = self.force(ty);
                let Value::Pi { grade, dom, cod, .. } = &*forced else { return err() };
                let lvl = inner_x.lvl;
                x_binders.push((inner_x.clone(), dom.clone()));
                inner_x = inner_x.bind(Some(x), &x.text, dom.clone()).graded(grade);
                x_grades.push(grade.clone());
                ty = self.apply(cod, vvar(lvl, x.text.clone()));
            }
            let (op_result, _) = self.split_comp(ty);
            // k : (y : op_result) -> result ! resumed
            let k_ty = {
                let l = inner_x.lvl;
                let dom = self.quote(l, op_result.clone());
                let cod = Rc::new(Core::Comp(self.quote(l + 1, result.clone()), self.quote(l + 1, resumed.clone())));
                // (a resumption uses what it is resumed with once: any
                // value can be given to it)
                let k_c = pi("y", Grade::ONE, Icit::Explicit, dom, cod);
                self.eval(&inner_x.env, &k_c)
            };
            let k_grade = {
                let m = self.fresh_meta(&inner_x, c.id, format!("how often {} resumes", op_info.name));
                self.eval(&inner_x.env, &m)
            };
            let inner_k = inner_x.bind(Some(k), &k.text, k_ty.clone()).graded(&k_grade);
            let (c_body, mut usage) = self.check(&inner_k, &c.body, result.clone());
            // how often it resumes is how often it uses `k`, which is at
            // most once unless the operation says otherwise
            let unknown = usage.is_unknown();
            self.leave_binder(&inner_k, c.id, k, k_grade.clone(), &mut usage, false);
            if !op_info.many && !unknown {
                let resumes = self.to_poly(self.force(k_grade.clone())).as_constant();
                if !resumes.is_some_and(|g| g.fits(Grade::range(Grade::ZERO, Grade::ONE).expect("0 <= 1"))) {
                    let effect = info.name.clone();
                    self.error(c.id, TypeErrorKind::ResumesTooOften { op: op_info.name.clone(), effect });
                }
            }
            for (x, grade) in xs.iter().zip(&x_grades) {
                self.leave_binder(&inner_x, c.id, x, grade.clone(), &mut usage, false);
            }
            let body = self.lam(&inner_x, k.text.clone(), &k_grade, &k_ty, c_body);
            let body = xs
                .iter()
                .zip(&x_grades)
                .zip(&x_binders)
                .rev()
                .fold(body, |acc, ((x, grade), (at, dom))| self.lam(at, x.text.clone(), grade, dom, acc));
            let zero = super::value::Poly::constant(Grade::ZERO).into_value();
            let body = erased.iter().rev().fold(body, |acc, (name, at, dom)| self.lam(at, name.clone(), &zero, dom, acc));
            ops.push(OpClause { op, name: op_info.name.clone(), erased: erased.len(), body });
            // a clause runs as often as its operation is performed
            ops_usage = ops_usage + usage.scale(&super::value::Poly::constant(Grade::ANY));
        }

        // What the handle expression does: the body's other effects, and
        // what the clauses do.  Resuming is running the rest of the body
        // under this handler, which is the same again.
        let resumed_key = super::value::atom_key(&self.force(resumed.clone()));
        let clause_effects: Vec<Rc<Value>> = ret_usage.take_effects().into_iter().chain(ops_usage.take_effects()).collect();
        let clause_parts = self.row_parts(&self.row(clause_effects.into_iter()));
        let total = self.row(
            std::iter::once(rest)
                .chain(clause_parts.labels.iter().map(|(_, v)| v.clone()))
                .chain(clause_parts.tails.iter().filter(|(k, _)| *k != resumed_key).map(|(_, v)| v.clone())),
        );
        if !self.unify(ctx.lvl, resumed, total.clone()) {
            let row = self.quote(ctx.lvl, total.clone());
            self.error(nid, TypeErrorKind::AmbiguousHandler { effect: info.name.clone(), row });
        }
        let usage = body_usage + ret_usage + ops_usage + Usage::effect(total);
        (Rc::new(Core::Handle(body_c, ret_core, Rc::new(ops))), result, usage)
    }
}

fn pi(name: &str, grade: Grade, icit: Icit, dom: Rc<Core>, cod: Rc<Core>) -> Rc<Core> {
    Rc::new(Core::Pi {
        name: name.to_string(),
        icit,
        grade: Core::grade(grade),
        in_type: Core::grade(Grade::DEFAULT_IN_TYPE),
        dom,
        cod,
    })
}

/// An operation's type, `{..} -> a -> .. -> b`, with the effect's row put on
/// its last arrow; how many implicit arguments come first, and how many
/// explicit ones follow.  `None` if it is not a function type of at least one
/// explicit argument, whose result is pure.
fn with_row(c: &Rc<Core>, depth: usize, label: &dyn Fn(usize) -> Rc<Core>) -> Option<(Rc<Core>, usize, usize)> {
    match &**c {
        Core::Pi { name, icit: Icit::Implicit, grade, in_type, dom, cod } => {
            let (cod, n, arity) = with_row(cod, depth + 1, label)?;
            let pi = Core::Pi { name: name.clone(), icit: Icit::Implicit, grade: grade.clone(), in_type: in_type.clone(), dom: dom.clone(), cod };
            Some((Rc::new(pi), n + 1, arity))
        }
        Core::Pi { icit: Icit::Explicit, .. } => {
            let (ty, arity) = arguments(c, depth, label)?;
            Some((ty, 0, arity))
        }
        _ => None,
    }
}

/// The explicit arguments of an operation's type, and the row on the last.
fn arguments(c: &Rc<Core>, depth: usize, label: &dyn Fn(usize) -> Rc<Core>) -> Option<(Rc<Core>, usize)> {
    match &**c {
        Core::Pi { name, icit: Icit::Explicit, grade, in_type, dom, cod } => {
            let (cod, arity) = arguments(cod, depth + 1, label)?;
            let pi = Core::Pi { name: name.clone(), icit: Icit::Explicit, grade: grade.clone(), in_type: in_type.clone(), dom: dom.clone(), cod };
            Some((Rc::new(pi), arity + 1))
        }
        // (a result with a row of its own is not an operation's)
        Core::Comp(..) => None,
        _ => Some((Rc::new(Core::Comp(c.clone(), label(depth))), 0)),
    }
}
