//! The semantic domain: values, spines and closures.
//!
//! This module is deliberately data-only.  Evaluating or comparing a value
//! means consulting the metacontext (a solved meta has to be expanded before
//! you can look at the value's head), so `eval`, `quote` and `unify` are
//! methods on the [`super::Checker`] and live in [`super::eval`] and
//! [`super::unify`].
//!
//! Stuck computations are kept in *head-spine* form — a head that cannot
//! compute ([`Value::Rigid`], [`Value::Flex`], [`Value::Opaque`]) and the
//! eliminations waiting on it — because pattern unification needs to look at
//! a meta's whole argument list at once.

use super::core::{CaseAlt, Core, Lvl, MetaVar};
use idyllic_ast::{Icit, Lit};
use idyllic_builtins::{Builtin, Prim};
use crate::grade::Grade;
use std::collections::BTreeMap;
use std::ops::{Add, Mul};
use std::rc::Rc;

/// One elimination waiting on a stuck head.
#[derive(Clone, Debug)]
pub enum Elim {
    /// Applied to an argument.
    App(Rc<Value>),
    /// Unboxed (`let [x] = head in ..`).
    Unbox,
    /// Scrutinised by a `case` that cannot choose yet: the alternatives, and
    /// the environment they were found in.
    Case(Env, Rc<Vec<CaseAlt>>),
}

/// Eliminations of a stuck head, in order: `f a b` has spine `[App a, App b]`.
/// Spines are short, so appending at the end is fine.
pub type Spine = Vec<Elim>;

/// A type is just a value.
pub type VTy = Rc<Value>;

/// Values for the variables in scope, innermost first, so a Core de Bruijn
/// index is a direct offset into the list.  Persistent: extending one
/// environment shares its tail with every other environment built from it,
/// which is what makes closures cheap.
#[derive(Clone, Debug, Default)]
pub struct Env(Option<Rc<EnvNode>>);

#[derive(Debug)]
struct EnvNode {
    head: Rc<Value>,
    tail: Env,
}

impl Env {
    pub fn empty() -> Self {
        Env(None)
    }

    pub fn push(&self, v: Rc<Value>) -> Env {
        Env(Some(Rc::new(EnvNode { head: v, tail: self.clone() })))
    }

    pub fn get(&self, idx: usize) -> Option<Rc<Value>> {
        let mut cur = self.0.as_ref();
        for _ in 0..idx {
            cur = cur?.tail.0.as_ref();
        }
        cur.map(|n| n.head.clone())
    }
}

/// A suspended term together with the environment it was found in.  First
/// order (rather than a Rust closure) so values stay printable and so
/// pattern unification's renaming can go under binders.
#[derive(Clone, Debug)]
pub struct Closure {
    pub env: Env,
    pub body: Rc<Core>,
}

#[derive(Clone, Debug)]
pub enum Value {
    Type,
    LitTy(Builtin),
    Lit(Lit),
    /// `(x : (grade, in_type) dom) -> cod`; `cod` is a closure awaiting the
    /// argument.
    Pi { name: String, icit: Icit, grade: Rc<Value>, in_type: Rc<Value>, dom: VTy, cod: Closure },
    Lam(String, Closure),
    /// The graded modality `a [r]`: grade, then type.
    BoxTy(Rc<Value>, VTy),
    /// A promoted value `[v]`.
    BoxIntro(Rc<Value>),
    /// Stuck on an *unsolved* metavariable.  Becomes reducible the moment
    /// that meta is solved, which is why every value must be forced before
    /// its head is inspected.
    Flex(MetaVar, Spine),
    /// Stuck on a bound variable (a de Bruijn level), and therefore stuck for
    /// good.
    Rigid(Lvl, String, Spine),
    /// Stuck on a recursive definition, identified by its binder's unique.
    /// Recursive definitions never unfold (that is what keeps unification
    /// terminating), so they behave as constants.
    Opaque(usize, String, Spine),
    /// A primitive operation waiting for arguments, or saturated but stuck
    /// because some argument is not a literal (`n + 1`).  Like `Flex`, it can
    /// become reducible later — when a meta among its arguments is solved —
    /// so forcing retries the reduction.
    Prim(Prim, Vec<Rc<Value>>),
    /// A grade in normal form.  Never a lone variable: that is just the
    /// variable (see [`Poly::into_value`]).
    Grade(Poly),
    /// `lo..hi` whose ends are not both constants, so it cannot be a
    /// coefficient yet.  Opaque to grade arithmetic.
    GradeRange(Rc<Value>, Rc<Value>),
    /// The result of an ill-typed subterm.  Absorbing during unification,
    /// which stops one type error from cascading.
    Error,
}

