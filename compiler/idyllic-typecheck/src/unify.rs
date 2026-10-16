//! Unification, including Miller pattern unification for metavariables.
//!
//! This replaces the pure conversion check a meta-free calculus would use:
//! deciding `t == u` can now *solve* metas as a side effect, which is exactly
//! how a bidirectional elaborator learns the types it was not told.
//!
//! The solvable case is the *pattern fragment*: a meta applied to distinct
//! bound variables, `?m x y z =? t`.  Such an equation has a unique solution
//! `?m := \x y z. t`, provided
//!
//! * `t` does not itself mention `?m` (the occurs check), and
//! * every variable free in `t` is among `x y z` (the scope check),
//!
//! both of which fall out of [`Checker::rename`], which quotes `t` through
//! the inverted spine and fails if either condition is violated.  Outside
//! that fragment we simply report a mismatch rather than postponing the
//! constraint.
//!
//! Grades are unified like any other term, so `(x : [1] a) -> b` and
//! `(x : [2] a) -> b` are different types, as are `a [1]` and `a [2]`; there
//! is no subtyping by grade approximation.  Grade *arithmetic* is compared
//! in normal form (see [`super::value::Poly`]), which identifies `r + r` with
//! `2 * r` but does not solve equations: `?m + 1 =? 3` is a mismatch.

use super::core::{CaseAlt, Core, Level, Lvl, MetaVar};
use super::value::{vvar, Elim, Spine, Value};
use super::{Checker, MetaEntry, MetaInfo, MetaOrigin, TypeErrorKind};
use std::collections::HashMap;
use std::rc::Rc;

impl Checker {
    /// Whether something of type `actual` may be used where `expected` is
    /// wanted: the two are convertible, or both are universes and `actual`
    /// is the smaller.  That is cumulativity, and it is only this deep: a
    /// function returning `Type` is not accepted where one returning
    /// `Type 1` is.
    pub fn subtype(&mut self, l: Lvl, actual: Rc<Value>, expected: Rc<Value>) -> bool {
        let actual = self.force(actual);
        let expected = self.force(expected);
        match (&*actual, &*expected) {
            (Value::Type(i), Value::Type(j)) => i <= j,
            // a row resumed at most once is one that may be resumed more
            (Value::LitTy(idyllic_builtins::Builtin::Effect), _) if self.row_kind(&expected) == Some(true) => true,
            // A function that uses its argument within what is allowed is
            // one that may: `(x : [1] a) -> b` where `a -> b` (any number of
            // uses) is wanted, as a constructor is passed to `map`. The
            // grade fits, the domains are the same, and the results are
            // compared in turn.
            // And a function whose results have fewer effects than are
            // allowed is one that may be called there: a function type is
            // covariant in its row, as in Koka. So `Int -> Int ! Log` fits
            // `Int -> Int ! {Ask, Log}`, and passing `logged` to `compose`
            // leaves `compose`'s row open for what `addAsk` does.
            (
                Value::Pi { name, icit: i1, grade: g1, in_type: t1, dom: d1, cod: c1 },
                Value::Pi { icit: i2, grade: g2, in_type: t2, dom: d2, cod: c2, .. },
            ) if i1 == i2 => {
                let known = |c: &mut Self, g: &Rc<Value>| c.to_poly(c.force(g.clone())).as_constant();
                let grades = match (known(self, g1), known(self, g2)) {
                    (Some(a), Some(b)) => a == b || a.fits(b),
                    _ => self.unify(l, g1.clone(), g2.clone()),
                };
                let x = vvar(l, name.clone());
                let (c1, c2) = (self.apply(c1, x.clone()), self.apply(c2, x));
                grades && self.unify(l, t1.clone(), t2.clone()) && self.unify(l, d1.clone(), d2.clone()) && self.subtype(l + 1, c1, c2)
            }
            (Value::Comp(a, e1), Value::Comp(b, e2)) => {
                let (a, b, e1, e2) = (a.clone(), b.clone(), e1.clone(), e2.clone());
                self.subtype(l, a, b) && self.effects_fit(l, e1, e2)
            }
            // a pure result fits wherever effects are allowed
            (a, Value::Comp(b, _)) if !matches!(a, Value::Flex(..)) => {
                let b = b.clone();
                self.subtype(l, actual.clone(), b)
            }
            _ => self.unify(l, actual, expected),
        }
    }

