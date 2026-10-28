//! Termination checking, and what it buys: recursive definitions that
//! compute in types.
//!
//! A recursive definition is an opaque constant while its group is being
//! checked, as it always used to be.  Once the group is checked, its
//! elaborated case trees are examined, as Agda does, with the size-change
//! principle of Lee, Jones & Ben-Amram: each recursive call is summarised
//! by how its arguments relate to the caller's parameters — *smaller* (a
//! variable some split took out of the parameter), *the same*, or *unknown*
//! — and a group terminates if every way round a loop of calls, composed
//! with itself until nothing new appears, makes some parameter smaller.
//! That covers structural recursion on any argument, mutual recursion, and
//! lexicographic orders like Ackermann's.
//!
//! A group that terminates becomes **transparent**: its members unfold when
//! applied to enough arguments, so `S n + m` computes to `S (n + m)` inside a
//! type.  They unfold only when that makes progress — when the case tree
//! can choose its way down to a right-hand side.  A call whose case tree is
//! stuck on a variable stays a call, `n + m`, rather than becoming the
//! stuck `case` it would unfold to, which conversion checking could not see
//! the end of.
//!
//! A group that does not terminate stays opaque, and nothing is reported:
//! it still runs, it just cannot compute in a type.  (Divergence as an
//! effect, phase 2 of the roadmap, is what will give that a name.)  Only
//! top-level groups are made transparent: one nested under a binder is
//! identified by its binding site, so its instances could not be told apart.

use super::core::{CaseAlt, Core, Head, Lvl};
use super::value::{Elim, Env, Spine, Value};
use super::{BinderInfo, Checker, Ctx};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// A recursive definition that may unfold.
#[derive(Clone)]
pub(super) struct Unfold {
    /// the environment it was defined in
    env: Env,
    /// how many arguments it takes before it can compute
    arity: usize,
    /// its body under those arguments' lambdas: the case tree
    body: Rc<Core>,
    /// only where a `case` waits on it: an instance, which a projection of
    /// one of its fields looks into (`Elem (List Int)` is `Int`), but which
    /// a type otherwise shows by its name
    pub(super) on_case: bool,
}

impl Unfold {
    /// One another package compiled: its case tree is closed, so it needs no
    /// environment.
    pub(super) fn closed(arity: usize, body: Rc<Core>, on_case: bool) -> Unfold {
        Unfold { env: Env::empty(), arity, body, on_case }
    }

    /// The case tree, under its parameters, closed for another package to
    /// unfold: normalised where it was defined, its references to the
    /// top-level definitions there made constants.
    pub(super) fn export(&self, chk: &Checker, ctx: &Ctx) -> (usize, Rc<Core>) {
        let top = self.env.len();
        let env = (0..self.arity).fold(self.env.clone(), |env, i| env.push(super::value::vvar(top + i, "x")));
        let body = chk.quote(top + self.arity, chk.eval(&env, &self.body));
        (self.arity, chk.close_at(ctx, top + self.arity, top, &body))
    }
}

/// How a call's argument relates to one of the caller's parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Rel {
    Unknown,
    Same,
    Smaller,
}

impl Rel {
    /// Through a call and then another.
    fn then(self, next: Rel) -> Rel {
        match (self, next) {
            (Rel::Unknown, _) | (_, Rel::Unknown) => Rel::Unknown,
            (Rel::Same, Rel::Same) => Rel::Same,
            _ => Rel::Smaller,
        }
    }
}

/// A call's summary: for each of the caller's parameters and each of the
/// callee's, how the argument passed relates to the parameter.
type Matrix = Vec<Vec<Rel>>;

fn compose(a: &Matrix, b: &Matrix, callee_params: usize) -> Matrix {
    a.iter()
        .map(|row| {
            (0..callee_params)
                .map(|k| row.iter().zip(b).map(|(r, b_row)| r.then(b_row[k])).max().unwrap_or(Rel::Unknown))
                .collect()
        })
        .collect()
}