pub fn vvar(l: Lvl, name: impl Into<String>) -> Rc<Value> {
    Rc::new(Value::Rigid(l, name.into(), Vec::new()))
}

pub fn vmeta(m: MetaVar) -> Rc<Value> {
    Rc::new(Value::Flex(m, Vec::new()))
}

// ---------------------------------------------------------------------------
// Symbolic grades
// ---------------------------------------------------------------------------

/// A grade in normal form: a polynomial over *atoms* — the stuck values of
/// type `Grade`, which is to say grade variables and unsolved metas — with
/// constant [`Grade`]s for coefficients.  `2 * r + 0..1` is the monomial `r`
/// with coefficient `2`, plus the constant monomial with coefficient `0..1`.
///
/// Grades form a commutative semiring, so every grade expression has such a
/// form, and two expressions that the semiring laws identify get the *same*
/// one: `r + r` and `2 * r`, `r * s` and `s * r`.  That is the whole of the
/// checker's reasoning about grade variables; where Granule hands its
/// constraints to an SMT solver, this compares normal forms.
///
/// Monomials are keyed by a printed form of their atoms (values have no
/// equality of their own).  Keys are computed when an atom goes in, so a
/// polynomial mentioning a meta that has since been solved is stale:
/// `Checker::force` rebuilds it.
#[derive(Clone, Debug, Default)]
pub struct Poly {
    /// monomial (its atoms' keys, sorted) to the atoms and the coefficient;
    /// no coefficient is zero
    terms: BTreeMap<Vec<String>, (Vec<Rc<Value>>, Grade)>,
}

impl Poly {
    pub fn zero() -> Poly {
        Poly::default()
    }

    pub fn constant(g: Grade) -> Poly {
        let mut p = Poly::zero();
        p.add_term(Vec::new(), Vec::new(), g);
        p
    }

    /// A stuck value, standing for itself.
    pub fn atom(v: Rc<Value>) -> Poly {
        let mut p = Poly::zero();
        p.add_term(vec![atom_key(&v)], vec![v], Grade::ONE);
        p
    }

    fn add_term(&mut self, key: Vec<String>, atoms: Vec<Rc<Value>>, g: Grade) {
        if g.is_zero() {
            return;
        }
        let slot = self.terms.entry(key).or_insert((atoms, Grade::ZERO));
        slot.1 = slot.1 + g;
    }

    pub fn is_zero(&self) -> bool {
        self.terms.is_empty()
    }

    /// The grade this is, if it mentions no atoms.
    pub fn as_constant(&self) -> Option<Grade> {
        match self.terms.len() {
            0 => Some(Grade::ZERO),
            1 => self.terms.get(&Vec::new()).map(|(_, g)| *g),
            _ => None,
        }
    }

    /// Each monomial's atoms and coefficient, constant term first.
    pub fn terms(&self) -> impl Iterator<Item = (&[Rc<Value>], Grade)> {
        self.terms.values().map(|(atoms, g)| (atoms.as_slice(), *g))
    }

    pub fn monomials(&self) -> impl Iterator<Item = &Vec<String>> {
        self.terms.keys()
    }

    /// A monomial's coefficient; zero if it does not occur.
    pub fn coefficient(&self, monomial: &[String]) -> Grade {
        self.terms.get(monomial).map_or(Grade::ZERO, |(_, g)| *g)
    }