    /// A new row variable beside `tail`, which is one still to be found:
    /// made where it was, so that it may stand for what `tail` may. What a
    /// row variable is solved with keeps such a one at its end, open for
    /// more: `?e := {Log | ?t}`, not `?e := Log`. Whatever is still open at
    /// the end is closed to the empty row.
    pub(super) fn sibling(&mut self, tail: &Rc<Value>) -> Option<Rc<Value>> {
        let Value::Flex(m, sp) = &*self.force(tail.clone()) else { return None };
        let (m, sp) = (*m, sp.clone());
        let info = match &self.metas[m] {
            MetaEntry::Unsolved(info) | MetaEntry::Solved(_, info) => info.clone(),
        };
        let fresh = self.new_meta(info.node, info.origin, info.ty);
        self.opened.push((fresh, sp.len()));
        Some(Rc::new(Value::Flex(fresh, sp)))
    }

    /// Decide whether two values are convertible, solving metas as needed.
    /// Returns `false` on failure; the caller turns that into a diagnostic,
    /// so one failed unification does not abort the whole module.
    pub fn unify(&mut self, l: Lvl, t: Rc<Value>, u: Rc<Value>) -> bool {
        let t = self.force(t);
        let u = self.force(u);
        let fresh = || vvar(l, "_");
        match (&*t, &*u) {
            // an already-reported error unifies with anything, so a single
            // mistake produces a single message
            (Value::Error, _) | (_, Value::Error) => true,
            (Value::Lam(_, a), Value::Lam(_, b)) => {
                let lhs = self.apply(a, fresh());
                let rhs = self.apply(b, fresh());
                self.unify(l + 1, lhs, rhs)
            }
            // eta: a lambda equals anything that behaves like it under
            // application
            (Value::Lam(_, a), _) => {
                let lhs = self.apply(a, fresh());
                let rhs = self.vapp(u.clone(), fresh());
                self.unify(l + 1, lhs, rhs)
            }
            (_, Value::Lam(_, b)) => {
                let lhs = self.vapp(t.clone(), fresh());
                let rhs = self.apply(b, fresh());
                self.unify(l + 1, lhs, rhs)
            }
            (Value::Type(i), Value::Type(j)) => i == j,
            (Value::LitTy(a), Value::LitTy(b)) => a == b,
            (Value::Lit(a), Value::Lit(b)) => a == b,
            (
                Value::Pi { icit, grade, in_type, dom, cod, .. },
                Value::Pi { icit: icit2, grade: grade2, in_type: in_type2, dom: dom2, cod: cod2, .. },
            ) => {
                if icit != icit2
                    || !self.unify(l, grade.clone(), grade2.clone())
                    || !self.unify(l, in_type.clone(), in_type2.clone())
                    || !self.unify(l, dom.clone(), dom2.clone())
                {
                    return false;
                }
                let lhs = self.apply(cod, fresh());
                let rhs = self.apply(cod2, fresh());
                self.unify(l + 1, lhs, rhs)
            }
            (Value::BoxTy(g, a), Value::BoxTy(g2, a2)) => {
                self.unify(l, g.clone(), g2.clone()) && self.unify(l, a.clone(), a2.clone())
            }
            (Value::GradeRange(lo, hi), Value::GradeRange(lo2, hi2)) => {
                self.unify(l, lo.clone(), lo2.clone()) && self.unify(l, hi.clone(), hi2.clone())
            }
            (Value::BoxIntro(a), Value::BoxIntro(b)) => self.unify(l, a.clone(), b.clone()),
            // two handles are the same when they read back the same
            (Value::Handle(..), Value::Handle(..)) => self.quote(l, t.clone()) == self.quote(l, u.clone()),
            (Value::Comp(a, e), Value::Comp(b, f)) => {
                self.unify(l, a.clone(), b.clone()) && self.unify(l, e.clone(), f.clone())
            }
            (Value::Rigid(k, _, sp), Value::Rigid(k2, _, sp2)) if k == k2 => {
                let (sp, sp2) = (sp.clone(), sp2.clone());
                self.unify_spine(l, &sp, &sp2)
            }
            (Value::Opaque(a, _, sp), Value::Opaque(b, _, sp2)) if a == b => {
                let (sp, sp2) = (sp.clone(), sp2.clone());
                self.unify_spine(l, &sp, &sp2)
            }
            (Value::Flex(m, sp), Value::Flex(m2, sp2)) if m == m2 => {
                let (sp, sp2) = (sp.clone(), sp2.clone());
                self.unify_spine(l, &sp, &sp2)
            }
            // Two stuck primitives are equal when their arguments are.  This
            // is purely structural — `n + 1` and `1 + n` are *not* identified —
            // and forcing has already computed anything that could compute.
            (Value::Stuck(h, sp), Value::Stuck(h2, sp2)) => {
                let (h, h2, sp, sp2) = (h.clone(), h2.clone(), sp.clone(), sp2.clone());
                self.unify(l, h, h2) && self.unify_spine(l, &sp, &sp2)
            }
            (Value::Prim(p, args), Value::Prim(p2, args2)) if p == p2 && args.len() == args2.len() => {
                let (args, args2) = (args.clone(), args2.clone());
                args.iter().zip(&args2).all(|(x, y)| self.unify(l, x.clone(), y.clone()))
            }
            (Value::Flex(m, sp), _) => {
                let (m, sp) = (*m, sp.clone());
                self.solve(l, m, &sp, u.clone())
            }
            (_, Value::Flex(m, sp)) => {
                let (m, sp) = (*m, sp.clone());
                self.solve(l, m, &sp, t.clone())
            }
            // a pure type where one with effects is wanted, or the other way
            (Value::Comp(a, e), _) => {
                let (a, e) = (a.clone(), e.clone());
                self.unify(l, a, u.clone()) && self.unify(l, e, super::value::empty_row())
            }
            (_, Value::Comp(b, f)) => {
                let (b, f) = (b.clone(), f.clone());
                self.unify(l, t.clone(), b) && self.unify(l, super::value::empty_row(), f)
            }
            (Value::Row(_), _) | (_, Value::Row(_)) => self.unify_rows(l, t.clone(), u.clone()),
            // Grade arithmetic: equal when the normal forms are.  (A lone
            // meta never gets here: it is `Flex`, and was solved above.)
            (Value::Grade(_), _) | (_, Value::Grade(_)) => {
                let (p, q) = (self.to_poly(t.clone()), self.to_poly(u.clone()));
                p.monomials().eq(q.monomials()) && p.monomials().all(|m| p.coefficient(m) == q.coefficient(m))
            }
            _ => false,
        }
    }