impl Checker {
    /// After a recursive group has been checked: whether every loop of calls
    /// in it makes something smaller.  If so, and it is not inside a
    /// function, its members unfold from now on.
    pub(super) fn check_termination(&mut self, ctx: &Ctx, members: &[(usize, Rc<Core>)]) -> bool {
        let index: HashMap<usize, usize> = members.iter().enumerate().map(|(i, (u, _))| (*u, i)).collect();
        // with metavariables solved and definitions inlined, so that calls
        // made through them are seen
        let bodies: Vec<Rc<Core>> =
            members.iter().map(|(_, core)| self.quote(ctx.lvl, self.eval(&ctx.env, core))).collect();
        let arities: Vec<usize> = members.iter().map(|(_, core)| lambdas_of(core).0).collect();
        let mut calls: HashSet<(usize, usize, Matrix)> = HashSet::new();
        for (f, body) in bodies.iter().enumerate() {
            let (n, inner) = lambdas(body, arities[f]);
            let mut walk = Walk {
                base: ctx.lvl,
                origin: (0..n).map(|p| Some((p, Rel::Same))).collect(),
                splits: Vec::new(),
                index: &index,
                arities: &arities,
                from: f,
                calls: &mut calls,
            };
            walk.term(inner, ctx.lvl + n);
        }
        if !terminates(calls, &arities) {
            return false;
        }
        // only a group with no enclosing variables (see the module docs); a
        // definition with effects is not one, since it is not a parameter
        if ctx.binders.iter().any(|b| b.info == BinderInfo::Bound) {
            return true;
        }
        for (u, core) in members {
            let (arity, body) = lambdas_of(core);
            self.transparent.insert(*u, Unfold { env: ctx.env.clone(), arity, body: body.clone(), on_case: false });
        }
        true
    }

    /// The instances of a recursive group checked at the top level that do
    /// not unfold already (one that refers to itself, through a default
    /// method, does not terminate as a definition): each unfolds where a
    /// `case` reads a type or a superclass off it, so that a field of a
    /// known instance computes in a type (see [`Unfold::on_case`]).
    pub(super) fn unfold_instances(&mut self, ctx: &Ctx, members: &[(usize, Rc<Core>)]) {
        if ctx.binders.iter().any(|b| b.info == BinderInfo::Bound) {
            return;
        }
        for (u, core) in members {
            if self.hints.contains(u) && !self.transparent.contains_key(u) {
                let (arity, body) = lambdas_of(core);
                self.transparent.insert(*u, Unfold { env: ctx.env.clone(), arity, body: body.clone(), on_case: true });
            }
        }
    }

    /// A transparent definition applied to `sp`, if its case tree can choose
    /// a right-hand side.
    pub(super) fn unfold(&self, u: usize, sp: &Spine) -> Option<Rc<Value>> {
        let f = self.transparent.get(&u)?;
        if sp.len() < f.arity || (f.on_case && !matches!(sp.get(f.arity), Some(Elim::Case(..)))) {
            return None;
        }
        let mut env = f.env.clone();
        for elim in &sp[..f.arity] {
            let Elim::App(a) = elim else { return None };
            env = env.push(a.clone());
        }
        let v = self.eval_tree(&env, &f.body)?;
        Some(self.apply_spine(v, &sp[f.arity..]))
    }

    /// A transparent definition applied to `sp`, unfolded even where its
    /// case tree cannot choose, its `case`s left waiting: `Elem c @{d}` at a
    /// dictionary `d` the context binds is `case d of Container _ e .. -> e`,
    /// a type a variable's field holds, which the typer reads a
    /// representation off.
    pub(super) fn unfold_stuck(&self, u: usize, sp: &Spine) -> Option<Rc<Value>> {
        let f = self.transparent.get(&u)?;
        if sp.len() < f.arity {
            return None;
        }
        let mut env = f.env.clone();
        for elim in &sp[..f.arity] {
            let Elim::App(a) = elim else { return None };
            env = env.push(a.clone());
        }
        let v = self.eval(&env, &f.body);
        Some(self.force(self.apply_spine(v, &sp[f.arity..])))
    }

    /// An instance a `case` waits on, unfolded (see [`Unfold::on_case`]):
    /// when the case reads a field a type may compute with, a type or a
    /// superclass (a method stays a call of the instance's).
    pub(super) fn opened(&self, v: &Rc<Value>, alts: &[CaseAlt]) -> Option<Rc<Value>> {
        let Value::Opaque(u, _, sp) = &**v else { return None };
        let [alt] = alts else { return None };
        let Head::Con(ctor) = alt.head else { return None };
        let mut body = &alt.body;
        while let Core::Runtime(_, inner) = &**body {
            body = inner;
        }
        let Core::Var(k, _) = &**body else { return None };
        let field = alt.vars.len().checked_sub(k + 1)?;
        if !self.ctors.get(&ctor).is_some_and(|c| c.opens.get(field) == Some(&true)) {
            return None;
        }
        let f = self.transparent.get(u).filter(|f| f.on_case && sp.len() >= f.arity)?;
        if self.opening.borrow().contains(u) {
            return None;
        }
        let mut env = f.env.clone();
        for elim in &sp[..f.arity] {
            let Elim::App(a) = elim else { return None };
            env = env.push(a.clone());
        }
        self.opening.borrow_mut().push(*u);
        let record = self.eval_tree(&env, &f.body);
        self.opening.borrow_mut().retain(|o| o != u);
        Some(self.force(self.apply_spine(record?, &sp[f.arity..])))
    }

