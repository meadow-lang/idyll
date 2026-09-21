//! Grades: how many times a variable may be used.
//!
//! This is the coeffect algebra behind the graded modality, after Granule.
//! Grades form a *pre-ordered semiring*:
//!
//! * `0` and `+` combine the uses of a variable in two subterms
//!   (`f x x` uses `x` `1 + 1` times);
//! * `1` and `·` scale uses through a binder (`g y` where `g` wants its
//!   argument `2` times uses `y` `2 · 1` times);
//! * `⊑` says when an actual usage is acceptable where a grade was promised.
//!
//! The semiring is Granule's `Interval (Ext Nat)`: a grade is a range
//! `lo..hi` of use counts, each end a natural number or `∞`.  A bare `n` is
//! the point interval `n..n`, an *exact* count, so Granule's `Nat` grades
//! embed here unchanged (`0` erased, `1` linear).  Proper intervals give the
//! rest: `0..1` is affine ("at most once"), `1..∞` is relevant ("at least
//! once"), and `0..∞` is unrestricted — linear logic's `!`.
//!
//! Arithmetic is interval arithmetic, and the order is containment:
//! `a..b ⊑ c..d` iff `c ≤ a` and `b ≤ d`.  On point intervals that is
//! equality, which is exactly why a bare `2` still means "exactly twice".
//!
//! This module is the *concrete* algebra.  Grades in a program are terms
//! (they can be variables, for grade polymorphism), and the checker normalises
//! them to polynomials whose coefficients are the grades defined here; see
//! `typecheck::value::Poly`.

use std::fmt;
use std::ops::{Add, Mul};

/// A natural number or infinity (Granule's `Ext Nat`).  The derived order
/// puts every finite count below `Inf`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ext {
    Fin(u64),
    Inf,
}

impl Add for Ext {
    type Output = Ext;

    fn add(self, other: Ext) -> Ext {
        match (self, other) {
            (Ext::Fin(a), Ext::Fin(b)) => a.checked_add(b).map_or(Ext::Inf, Ext::Fin),
            _ => Ext::Inf,
        }
    }
}

/// `0 · ∞ = 0`: an erased position erases everything in it, which is what
/// makes type arguments free.
impl Mul for Ext {
    type Output = Ext;

    fn mul(self, other: Ext) -> Ext {
        match (self, other) {
            (Ext::Fin(0), _) | (_, Ext::Fin(0)) => Ext::Fin(0),
            (Ext::Fin(a), Ext::Fin(b)) => a.checked_mul(b).map_or(Ext::Inf, Ext::Fin),
            _ => Ext::Inf,
        }
    }
}

impl fmt::Display for Ext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ext::Fin(n) => write!(f, "{n}"),
            Ext::Inf => write!(f, "∞"),
        }
    }
}

/// An interval of use counts, `lo..hi` with `lo ≤ hi`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Grade {
    lo: Ext,
    hi: Ext,
}

impl Grade {
    pub const ZERO: Grade = Grade::exact(0);
    pub const ONE: Grade = Grade::exact(1);
    pub const INF: Grade = Grade { lo: Ext::Inf, hi: Ext::Inf };
    /// Unrestricted use, `0..∞`; Granule also writes it as an empty `[]`.
    pub const ANY: Grade = Grade { lo: Ext::Fin(0), hi: Ext::Inf };

    /// What an omitted grade means on a binder.  As in Granule the base is
    /// *linear*: `a -> b` uses its argument exactly once, and anything else
    /// is said explicitly.
    pub const DEFAULT: Grade = Grade::ONE;
    /// What an omitted *type-level* grade means: how often the rest of a
    /// function type may mention its binder.  Unrestricted, so dependency is
    /// only policed where it is asked for.
    pub const DEFAULT_IN_TYPE: Grade = Grade::ANY;

    /// Exactly `n` uses.
    pub const fn exact(n: u64) -> Grade {
        Grade { lo: Ext::Fin(n), hi: Ext::Fin(n) }
    }

    /// `lo..hi`, from the lower end of one grade to the upper end of another;
    /// `None` if that would be empty.
    pub fn range(lo: Grade, hi: Grade) -> Option<Grade> {
        (lo.lo <= hi.hi).then_some(Grade { lo: lo.lo, hi: hi.hi })
    }

    pub fn is_zero(self) -> bool {
        self == Grade::ZERO
    }