    /// Two rows are equal when they have the same effects and the same row
    /// variables.  What one has and the other lacks can be made up by a
    /// row variable that is still a metavariable: `{Ask | ?e} =? {Ask, Log}`
    /// solves `?e := Log`.
    fn unify_rows(&mut self, l: Lvl, t: Rc<Value>, u: Rc<Value>) -> bool {
        self.unify_rows_pairing(l, t, u, true)
    }

    /// The head of an effect with parameters, `State` of `State s`: a row
    /// has at most one effect of each, so two rows' effects with the same
    /// head are the same effect, their parameters to be unified.
    pub(super) fn effect_head(&self, v: &Rc<Value>) -> Option<(bool, usize)> {
        match &*self.force(v.clone()) {
            Value::Opaque(u, _, spine) if !spine.is_empty() => Some((false, *u)),
            Value::Rigid(lvl, _, spine) if !spine.is_empty() => Some((true, *lvl)),
            _ => None,
        }
    }

    /// [`Checker::unify_rows`]; with `pair`, effects with parameters that
    /// differ only in those (`State ?s` and `State s`) are first unified
    /// with each other, and the rows compared again.
    fn unify_rows_pairing(&mut self, l: Lvl, t: Rc<Value>, u: Rc<Value>, pair: bool) -> bool {
        // a row with an error in it has been reported already
        if self.row_has_error(&t) || self.row_has_error(&u) {
            return true;
        }
        if pair {
            let (a, b) = (self.row_parts(&t), self.row_parts(&u));
            let extra = |x: &super::value::Row, y: &super::value::Row| -> Vec<Rc<Value>> {
                x.labels.iter().filter(|(k, _)| !y.labels.iter().any(|(k2, _)| k2 == k)).map(|(_, v)| v.clone()).collect()
            };
            let (la, lb) = (extra(&a, &b), extra(&b, &a));
            let heads_a: Vec<Option<(bool, usize)>> = la.iter().map(|x| self.effect_head(x)).collect();
            let heads_b: Vec<Option<(bool, usize)>> = lb.iter().map(|y| self.effect_head(y)).collect();
            let mut paired = false;
            for (x, h) in la.iter().zip(&heads_a) {
                let Some(h) = h else { continue };
                let once = |hs: &[Option<(bool, usize)>]| hs.iter().filter(|g| g.as_ref() == Some(h)).count() == 1;
                if !(once(&heads_a) && once(&heads_b)) {
                    continue;
                }
                let y = lb.iter().zip(&heads_b).find(|(_, g)| g.as_ref() == Some(h)).map(|(y, _)| y.clone()).expect("once");
                if !self.unify(l, x.clone(), y) {
                    return false;
                }
                paired = true;
            }
            if paired {
                return self.unify_rows_pairing(l, t, u, false);
            }
        }
        let (a, b) = (self.row_parts(&t), self.row_parts(&u));
        let only = |x: &super::value::Row, y: &super::value::Row| {
            let labels: Vec<Rc<Value>> =
                x.labels.iter().filter(|(k, _)| !y.labels.iter().any(|(k2, _)| k2 == k)).map(|(_, v)| v.clone()).collect();
            let tails: Vec<Rc<Value>> =
                x.tails.iter().filter(|(k, _)| !y.tails.iter().any(|(k2, _)| k2 == k)).map(|(_, v)| v.clone()).collect();
            (labels, tails)
        };
        let ((la, ta), (lb, tb)) = (only(&a, &b), only(&b, &a));
        if la.is_empty() && ta.is_empty() && lb.is_empty() && tb.is_empty() {
            return true;
        }
        // one side's extra is one metavariable: it is the other side's extra
        let lone_meta = |ts: &[Rc<Value>], ls: &[Rc<Value>]| ls.is_empty() && ts.len() == 1 && matches!(*ts[0], Value::Flex(..));
        if lone_meta(&ta, &la) {
            let rest = self.row(lb.into_iter().chain(tb));
            return self.unify(l, ta[0].clone(), rest);
        }
        if lone_meta(&tb, &lb) {
            let rest = self.row(la.into_iter().chain(ta));
            return self.unify(l, rest, tb[0].clone());
        }
        // Several row variables still to be found on one side, and nothing
        // else on it the other lacks... or effects too: they are made one
        // (`?a := ?b`), which is enough for what they have to stand for
        // between them, and the rows compared again.
        let flex = |ts: &[Rc<Value>]| ts.len() > 1 && ts.iter().all(|t| matches!(**t, Value::Flex(..)));
        if flex(&ta) || flex(&tb) {
            let merge = |c: &mut Self, ts: &[Rc<Value>]| {
                ts.len() < 2 || !ts.iter().all(|t| matches!(**t, Value::Flex(..))) || ts[1..].iter().all(|x| c.unify(l, x.clone(), ts[0].clone()))
            };
            return merge(self, &ta) && merge(self, &tb) && self.unify_rows_pairing(l, t, u, false);
        }
        // Each side has a row variable still to be found, and effects the
        // other lacks: both are the other's effects and one fresh variable
        // more, as in Leijen's row unification. `{Log | ?a} =? {Ask | ?b}`
        // gives `?a := {Ask | ?t}` and `?b := {Log | ?t}`.
        if let ([a_tail], [b_tail]) = (ta.as_slice(), tb.as_slice()) {
            if matches!(**a_tail, Value::Flex(..)) && matches!(**b_tail, Value::Flex(..)) {
                let (a_tail, b_tail) = (a_tail.clone(), b_tail.clone());
                if let Some(fresh) = self.sibling(&a_tail) {
                    let for_a = self.row(lb.into_iter().chain(std::iter::once(fresh.clone())));
                    let for_b = self.row(la.into_iter().chain(std::iter::once(fresh)));
                    return self.unify(l, a_tail, for_a) && self.unify(l, b_tail, for_b);
                }
            }
        }
        false
    }