    /// Evaluate a case tree, or give up where it cannot choose.
    fn eval_tree(&self, env: &Env, c: &Core) -> Option<Rc<Value>> {
        match c {
            Core::Runtime(_, inner) => self.eval_tree(env, inner),
            Core::Let(_, e, body) => self.eval_tree(&env.push(self.eval(env, e)), body),
            Core::Case(scrutinee, alts) => {
                let v = self.force(self.eval(env, scrutinee));
                // (an instance a field is read off: what it is)
                let v = match self.choose(&v, alts) {
                    None => self.opened(&v, alts).unwrap_or(v),
                    Some(_) => v,
                };
                let (alt, fields) = self.choose(&v, alts)?;
                let inner = fields.into_iter().fold(env.clone(), |e, f| e.push(f));
                self.eval_tree(&inner, &alt.body)
            }
            _ => Some(self.eval(env, c)),
        }
    }

    /// The alternative for a value, and the values its variables stand for.
    pub(super) fn choose<'a>(&self, v: &Value, alts: &'a [CaseAlt]) -> Option<(&'a CaseAlt, Vec<Rc<Value>>)> {
        match v {
            Value::Lit(idyllic_ast::Lit::Bool(b)) => {
                alts.iter().find(|a| a.head == super::core::Head::Bool(*b)).map(|a| (a, Vec::new()))
            }
            Value::Opaque(u, _, sp) => {
                let alt = alts.iter().find(|a| a.head == super::core::Head::Con(*u))?;
                // the constructor's arguments, all of them
                let args: Option<Vec<Rc<Value>>> = sp[sp.len().checked_sub(alt.vars.len())?..]
                    .iter()
                    .map(|e| match e {
                        Elim::App(f) => Some(f.clone()),
                        _ => None,
                    })
                    .collect();
                Some((alt, args?))
            }
            _ => None,
        }
    }
}

/// Up to `most` of the lambdas a term starts with: how many, and what is
/// under them.
fn lambdas(mut c: &Rc<Core>, most: usize) -> (usize, &Rc<Core>) {
    let mut n = 0;
    while let (true, Core::Lam(_, body)) = (n < most, &**c) {
        n += 1;
        c = body;
    }
    (n, c)
}

/// How many arguments a recursive definition takes before its case tree.
pub(super) fn arity(core: &Rc<Core>) -> usize {
    lambdas_of(core).0
}

/// How many lambdas an elaborated term starts with, looking through the
/// notes elaboration leaves on them, and what is under them.
fn lambdas_of(mut c: &Rc<Core>) -> (usize, &Rc<Core>) {
    let mut n = 0;
    loop {
        match &**c {
            Core::Lam(_, body) => {
                n += 1;
                c = body;
            }
            Core::Runtime(_, inner) if matches!(**inner, Core::Lam(..)) => c = inner,
            _ => return (n, c),
        }
    }
}

/// Where a variable came from: which of the definition's parameters, and
/// how it relates to it.
type Origin = Option<(usize, Rel)>;

/// Collecting a definition's calls to its group.
struct Walk<'a> {
    /// the level of the definition's first parameter
    base: Lvl,
    /// for each variable from `base` on: which parameter it came out of,
    /// and whether it is smaller than it
    origin: Vec<Origin>,
    /// the splits around here: the constructor, the levels its arguments
    /// were bound at, and where the variable split came from.  That
    /// constructor applied to those variables is the variable again, so
    /// `ack (S m) n` passes `ack`'s own first argument back.
    splits: Vec<(usize, Vec<Lvl>, Origin)>,
    index: &'a HashMap<usize, usize>,
    arities: &'a [usize],
    from: usize,
    calls: &'a mut HashSet<(usize, usize, Matrix)>,
}