    /// An upper bound of both, monomial by monomial (one that is missing has
    /// coefficient zero).  Not the least one there could be — that would need
    /// to know how the atoms compare — but what fits it fits by the same
    /// monomial-wise test the checker uses everywhere else.
    pub fn join(mut self, other: Poly) -> Poly {
        let mine: Vec<Vec<String>> = self.terms.keys().cloned().collect();
        for key in mine {
            if !other.terms.contains_key(&key) {
                let slot = self.terms.get_mut(&key).expect("just listed");
                slot.1 = slot.1.join(Grade::ZERO);
            }
        }
        for (key, (atoms, g)) in other.terms {
            match self.terms.get_mut(&key) {
                Some(slot) => slot.1 = slot.1.join(g),
                None => {
                    self.terms.insert(key, (atoms, g.join(Grade::ZERO)));
                }
            }
        }
        self
    }

    /// As a value.  A polynomial that is just one atom *is* that atom, so
    /// that a grade variable looks the same whether or not it has been
    /// through any arithmetic.
    pub fn into_value(self) -> Rc<Value> {
        if self.terms.len() == 1 {
            if let Some((atoms, g)) = self.terms.values().next() {
                if atoms.len() == 1 && *g == Grade::ONE {
                    return atoms[0].clone();
                }
            }
        }
        Rc::new(Value::Grade(self))
    }
}

impl Add for Poly {
    type Output = Poly;

    fn add(mut self, other: Poly) -> Poly {
        for (key, (atoms, g)) in other.terms {
            self.add_term(key, atoms, g);
        }
        self
    }
}

impl Mul for &Poly {
    type Output = Poly;

    fn mul(self, other: &Poly) -> Poly {
        let mut out = Poly::zero();
        for (k1, (a1, g1)) in &self.terms {
            for (k2, (a2, g2)) in &other.terms {
                let mut pairs: Vec<(String, Rc<Value>)> = k1
                    .iter()
                    .chain(k2)
                    .cloned()
                    .zip(a1.iter().chain(a2).cloned())
                    .collect();
                pairs.sort_by(|x, y| x.0.cmp(&y.0));
                let (key, atoms) = pairs.into_iter().unzip();
                out.add_term(key, atoms, *g1 * *g2);
            }
        }
        out
    }
}

/// A printed form of a stuck value, equal for two values only if they are
/// the same.  It is sound but incomplete — values under binders are keyed by
/// address — which is fine for what reaches it: variables and metas applied
/// to variables.
fn atom_key(v: &Value) -> String {
    let spine = |sp: &Spine| {
        sp.iter()
            .map(|e| match e {
                Elim::App(a) => format!(" ({})", atom_key(a)),
                Elim::Unbox => " unbox".to_string(),
                // (two stuck cases are the same atom only if they are the
                // same alternatives: good enough for a grade, which is the
                // only thing keyed)
                Elim::Case(_, alts) => format!(" case@{:p}", Rc::as_ptr(alts)),
            })
            .collect::<String>()
    };
    match v {
        Value::Rigid(l, _, sp) => format!("v{l}{}", spine(sp)),
        Value::Flex(m, sp) => format!("?{m}{}", spine(sp)),
        Value::Opaque(u, _, sp) => format!("c{u}{}", spine(sp)),
        Value::Type => "Type".into(),
        Value::LitTy(b) => b.name().into(),
        Value::Lit(l) => l.to_string(),
        Value::Error => "!".into(),
        Value::BoxIntro(e) => format!("[{}]", atom_key(e)),
        Value::BoxTy(g, a) => format!("{} [{}]", atom_key(a), atom_key(g)),
        Value::GradeRange(lo, hi) => format!("({}..{})", atom_key(lo), atom_key(hi)),
        Value::Grade(p) => {
            let terms: Vec<String> = p.terms.iter().map(|(k, (_, g))| format!("{g}*{}", k.join("*"))).collect();
            format!("({})", terms.join("+"))
        }
        Value::Prim(p, args) => {
            let args: Vec<String> = args.iter().map(|a| format!(" ({})", atom_key(a))).collect();
            format!("{}{}", p.name(), args.concat())
        }
        Value::Pi { .. } | Value::Lam(..) => format!("@{:p}", v as *const Value),
    }
}