    fn unify_spine(&mut self, l: Lvl, a: &Spine, b: &Spine) -> bool {
        a.len() == b.len()
            && a.iter().zip(b).all(|pair| match pair {
                (Elim::App(x), Elim::App(y)) => self.unify(l, x.clone(), y.clone()),
                (Elim::Unbox, Elim::Unbox) => true,
                // two stuck cases agree if their alternatives do, run on the
                // same fresh fields
                (Elim::Case(env, alts), Elim::Case(env2, alts2)) => {
                    alts.len() == alts2.len()
                        && alts.iter().zip(alts2.iter()).all(|(x, y)| {
                            if x.head != y.head || x.vars.len() != y.vars.len() {
                                return false;
                            }
                            let (mut e, mut e2) = (env.clone(), env2.clone());
                            for i in 0..x.vars.len() {
                                e = e.push(vvar(l + i, "_"));
                                e2 = e2.push(vvar(l + i, "_"));
                            }
                            let (lhs, rhs) = (self.eval(&e, &x.body), self.eval(&e2, &y.body));
                            self.unify(l + x.vars.len(), lhs, rhs)
                        })
                }
                _ => false,
            })
    }

    // -----------------------------------------------------------------------
    // Pattern unification
    // -----------------------------------------------------------------------

    /// Solve `?m sp =? rhs`.
    fn solve(&mut self, gamma: Lvl, m: MetaVar, sp: &Spine, rhs: Rc<Value>) -> bool {
        let Some(pren) = self.invert(gamma, sp) else { return false };
        let Some(body) = self.rename(m, &pren, rhs) else { return false };
        // abstract over the spine: ?m := \x y z. body
        let solution = pren
            .names
            .iter()
            .rev()
            .fold(body, |acc, n| Rc::new(Core::Lam(n.clone(), acc)));
        let value = self.eval(&Default::default(), &solution);
        let info = match &self.metas[m] {
            MetaEntry::Unsolved(info) | MetaEntry::Solved(_, info) => info.clone(),
        };
        // A solution that is too large is reported here, where the reason is
        // known, and the meta is solved with an error so that nothing else
        // complains about it.
        if let Some(kind) = self.universe_violation(&info, &value).or_else(|| self.row_kind_violation(&info, &value)) {
            self.error(info.node, kind);
            self.metas[m] = MetaEntry::Solved(Rc::new(Value::Error), info);
            return true;
        }
        self.metas[m] = MetaEntry::Solved(value, info);
        true
    }