impl Walk<'_> {
    fn origin_of(&self, c: &Core, lvl: Lvl) -> Origin {
        if let Core::Var(i, _) = c {
            let at = (lvl - i - 1).checked_sub(self.base)?;
            return self.origin.get(at).copied().flatten();
        }
        // a split variable, put back together
        let mut args = Vec::new();
        let mut head = c;
        while let Core::App(f, a) = head {
            args.push(&**a);
            head = f;
        }
        let Core::Opaque(u, _) = head else { return None };
        args.reverse();
        self.splits.iter().rev().find_map(|(c, vars, origin)| {
            let same = c == u
                && args.len() == vars.len()
                && args.iter().zip(vars).all(|(a, v)| matches!(a, Core::Var(i, _) if lvl - i - 1 == *v));
            same.then_some(*origin).flatten()
        })
    }

    fn under(&mut self, n: usize, origin: Origin, body: &Core, lvl: Lvl) {
        self.origin.extend(std::iter::repeat_n(origin, n));
        self.term(body, lvl + n);
        self.origin.truncate(self.origin.len() - n);
    }

    fn term(&mut self, c: &Core, lvl: Lvl) {
        match c {
            Core::Lam(_, body) => self.under(1, None, body, lvl),
            Core::Pi { grade, in_type, dom, cod, .. } => {
                self.term(grade, lvl);
                self.term(in_type, lvl);
                self.term(dom, lvl);
                self.under(1, None, cod, lvl);
            }
            Core::Let(_, e, body) => {
                self.term(e, lvl);
                let o = self.origin_of(e, lvl);
                self.under(1, o, body, lvl);
            }
            // what comes out of a split is smaller than what went in
            Core::Case(scrutinee, alts) => {
                self.term(scrutinee, lvl);
                let whole = self.origin_of(scrutinee, lvl);
                let o = whole.map(|(p, _)| (p, Rel::Smaller));
                for alt in alts.iter() {
                    if let super::core::Head::Con(c) = alt.head {
                        self.splits.push((c, (lvl..lvl + alt.vars.len()).collect(), whole));
                    }
                    self.under(alt.vars.len(), o, &alt.body, lvl);
                    if let super::core::Head::Con(_) = alt.head {
                        self.splits.pop();
                    }
                }
            }
            Core::App(..) | Core::Opaque(..) => {
                let mut args = Vec::new();
                let mut head = c;
                while let Core::App(f, a) = head {
                    args.push(&**a);
                    head = f;
                }
                args.reverse();
                if let Core::Opaque(u, _) = head {
                    if let Some(&to) = self.index.get(u) {
                        let params = self.arities[self.from];
                        let matrix = (0..params)
                            .map(|p| {
                                (0..self.arities[to])
                                    .map(|j| match args.get(j).and_then(|a| self.origin_of(a, lvl)) {
                                        Some((q, rel)) if q == p => rel,
                                        _ => Rel::Unknown,
                                    })
                                    .collect()
                            })
                            .collect();
                        self.calls.insert((self.from, to, matrix));
                    }
                } else {
                    self.term(head, lvl);
                }
                for a in args {
                    self.term(a, lvl);
                }
            }
            Core::Row(labels, tails) => labels.iter().chain(tails).for_each(|c| self.term(c, lvl)),
            Core::Handle(body, ret, ops) => {
                self.term(body, lvl);
                self.term(ret, lvl);
                ops.iter().for_each(|op| self.term(&op.body, lvl));
            }
            Core::BoxTy(a, b) | Core::Comp(a, b) | Core::GradeAdd(a, b) | Core::GradeMul(a, b) | Core::GradeRange(a, b) => {
                self.term(a, lvl);
                self.term(b, lvl);
            }
            Core::BoxIntro(a) | Core::Unbox(a) | Core::Runtime(_, a) => self.term(a, lvl),
            Core::Type(_)
            | Core::LitTy(_)
            | Core::Lit(_)
            | Core::Prim(_)
            | Core::Var(..)
            | Core::Meta(_)
            | Core::GradeLit(_)
            | Core::Error => {}
        }
    }
}

/// The size-change test: close the calls under composition, and look at
/// every call from a definition to itself that composing with itself does
/// not change.  Each must make some parameter smaller.
fn terminates(calls: HashSet<(usize, usize, Matrix)>, arities: &[usize]) -> bool {
    let mut all = calls.clone();
    let mut fresh: Vec<(usize, usize, Matrix)> = calls.iter().cloned().collect();
    while let Some((f, g, m)) = fresh.pop() {
        let next: Vec<(usize, usize, Matrix)> = all
            .iter()
            .filter(|(g2, ..)| *g2 == g)
            .map(|(_, h, m2)| (f, *h, compose(&m, m2, arities[*h])))
            .chain(all.iter().filter(|(.., h, _)| *h == f).map(|(e, _, m0)| (*e, g, compose(m0, &m, arities[g]))))
            .collect();
        for c in next {
            if all.insert(c.clone()) {
                fresh.push(c);
            }
        }
    }
    all.iter().filter(|(f, g, _)| f == g).all(|(_, g, m)| {
        let square = compose(m, m, arities[*g]);
        square != *m || (0..m.len().min(arities[*g])).any(|i| m[i][i] == Rel::Smaller)
    })
}
