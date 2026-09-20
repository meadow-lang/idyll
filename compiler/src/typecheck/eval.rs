//! Normalisation by evaluation.
//!
//! These would be plain functions in a calculus without metavariables.  They
//! are methods on the checker because solving a meta must retroactively
//! change what every value that mentions it *means*: a [`Value::Flex`] whose
//! meta has since been solved is no longer stuck.  [`Checker::force`] is what
//! makes that visible, and every place that inspects a value's head goes
//! through it first.

use super::core::{CaseAlt, Core, Head, Lvl};
use super::value::{vvar, Closure, Elim, Env, Poly, Spine, Value};
use super::{Checker, MetaEntry};
use crate::ast::Lit;
use crate::builtins::Prim;
use crate::grade::Grade;
use std::rc::Rc;

impl Checker {
    /// Expand the head of a value until it is no longer a solved metavariable.
    pub fn force(&self, mut v: Rc<Value>) -> Rc<Value> {
        loop {
            let next = match &*v {
                Value::Flex(m, sp) => match &self.metas[*m] {
                    MetaEntry::Solved(sol, _) => self.apply_spine(sol.clone(), sp),
                    MetaEntry::Unsolved(_) => return v,
                },
                // a primitive stuck on a meta computes once that meta is solved
                Value::Prim(p, args) => match self.reduce_prim(*p, args) {
                    Some(reduced) => reduced,
                    None => return v,
                },
                // grade arithmetic stuck on a meta likewise; this one cannot
                // expose a new head to expand, so there is no going round again
                Value::Grade(p) => return if self.is_stale(p) { self.renormalise(p).into_value() } else { v },
                Value::GradeRange(lo, hi) => return self.grade_range(lo.clone(), hi.clone()),
                _ => return v,
            };
            v = next;
        }
    }

    /// Compute a saturated primitive whose arguments are all literals.
    /// `None` means it is (still) stuck: too few arguments, or one of them is
    /// a variable or an unsolved meta.  No algebraic laws are applied, so
    /// `n + 0` stays as it is.
    fn reduce_prim(&self, p: Prim, args: &[Rc<Value>]) -> Option<Rc<Value>> {
        if args.len() != p.arity() {
            return None;
        }
        let args: Vec<Rc<Value>> = args.iter().map(|a| self.force(a.clone())).collect();
        let args: Vec<&Value> = args.iter().map(|a| &**a).collect();
        match (p, args.as_slice()) {
            (Prim::IntAdd, [Value::Lit(Lit::Int(a)), Value::Lit(Lit::Int(b))]) => {
                Some(Rc::new(Value::Lit(Lit::Int(a + b))))
            }
            (Prim::IntMul, [Value::Lit(Lit::Int(a)), Value::Lit(Lit::Int(b))]) => {
                Some(Rc::new(Value::Lit(Lit::Int(a * b))))
            }
            (Prim::IntSub, [Value::Lit(Lit::Int(a)), Value::Lit(Lit::Int(b))]) => {
                Some(Rc::new(Value::Lit(Lit::Int(a - b))))
            }
            (Prim::IntLt, [Value::Lit(Lit::Int(a)), Value::Lit(Lit::Int(b))]) => {
                Some(Rc::new(Value::Lit(Lit::Bool(a < b))))
            }
            (Prim::IntEq, [Value::Lit(Lit::Int(a)), Value::Lit(Lit::Int(b))]) => {
                Some(Rc::new(Value::Lit(Lit::Bool(a == b))))
            }
            _ => None,
        }
    }

    /// Instantiate a closure.
    pub fn apply(&self, cl: &Closure, v: Rc<Value>) -> Rc<Value> {
        self.eval(&cl.env.push(v), &cl.body)
    }

    pub fn vapp(&self, f: Rc<Value>, a: Rc<Value>) -> Rc<Value> {
        match &*f {
            Value::Lam(_, cl) => self.apply(cl, a),
            Value::Prim(p, args) if args.len() < p.arity() => {
                let mut args = args.clone();
                args.push(a);
                self.reduce_prim(*p, &args).unwrap_or_else(|| Rc::new(Value::Prim(*p, args)))
            }
            _ => self.stuck(f, Elim::App(a)),
        }
    }

