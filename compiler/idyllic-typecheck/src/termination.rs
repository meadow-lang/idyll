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
//! A recursion on an `Int` has no constructor to take apart, so it is seen
//! to end by a measure instead, where a test bounds it: each `Int` parameter
//! `n` has three, each a parameter of its own to the size-change test (see
//! [`Slot`]). Counting down, `f (n - 1)` under a test that `n` is above a
//! number (`if n <= 0 then .. else`, `| n > 0`) makes `n` smaller; counting
//! up, `f (i + 1)` under a test that `i` is below a number, or below another
//! parameter `n` passed on as it is (`if i < n then`), makes `n - i` smaller.
//! Up and down are measures apart: a loop that counts down and then up
//! again does not end. `f 0 = ..; f n = f (n - 1)` is not seen to end, as it
//! does not for a negative `n`. The measures are only looked for when the
//! group is not seen to end without them, so a group that never counts
//! costs nothing more.
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
/// callee's, how the argument passed relates to the parameter. With
/// measures, each "parameter" is a [`Slot`].
type Matrix = Vec<Vec<Rel>>;

/// What the size-change test compares, of a definition of `n` parameters:
/// the parameters themselves, and with measures (see the module docs) each
/// one counted down, each counted up to a number, and each counted up to
/// another, `Gap(q, p)` being `q - p`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Param(usize),
    Down(usize),
    Up(usize),
    Gap(usize, usize),
}

/// The most parameters a definition may have for its gaps to be measured:
/// there are as many as pairs of them.
const MOST_GAPS: usize = 6;

fn slots(n: usize, measured: bool) -> usize {
    match (measured, n <= MOST_GAPS) {
        (false, _) => n,
        (true, true) => 3 * n + n * n,
        (true, false) => 3 * n,
    }
}

fn slot(i: usize, n: usize, measured: bool) -> Slot {
    match () {
        _ if !measured || i < n => Slot::Param(i),
        _ if i < 2 * n => Slot::Down(i - n),
        _ if i < 3 * n => Slot::Up(i - 2 * n),
        _ => Slot::Gap((i - 3 * n) / n, (i - 3 * n) % n),
    }
}

/// A variable, or a number, in a comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Atom {
    Var(Lvl),
    Lit,
}

/// What a branch knows, of a test it is under: `lo < hi`, or `lo <= hi`.
#[derive(Clone, Copy, Debug)]
struct Fact {
    lo: Atom,
    hi: Atom,
}

