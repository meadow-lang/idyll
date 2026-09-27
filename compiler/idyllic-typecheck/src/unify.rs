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

use super::core::{CaseAlt, Core, Lvl, MetaVar};
use super::value::{vvar, Elim, Spine, Value};
use super::{Checker, MetaEntry};
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
            _ => self.unify(l, actual, expected),
        }
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
            // Grade arithmetic: equal when the normal forms are.  (A lone
            // meta never gets here: it is `Flex`, and was solved above.)
            (Value::Grade(_), _) | (_, Value::Grade(_)) => {
                let (p, q) = (self.to_poly(t.clone()), self.to_poly(u.clone()));
                p.monomials().eq(q.monomials()) && p.monomials().all(|m| p.coefficient(m) == q.coefficient(m))
            }
            _ => false,
        }
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
        self.metas[m] = MetaEntry::Solved(value, info);
        true
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