    /// `unbox [v]` is `v`; otherwise the unboxing waits on the stuck head.
    pub fn vunbox(&self, v: Rc<Value>) -> Rc<Value> {
        match &*v {
            Value::BoxIntro(inner) => inner.clone(),
            _ => self.stuck(v, Elim::Unbox),
        }
    }

    /// Record an elimination on a stuck head.  Eliminating anything else is
    /// ill-typed — already reported, so keep going with an error value.
    fn stuck(&self, head: Rc<Value>, elim: Elim) -> Rc<Value> {
        let push = |sp: &Spine| {
            let mut sp = sp.clone();
            sp.push(elim.clone());
            sp
        };
        Rc::new(match &*head {
            Value::Flex(m, sp) => Value::Flex(*m, push(sp)),
            Value::Rigid(l, n, sp) => Value::Rigid(*l, n.clone(), push(sp)),
            Value::Opaque(u, n, sp) => Value::Opaque(*u, n.clone(), push(sp)),
            _ => Value::Error,
        })
    }

    pub fn apply_spine(&self, head: Rc<Value>, sp: &Spine) -> Rc<Value> {
        sp.iter().fold(head, |acc, elim| match elim {
            Elim::App(a) => self.vapp(acc, a.clone()),
            Elim::Unbox => self.vunbox(acc),
            Elim::Case(env, alts) => self.vcase(acc, env, alts),
        })
    }

    /// Evaluate a Core term in an environment.  Total: eliminating a value of
    /// the wrong shape yields [`Value::Error`] rather than panicking, because
    /// the checker keeps going after reporting an error.
    pub fn eval(&self, env: &Env, c: &Core) -> Rc<Value> {
        match c {
            Core::Type => Rc::new(Value::Type),
            Core::LitTy(b) => Rc::new(Value::LitTy(*b)),
            Core::Lit(l) => Rc::new(Value::Lit(l.clone())),
            Core::Prim(p) => Rc::new(Value::Prim(*p, Vec::new())),
            Core::Error => Rc::new(Value::Error),
            Core::Var(i, _) => env.get(*i).unwrap_or_else(|| Rc::new(Value::Error)),
            Core::Opaque(u, n) => Rc::new(Value::Opaque(*u, n.clone(), Vec::new())),
            Core::Meta(m) => match &self.metas[*m] {
                MetaEntry::Solved(sol, _) => sol.clone(),
                MetaEntry::Unsolved(_) => Rc::new(Value::Flex(*m, Vec::new())),
            },
            Core::Pi { name, icit, grade, in_type, dom, cod } => Rc::new(Value::Pi {
                name: name.clone(),
                icit: *icit,
                grade: self.eval(env, grade),
                in_type: self.eval(env, in_type),
                dom: self.eval(env, dom),
                cod: Closure { env: env.clone(), body: cod.clone() },
            }),
            Core::Lam(x, b) => {
                Rc::new(Value::Lam(x.clone(), Closure { env: env.clone(), body: b.clone() }))
            }
            Core::App(f, a) => {
                let f = self.eval(env, f);
                let a = self.eval(env, a);
                self.vapp(f, a)
            }
            Core::BoxTy(g, a) => Rc::new(Value::BoxTy(self.eval(env, g), self.eval(env, a))),
            Core::GradeLit(g) => Poly::constant(*g).into_value(),
            Core::GradeAdd(a, b) => (self.to_poly(self.eval(env, a)) + self.to_poly(self.eval(env, b))).into_value(),
            Core::GradeMul(a, b) => (&self.to_poly(self.eval(env, a)) * &self.to_poly(self.eval(env, b))).into_value(),
            Core::GradeRange(a, b) => self.grade_range(self.eval(env, a), self.eval(env, b)),
            Core::BoxIntro(e) => Rc::new(Value::BoxIntro(self.eval(env, e))),
            Core::Unbox(e) => self.vunbox(self.eval(env, e)),
            Core::Runtime(_, inner) => self.eval(env, inner),
            Core::Case(scrutinee, alts) => self.vcase(self.eval(env, scrutinee), env, alts),
            Core::Let(_, e, body) => {
                let e = self.eval(env, e);
                self.eval(&env.push(e), body)
            }
        }
    }