    /// The least grade that both fit: how often something is used by a
    /// `case` whose alternatives use it `self` and `other` times.
    pub fn join(self, other: Grade) -> Grade {
        Grade { lo: self.lo.min(other.lo), hi: self.hi.max(other.hi) }
    }

    /// The most uses allowed.
    pub fn upper(self) -> Ext {
        self.hi
    }

    pub fn is_exact(self) -> bool {
        self.lo == self.hi
    }

    /// `self ⊑ allowed`: may a variable of grade `allowed` be used `self`
    /// times?  Every count the usage might be has to be one the grade allows.
    pub fn fits(self, allowed: Grade) -> bool {
        allowed.lo <= self.lo && self.hi <= allowed.hi
    }
}

/// Uses in two subterms.
impl Add for Grade {
    type Output = Grade;

    fn add(self, other: Grade) -> Grade {
        Grade { lo: self.lo + other.lo, hi: self.hi + other.hi }
    }
}

/// Uses scaled through a binder.
impl Mul for Grade {
    type Output = Grade;

    fn mul(self, other: Grade) -> Grade {
        Grade { lo: self.lo * other.lo, hi: self.hi * other.hi }
    }
}

impl fmt::Display for Grade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_exact() {
            write!(f, "{}", self.lo)
        } else {
            write!(f, "{}..{}", self.lo, self.hi)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Ext, Grade};

    fn samples() -> Vec<Grade> {
        let ends = [Ext::Fin(0), Ext::Fin(1), Ext::Fin(2), Ext::Fin(7), Ext::Inf];
        let mut out = Vec::new();
        for lo in ends {
            for hi in ends {
                if lo <= hi {
                    out.push(Grade { lo, hi });
                }
            }
        }
        out
    }

    #[test]
    fn semiring_laws() {
        let s = samples();
        for &a in &s {
            assert_eq!(a + Grade::ZERO, a);
            assert_eq!(a * Grade::ONE, a);
            assert_eq!(a * Grade::ZERO, Grade::ZERO);
            assert_eq!(Grade::ZERO * a, Grade::ZERO);
            for &b in &s {
                assert_eq!(a + b, b + a);
                assert_eq!(a * b, b * a);
                for &c in &s {
                    assert_eq!((a + b) + c, a + (b + c));
                    assert_eq!((a * b) * c, a * (b * c));
                    assert_eq!(a * (b + c), a * b + a * c);
                }
            }
        }
    }

    #[test]
    fn operations_are_monotone() {
        // what makes checking a symbolic grade term by term sound
        let s = samples();
        for &a in &s {
            for &b in &s {
                if !a.fits(b) {
                    continue;
                }
                for &c in &s {
                    assert!((a + c).fits(b + c));
                    assert!((a * c).fits(b * c));
                }
            }
        }
    }

    #[test]
    fn exact_grades_are_ordered_discretely() {
        assert!(Grade::exact(1).fits(Grade::exact(1)));
        assert!(!Grade::exact(1).fits(Grade::exact(2)));
        assert!(!Grade::exact(0).fits(Grade::exact(1)));
    }

    #[test]
    fn intervals_are_ordered_by_containment() {
        let affine = Grade::range(Grade::ZERO, Grade::ONE).unwrap();
        let relevant = Grade::range(Grade::ONE, Grade::INF).unwrap();
        assert!(Grade::ZERO.fits(affine) && Grade::ONE.fits(affine));
        assert!(!Grade::exact(2).fits(affine));
        assert!(!Grade::ZERO.fits(relevant) && Grade::exact(9).fits(relevant));
        assert!(!affine.fits(Grade::ONE));
        for a in samples() {
            assert!(a.fits(Grade::ANY));
        }
        assert_eq!(Grade::range(Grade::exact(2), Grade::ONE), None);
    }

    #[test]
    fn overflow_saturates_to_infinity() {
        assert_eq!(Grade::exact(u64::MAX) + Grade::ONE, Grade::INF);
        assert_eq!(Grade::exact(u64::MAX) * Grade::exact(2), Grade::INF);
    }

    #[test]
    fn display() {
        assert_eq!(Grade::exact(3).to_string(), "3");
        assert_eq!(Grade::ANY.to_string(), "0..∞");
        assert_eq!(Grade::INF.to_string(), "∞");
    }
}