    /// Whether a solution for a meta that must be a type in `Type n` is a
    /// type in a larger universe.
    fn universe_violation(&self, info: &MetaInfo, solution: &Rc<Value>) -> Option<TypeErrorKind> {
        let mt = info.ty.as_ref()?;
        let Value::Type(allowed) = *self.force(mt.ty.clone()) else { return None };
        // the solution, back in the context the meta was made in
        let at = mt.types.len();
        let inst = mt.spine.iter().fold(solution.clone(), |f, &k| self.vapp(f, vvar(k, mt.types[k].0.clone())));
        let actual = self.sort_of(&mut mt.types.to_vec(), inst.clone())?;
        let what = match &info.origin {
            MetaOrigin::Hole => "this hole".to_string(),
            MetaOrigin::Inferred(desc) => desc.clone(),
        };
        (actual > allowed).then(|| TypeErrorKind::UniverseTooLarge { what, solution: self.quote(at, inst), allowed, actual })
    }

    /// Whether a solution for a row variable of kind `Effect` holds an
    /// operation that may be resumed more than once, which such a row may
    /// not: code polymorphic in it keeps its continuations on a stack.
    fn row_kind_violation(&self, info: &MetaInfo, solution: &Rc<Value>) -> Option<TypeErrorKind> {
        let mt = info.ty.as_ref()?;
        if self.row_kind(&mt.ty) != Some(false) {
            return None;
        }
        let at = mt.types.len();
        let inst = mt.spine.iter().fold(solution.clone(), |f, &k| self.vapp(f, vvar(k, mt.types[k].0.clone())));
        if !self.row_is_many(&mt.types, &inst) {
            return None;
        }
        let what = match &info.origin {
            MetaOrigin::Hole => "this hole".to_string(),
            MetaOrigin::Inferred(desc) => desc.clone(),
        };
        Some(TypeErrorKind::ManyEffectsInRow { what, row: self.quote(at, self.shown_row(inst)) })
    }