    /// Read a value back into Core.  `l` is the size of the context the
    /// result will live in.  Solved metas are expanded on the way, so quoting
    /// doubles as zonking: what comes out mentions only genuinely unsolved
    /// metas.
    pub fn quote(&self, l: Lvl, v: Rc<Value>) -> Rc<Core> {
        let v = self.force(v);
        Rc::new(match &*v {
            Value::Type => Core::Type,
            Value::LitTy(b) => Core::LitTy(*b),
            Value::Lit(x) => Core::Lit(x.clone()),
            Value::Error => Core::Error,
            Value::Flex(m, sp) => return self.quote_spine(l, Rc::new(Core::Meta(*m)), sp),
            Value::Rigid(k, n, sp) => {
                return self.quote_spine(l, Rc::new(Core::Var(l - k - 1, n.clone())), sp)
            }
            Value::Opaque(u, n, sp) => {
                return self.quote_spine(l, Rc::new(Core::Opaque(*u, n.clone())), sp)
            }
            Value::Prim(p, args) => {
                return args.iter().fold(Rc::new(Core::Prim(*p)), |acc, a| {
                    Rc::new(Core::App(acc, self.quote(l, a.clone())))
                })
            }
            Value::Pi { name, icit, grade, in_type, dom, cod } => Core::Pi {
                name: name.clone(),
                icit: *icit,
                grade: self.quote(l, grade.clone()),
                in_type: self.quote(l, in_type.clone()),
                dom: self.quote(l, dom.clone()),
                cod: self.quote(l + 1, self.apply(cod, vvar(l, name.clone()))),
            },
            Value::Lam(x, b) => {
                let b = self.quote(l + 1, self.apply(b, vvar(l, x.clone())));
                Core::Lam(x.clone(), b)
            }
            Value::BoxTy(g, a) => Core::BoxTy(self.quote(l, g.clone()), self.quote(l, a.clone())),
            Value::Grade(p) => {
                return self.poly_core(p, &mut |atom| Some(self.quote(l, atom.clone()))).expect("quoting cannot fail")
            }
            Value::GradeRange(lo, hi) => Core::GradeRange(self.quote(l, lo.clone()), self.quote(l, hi.clone())),
            Value::BoxIntro(e) => Core::BoxIntro(self.quote(l, e.clone())),
        })
    }

    fn quote_spine(&self, l: Lvl, head: Rc<Core>, sp: &Spine) -> Rc<Core> {
        sp.iter().fold(head, |acc, elim| {
            Rc::new(match elim {
                Elim::App(a) => Core::App(acc, self.quote(l, a.clone())),
                Elim::Unbox => Core::Unbox(acc),
                Elim::Case(env, alts) => {
                    let alts = alts.iter().map(|alt| self.quote_alt(l, env, alt)).collect();
                    Core::Case(acc, Rc::new(alts))
                }
            })
        })
    }

    /// Read an alternative back: run its body on fresh variables.
    fn quote_alt(&self, l: Lvl, env: &Env, alt: &CaseAlt) -> CaseAlt {
        let mut inner = env.clone();
        for (i, x) in alt.vars.iter().enumerate() {
            inner = inner.push(vvar(l + i, x.clone()));
        }
        let body = self.quote(l + alt.vars.len(), self.eval(&inner, &alt.body));
        CaseAlt { body, ..alt.clone() }
    }