/// How a recursive group's termination came out: whether it ends, and if
/// not, whether one of its calls counts an `Int` down with nothing to stop
/// it, `f 0 = ..; f n = f (n - 1)`.
pub(super) struct Ends {
    pub(super) ends: bool,
    pub(super) counted_down: bool,
}

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
    pub(super) fn check_termination(&mut self, ctx: &Ctx, members: &[(usize, Rc<Core>)]) -> Ends {
        let index: HashMap<usize, usize> = members.iter().enumerate().map(|(i, (u, _))| (*u, i)).collect();
        // with metavariables solved and definitions inlined, so that calls
        // made through them are seen
        let bodies: Vec<Rc<Core>> =
            members.iter().map(|(_, core)| self.quote(ctx.lvl, self.eval(&ctx.env, core))).collect();
        let arities: Vec<usize> = members.iter().map(|(_, core)| lambdas_of(core).0).collect();
        // the parameters alone, and then, if a call counts, with measures
        let walked = |measured: bool| {
            let mut calls: HashSet<(usize, usize, Matrix)> = HashSet::new();
            let mut counts = false;
            let mut counted_down = false;
            for (f, body) in bodies.iter().enumerate() {
                let (n, inner) = lambdas(body, arities[f]);
                let mut walk = Walk {
                    base: ctx.lvl,
                    origin: (0..n).map(|p| Some((p, Rel::Same))).collect(),
                    splits: Vec::new(),
                    facts: Vec::new(),
                    measured,
                    counts: false,
                    counted_down: false,
                    index: &index,
                    arities: &arities,
                    from: f,
                    calls: &mut calls,
                };
                walk.term(inner, ctx.lvl + n);
                counts |= walk.counts;
                counted_down |= walk.counted_down;
            }
            let sizes: Vec<usize> = arities.iter().map(|&n| slots(n, measured)).collect();
            (terminates(calls, &sizes), counts, counted_down)
        };
        let (mut ends, counts, _) = walked(false);
        let mut counted_down = false;
        if !ends && counts {
            (ends, _, counted_down) = walked(true);
        }
        if !ends {
            return Ends { ends: false, counted_down };
        }
        // only a group with no enclosing variables (see the module docs); a
        // definition with effects is not one, since it is not a parameter
        if ctx.binders.iter().any(|b| b.info == BinderInfo::Bound) {
            return Ends { ends: true, counted_down: false };
        }
        for (u, core) in members {
            let (arity, body) = lambdas_of(core);
            self.transparent.insert(*u, Unfold { env: ctx.env.clone(), arity, body: body.clone(), on_case: false });
        }
        Ends { ends: true, counted_down: false }
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
    /// what the tests around here say (with measures)
    facts: Vec<Fact>,
    /// whether calls are summarised with measures
    measured: bool,
    /// whether a call passes a parameter plus or minus a number
    counts: bool,
    /// whether one counts down with no test to stop it
    counted_down: bool,
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

    /// The parameter a variable is, as it is.
    fn param(&self, c: &Core, lvl: Lvl) -> Option<usize> {
        match c {
            Core::Runtime(_, inner) => self.param(inner, lvl),
            Core::Var(..) => match self.origin_of(c, lvl) {
                Some((p, Rel::Same)) => Some(p),
                _ => None,
            },
            _ => None,
        }
    }

    /// A parameter plus or minus a number: which, and by how much.
    fn stepped(&self, c: &Core, lvl: Lvl) -> Option<(usize, i64)> {
        let Core::App(f, b) = c else {
            return match c {
                Core::Runtime(_, inner) => self.stepped(inner, lvl),
                _ => None,
            };
        };
        let Core::App(op, a) = &**f else { return None };
        let number = |c: &Core| match c {
            Core::Lit(idyllic_ast::Lit::Int(k)) if *k >= 1 => Some(*k),
            _ => None,
        };
        match &**op {
            Core::Prim(idyllic_builtins::Prim::IntSub) => Some((self.param(a, lvl)?, -number(b)?)),
            Core::Prim(idyllic_builtins::Prim::IntAdd) => match (self.param(a, lvl), number(b), number(a), self.param(b, lvl)) {
                (Some(p), Some(k), ..) | (.., Some(k), Some(p)) => Some((p, k)),
                _ => None,
            },
            _ => None,
        }
    }

    /// The parameter a fact's variable is, as it is.
    fn fact_param(&self, a: Atom) -> Option<usize> {
        match a {
            Atom::Var(l) => match self.origin.get(l.checked_sub(self.base)?) {
                Some(Some((p, Rel::Same))) => Some(*p),
                _ => None,
            },
            Atom::Lit => None,
        }
    }

    /// A variable or a number, in a comparison.
    fn atom(c: &Core, lvl: Lvl) -> Option<Atom> {
        match c {
            Core::Runtime(_, inner) => Self::atom(inner, lvl),
            Core::Var(i, _) => Some(Atom::Var(lvl - i - 1)),
            Core::Lit(idyllic_ast::Lit::Int(_)) => Some(Atom::Lit),
            _ => None,
        }
    }

    /// What a test says where it is true, and where it is false: `a < b`,
    /// or else `b <= a`; `not t` (a `case` of `t` giving the other `Bool`)
    /// the other way round.
    fn condition(c: &Core, lvl: Lvl) -> Option<(Fact, Fact)> {
        match c {
            Core::Runtime(_, inner) => Self::condition(inner, lvl),
            Core::App(f, b) => match &**f {
                Core::App(op, a) if matches!(**op, Core::Prim(idyllic_builtins::Prim::IntLt)) => {
                    let (a, b) = (Self::atom(a, lvl)?, Self::atom(b, lvl)?);
                    Some((Fact { lo: a, hi: b }, Fact { lo: b, hi: a }))
                }
                _ => None,
            },
            Core::Case(scrutinee, alts) => {
                let (yes, no) = Self::condition(scrutinee, lvl)?;
                let gives = |b: bool| {
                    alts.iter().find(|a| a.head == super::core::Head::Bool(b)).and_then(|a| match &*a.body {
                        Core::Lit(idyllic_ast::Lit::Bool(v)) => Some(*v),
                        _ => None,
                    })
                };
                match (gives(true)?, gives(false)?) {
                    (true, false) => Some((yes, no)),
                    (false, true) => Some((no, yes)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// How argument `arg` relates to slot `to` of the callee, from slot
    /// `from` of the caller, the callee's other arguments being `args`.
    fn related(&self, from: Slot, to: Slot, args: &[&Core], lvl: Lvl) -> Rel {
        let param = |j: usize| args.get(j).and_then(|a| self.param(a, lvl));
        let stepped = |j: usize| args.get(j).and_then(|a| self.stepped(a, lvl));
        let below = |p: usize| self.facts.iter().any(|f| f.lo == Atom::Lit && self.fact_param(f.hi) == Some(p));
        let above = |p: usize| self.facts.iter().any(|f| self.fact_param(f.lo) == Some(p) && f.hi == Atom::Lit);
        let under = |p: usize, q: usize| self.facts.iter().any(|f| self.fact_param(f.lo) == Some(p) && self.fact_param(f.hi) == Some(q));
        match (from, to) {
            (Slot::Param(p), Slot::Param(j)) => match args.get(j).and_then(|a| self.origin_of(a, lvl)) {
                Some((q, rel)) if q == p => rel,
                _ => Rel::Unknown,
            },
            (Slot::Down(p), Slot::Down(j)) => match (param(j), stepped(j)) {
                (Some(q), _) if q == p => Rel::Same,
                (_, Some((q, k))) if q == p && k < 0 && below(p) => Rel::Smaller,
                _ => Rel::Unknown,
            },
            (Slot::Up(p), Slot::Up(j)) => match (param(j), stepped(j)) {
                (Some(q), _) if q == p => Rel::Same,
                (_, Some((q, k))) if q == p && k > 0 && above(p) => Rel::Smaller,
                _ => Rel::Unknown,
            },
            (Slot::Gap(q, p), Slot::Gap(jq, jp)) if q != p && param(jq) == Some(q) => match (param(jp), stepped(jp)) {
                (Some(r), _) if r == p => Rel::Same,
                (_, Some((r, k))) if r == p && k > 0 && under(p, q) => Rel::Smaller,
                _ => Rel::Unknown,
            },
            _ => Rel::Unknown,
        }
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
            // what comes out of a split is smaller than what went in; and a
            // branch of a test knows what the test said
            Core::Case(scrutinee, alts) => {
                self.term(scrutinee, lvl);
                let whole = self.origin_of(scrutinee, lvl);
                let o = whole.map(|(p, _)| (p, Rel::Smaller));
                let test = if self.measured { Self::condition(scrutinee, lvl) } else { None };
                for alt in alts.iter() {
                    if let super::core::Head::Con(c) = alt.head {
                        self.splits.push((c, (lvl..lvl + alt.vars.len()).collect(), whole));
                    }
                    let fact = match (alt.head, test) {
                        (super::core::Head::Bool(true), Some((yes, _))) => Some(yes),
                        (super::core::Head::Bool(false), Some((_, no))) => Some(no),
                        _ => None,
                    };
                    self.facts.extend(fact);
                    self.under(alt.vars.len(), o, &alt.body, lvl);
                    if fact.is_some() {
                        self.facts.pop();
                    }
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
                        let (n, m) = (self.arities[self.from], self.arities[to]);
                        let measured = self.measured;
                        let matrix: Matrix = (0..slots(n, measured))
                            .map(|r| {
                                (0..slots(m, measured))
                                    .map(|c| self.related(slot(r, n, measured), slot(c, m, measured), &args, lvl))
                                    .collect()
                            })
                            .collect();
                        for j in 0..m {
                            if let Some((p, k)) = args.get(j).and_then(|a| self.stepped(a, lvl)) {
                                self.counts = true;
                                let below = self.facts.iter().any(|f| f.lo == Atom::Lit && self.fact_param(f.hi) == Some(p));
                                self.counted_down |= k < 0 && measured && !below;
                            }
                        }
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
/// not change.  Each must make some parameter smaller. `arities` are how
/// many each definition's matrices have (its [`Slot`]s, with measures).
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