    /// A lower bound on the universe a type lives in, given the types of the
    /// variables in scope by level.  `None` if nothing is known, as for a
    /// type that is still a meta.
    pub(super) fn sort_of(&self, types: &mut Vec<(String, Rc<Value>)>, v: Rc<Value>) -> Option<Level> {
        let v = self.force(v);
        match &*v {
            Value::Type(n) => Some(n + 1),
            Value::LitTy(_) => Some(0),
            Value::BoxTy(_, a) | Value::Comp(a, _) => self.sort_of(types, a.clone()),
            Value::Pi { name, dom, cod, .. } => {
                let d = self.sort_of(types, dom.clone());
                let l = types.len();
                types.push((name.clone(), dom.clone()));
                let c = self.sort_of(types, self.apply(cod, vvar(l, name.clone())));
                types.pop();
                d.max(c)
            }
            Value::Rigid(k, _, sp) => {
                let ty = types.get(*k)?.1.clone();
                self.neutral_sort(ty, sp)
            }
            Value::Opaque(u, _, sp) => {
                let ty = self.constants.get(u)?.clone();
                self.neutral_sort(ty, sp)
            }
            _ => None,
        }
    }

    /// The universe of a neutral type, from the type of its head.
    fn neutral_sort(&self, mut ty: Rc<Value>, sp: &Spine) -> Option<Level> {
        for elim in sp {
            let forced = self.force(ty);
            ty = match (elim, &*forced) {
                (Elim::App(a), Value::Pi { cod, .. }) => self.apply(cod, a.clone()),
                (Elim::Unbox, Value::BoxTy(_, a)) => a.clone(),
                _ => return None,
            };
        }
        match *self.force(ty) {
            Value::Type(n) => Some(n),
            _ => None,
        }
    }

    /// Invert a meta's spine, if it is in the pattern fragment: every
    /// elimination must be an application to a distinct bound variable.
    fn invert(&self, gamma: Lvl, sp: &Spine) -> Option<PartialRenaming> {
        let mut map = HashMap::new();
        let mut names = Vec::new();
        for (dom, elim) in sp.iter().enumerate() {
            let Elim::App(v) = elim else { return None };
            match &*self.force(v.clone()) {
                Value::Rigid(k, n, args) if args.is_empty() && !map.contains_key(k) => {
                    map.insert(*k, dom);
                    names.push(n.clone());
                }
                // a repeated variable, or anything that is not a plain
                // variable, puts this equation outside the pattern fragment
                _ => return None,
            }
        }
        Some(PartialRenaming { dom: sp.len(), cod: gamma, map, names })
    }