    /// `case v of alts`: take the alternative for `v`'s constructor, with its
    /// fields for the variables.  If `v` is not a constructor yet, wait.
    pub fn vcase(&self, v: Rc<Value>, env: &Env, alts: &Rc<Vec<CaseAlt>>) -> Rc<Value> {
        let chosen = match &*v {
            Value::Lit(Lit::Bool(b)) => alts.iter().find(|a| a.head == Head::Bool(*b)).map(|a| (a, Vec::new())),
            Value::Opaque(u, _, sp) => alts.iter().find(|a| a.head == Head::Con(*u)).and_then(|a| {
                // a constructor's fields are its last arguments (the first
                // are the type's parameters)
                let fields: Option<Vec<Rc<Value>>> = sp[sp.len().checked_sub(a.vars.len())?..]
                    .iter()
                    .map(|e| match e {
                        Elim::App(f) => Some(f.clone()),
                        _ => None,
                    })
                    .collect();
                Some((a, fields?))
            }),
            _ => None,
        };
        match chosen {
            Some((alt, fields)) => {
                let inner = fields.into_iter().fold(env.clone(), |e, f| e.push(f));
                self.eval(&inner, &alt.body)
            }
            None => self.stuck(v, Elim::Case(env.clone(), alts.clone())),
        }
    }
}

// ---------------------------------------------------------------------------
// Grades
// ---------------------------------------------------------------------------

impl Checker {
    /// A grade value as a polynomial: the normal form if it has one, and
    /// otherwise — a variable, a meta — itself as an atom.
    pub fn to_poly(&self, v: Rc<Value>) -> Poly {
        let v = self.force(v);
        match &*v {
            Value::Grade(p) => p.clone(),
            _ => Poly::atom(v),
        }
    }

    /// Whether some atom of `p` has been solved since `p` was built.
    fn is_stale(&self, p: &Poly) -> bool {
        p.terms().any(|(atoms, _)| atoms.iter().any(|a| !Rc::ptr_eq(&self.force(a.clone()), a)))
    }

    /// Rebuild a polynomial from its atoms' current meanings.
    pub fn renormalise(&self, p: &Poly) -> Poly {
        p.terms().fold(Poly::zero(), |sum, (atoms, g)| {
            let monomial = atoms.iter().fold(Poly::constant(g), |acc, a| &acc * &self.to_poly(a.clone()));
            sum + monomial
        })
    }

    /// `lo..hi`.  Between constants it is a constant (or an error, if empty,
    /// which the elaborator has reported); otherwise it waits.
    pub fn grade_range(&self, lo: Rc<Value>, hi: Rc<Value>) -> Rc<Value> {
        let (lo, hi) = (self.to_poly(lo), self.to_poly(hi));
        match (lo.as_constant(), hi.as_constant()) {
            (Some(a), Some(b)) => match Grade::range(a, b) {
                Some(g) => Poly::constant(g).into_value(),
                None => Rc::new(Value::Error),
            },
            _ => Rc::new(Value::GradeRange(lo.into_value(), hi.into_value())),
        }
    }

    /// Read a polynomial back as a sum of products, given how to read back an
    /// atom (quoting and pattern unification's renaming differ only there).
    pub fn poly_core(
        &self,
        p: &Poly,
        atom: &mut dyn FnMut(&Rc<Value>) -> Option<Rc<Core>>,
    ) -> Option<Rc<Core>> {
        let mut sum: Option<Rc<Core>> = None;
        for (atoms, g) in self.renormalise(p).terms() {
            // a coefficient of one is left unwritten, unless it is all there is
            let mut product = (atoms.is_empty() || g != Grade::ONE).then(|| Core::grade(g));
            for a in atoms {
                let a = atom(a)?;
                product = Some(match product {
                    None => a,
                    Some(p) => Rc::new(Core::GradeMul(p, a)),
                });
            }
            let product = product.expect("a monomial with no atoms has a coefficient");
            sum = Some(match sum {
                None => product,
                Some(s) => Rc::new(Core::GradeAdd(s, product)),
            });
        }
        Some(sum.unwrap_or_else(|| Core::grade(Grade::ZERO)))
    }
}