    /// Quote a value into the meta's context, renaming variables through
    /// `pren`.  `None` if the occurs check or the scope check fails.
    fn rename(&self, m: MetaVar, pren: &PartialRenaming, v: Rc<Value>) -> Option<Rc<Core>> {
        let v = self.force(v);
        Some(Rc::new(match &*v {
            Value::Type(n) => Core::Type(*n),
            Value::LitTy(b) => Core::LitTy(*b),
            Value::Lit(l) => Core::Lit(l.clone()),
            Value::Error => Core::Error,
            Value::Flex(m2, sp) => {
                // occurs check: the solution may not mention the meta being
                // solved
                if *m2 == m {
                    return None;
                }
                return self.rename_spine(m, pren, Rc::new(Core::Meta(*m2)), sp);
            }
            Value::Rigid(k, n, sp) => {
                // scope check: this variable must be one the meta may depend on
                let k2 = *pren.map.get(k)?;
                return self.rename_spine(m, pren, Rc::new(Core::Var(pren.dom - k2 - 1, n.clone())), sp);
            }
            // constants are in scope everywhere
            Value::Opaque(u, n, sp) => {
                return self.rename_spine(m, pren, Rc::new(Core::Opaque(*u, n.clone())), sp);
            }
            Value::Stuck(h, sp) => {
                let head = self.rename(m, pren, h.clone())?;
                return self.rename_spine(m, pren, head, sp);
            }
            Value::Prim(p, args) => {
                return args.iter().try_fold(Rc::new(Core::Prim(*p)), |acc, a| {
                    Some(Rc::new(Core::App(acc, self.rename(m, pren, a.clone())?)))
                });
            }
            Value::Lam(x, cl) => {
                let body = self.apply(cl, vvar(pren.cod, x.clone()));
                Core::Lam(x.clone(), self.rename(m, &pren.lift(x), body)?)
            }
            Value::Pi { name, icit, grade, in_type, dom, cod } => {
                let body = self.apply(cod, vvar(pren.cod, name.clone()));
                Core::Pi {
                    name: name.clone(),
                    icit: *icit,
                    grade: self.rename(m, pren, grade.clone())?,
                    in_type: self.rename(m, pren, in_type.clone())?,
                    dom: self.rename(m, pren, dom.clone())?,
                    cod: self.rename(m, &pren.lift(name), body)?,
                }
            }
            Value::BoxTy(g, a) => Core::BoxTy(self.rename(m, pren, g.clone())?, self.rename(m, pren, a.clone())?),
            Value::Grade(p) => return self.poly_core(p, &mut |atom| self.rename(m, pren, atom.clone())),
            Value::GradeRange(lo, hi) => {
                Core::GradeRange(self.rename(m, pren, lo.clone())?, self.rename(m, pren, hi.clone())?)
            }
            Value::BoxIntro(e) => Core::BoxIntro(self.rename(m, pren, e.clone())?),
            Value::Comp(t, e) => Core::Comp(self.rename(m, pren, t.clone())?, self.rename(m, pren, e.clone())?),
            Value::Handle(env, c) => {
                let Core::Handle(body, ret, ops) = &**c else { unreachable!("a handle") };
                let part = |c: &Rc<Core>| self.rename(m, pren, self.eval(env, c));
                let ops = ops
                    .iter()
                    .map(|op| Some(super::core::OpClause { body: part(&op.body)?, ..op.clone() }))
                    .collect::<Option<_>>()?;
                Core::Handle(part(body)?, part(ret)?, Rc::new(ops))
            }
            Value::Row(r) => Core::Row(
                r.labels.iter().map(|(_, a)| self.rename(m, pren, a.clone())).collect::<Option<_>>()?,
                r.tails.iter().map(|(_, a)| self.rename(m, pren, a.clone())).collect::<Option<_>>()?,
            ),
        }))
    }

    fn rename_spine(&self, m: MetaVar, pren: &PartialRenaming, head: Rc<Core>, sp: &Spine) -> Option<Rc<Core>> {
        sp.iter().try_fold(head, |acc, elim| {
            Some(Rc::new(match elim {
                Elim::App(v) => Core::App(acc, self.rename(m, pren, v.clone())?),
                Elim::Unbox => Core::Unbox(acc),
                Elim::Case(env, alts) => {
                    let mut renamed = Vec::new();
                    for alt in alts.iter() {
                        let (mut inner, mut under) = (env.clone(), None::<PartialRenaming>);
                        for x in &alt.vars {
                            let p = under.as_ref().unwrap_or(pren);
                            inner = inner.push(vvar(p.cod, x.clone()));
                            under = Some(p.lift(x));
                        }
                        let body = self.rename(m, under.as_ref().unwrap_or(pren), self.eval(&inner, &alt.body))?;
                        renamed.push(CaseAlt { body, ..alt.clone() });
                    }
                    Core::Case(acc, Rc::new(renamed))
                }
            }))
        })
    }
}

/// The inverse of a meta's spine: a map from the levels of the *current*
/// context back to the levels of the meta's own (smaller) context.
struct PartialRenaming {
    /// size of the meta's context, i.e. the spine length
    dom: Lvl,
    /// size of the context the right-hand side lives in
    cod: Lvl,
    /// codomain level to domain level; absent means out of scope
    map: HashMap<Lvl, Lvl>,
    /// binder names, in domain order, for readable solutions
    names: Vec<String>,
}

impl PartialRenaming {
    /// Go under a binder on both sides.
    fn lift(&self, name: &str) -> PartialRenaming {
        let mut map = self.map.clone();
        map.insert(self.cod, self.dom);
        let mut names = self.names.clone();
        names.push(name.to_string());
        PartialRenaming { dom: self.dom + 1, cod: self.cod + 1, map, names }
    }
}
