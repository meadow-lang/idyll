//! Holding the lowered code to the grades.
//!
//! The type checker counted how often every variable is used, and erasure
//! kept its promise about what that means at run time: an argument to a
//! grade-`0` parameter is not computed, a grade-`0` box is not filled.  So
//! what is left should use each variable no more often than its grade allows
//! — and since `substitute` makes a copy for every use after the first, that
//! is exactly the claim that **a variable is never copied more often than
//! its grade says**.  A linear variable is never copied at all.
//!
//! This checks it, on the output of [`super::lower`], where uses are still
//! plain to see.  Two passes that never spoke to each other — the usage
//! analysis in the type checker, and the translation here — have to agree,
//! and a disagreement is a bug in one of them.
//!
//! Why only an upper bound.  A use scaled by `r` in the types is *at least*
//! one use at run time whenever `r` is not `0`, so uses at run time never
//! exceed the grade's upper end.  But they can fall short of its lower end:
//! `f x` with `f : (y : [2] a) -> b` uses `x` twice as far as the types are
//! concerned and mentions it once (the other use is inside `f`), and a use
//! inside a type that is computed — `\(a : [2] Type) -> a -> a` — is a use to
//! the types and gone at run time.  So "never dropped" does not follow, and
//! is not checked.
//!
//! What this does *not* say is that a linear variable's **value** is
//! unshared.  A caller holding a box at grade `2` may pass it twice to a
//! function that takes it linearly, and inside, the linear parameter points
//! at a block with another reference to it.  That is why the machine's
//! uniqueness check at `switch` and `invoke` is dynamic, as it is in the
//! AxCut paper, and why grades can take reference counts off *variables* but
//! not off *blocks*.

use super::{free_vars, Program, Stmt, Var};
use idyllic_typecheck::grade::{Ext, Grade};
use std::fmt;

/// A variable the lowered program uses more often than its grade allows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub var: Var,
    pub grade: Grade,
    pub uses: u64,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} has grade {}, but the lowered program uses it {} times", self.var, self.grade, self.uses)
    }
}

pub fn check(program: &Program) -> Vec<Violation> {
    let mut checker = Checker { program, violations: Vec::new() };
    for d in &program.definitions {
        checker.bound(&d.params, &d.body);
    }
    checker.violations
}

struct Checker<'p> {
    program: &'p Program,
    violations: Vec<Violation>,
}

impl Checker<'_> {
    /// `vars` come into scope over `body`: judge each, then look inside.
    fn bound(&mut self, vars: &[Var], body: &Stmt) {
        for v in vars {
            let Some(&grade) = self.program.grades.get(v) else { continue };
            let uses = uses(body, v);
            let allowed = match grade.upper() {
                Ext::Fin(n) => uses <= n,
                Ext::Inf => true,
            };
            if !allowed {
                self.violations.push(Violation { var: v.clone(), grade, uses });
            }
        }
        self.inside(body);
    }

    fn inside(&mut self, s: &Stmt) {
        match s {
            Stmt::Substitute(_, rest) => self.inside(rest),
            Stmt::Let(v, _, _, rest) => self.bound(std::slice::from_ref(v), rest),
            Stmt::New(v, _, clauses, rest) => {
                for (_, b) in clauses {
                    // a closure's body is a scope of its own, for its
                    // parameters and for what it captured
                    let mut scope = b.params.clone();
                    scope.extend(free_vars(&b.body).into_iter().filter(|x| !b.params.contains(x)));
                    self.bound(&scope, &b.body);
                }
                self.bound(std::slice::from_ref(v), rest);
            }
            Stmt::Switch(_, clauses) => clauses.iter().for_each(|(_, b)| self.bound(&b.params, &b.body)),
            Stmt::Extern(_, _, branches) => branches.iter().for_each(|b| self.bound(&b.params, &b.body)),
            Stmt::Invoke(..) | Stmt::Jump(..) => {}
        }
    }
}

/// The most times `s` can use `v` on any one path through it.  Being
/// captured by a closure is one use, whatever the closure does with it.
fn uses(s: &Stmt, v: &Var) -> u64 {
    let among = |vars: &[Var]| vars.iter().filter(|x| *x == v).count() as u64;
    let is = |x: &Var| u64::from(x == v);
    match s {
        Stmt::Substitute(pairs, _) => pairs.iter().filter(|(_, old)| old == v).count() as u64,
        Stmt::Let(_, _, args, rest) => among(args) + uses(rest, v),
        Stmt::New(_, _, clauses, rest) => {
            let captured = clauses.iter().any(|(_, b)| !b.params.contains(v) && free_vars(&b.body).contains(v));
            u64::from(captured) + uses(rest, v)
        }
        Stmt::Switch(x, clauses) => is(x) + clauses.iter().map(|(_, b)| uses(&b.body, v)).max().unwrap_or(0),
        Stmt::Invoke(x, _, args) => is(x) + among(args),
        Stmt::Jump(_, args) => among(args),
        Stmt::Extern(_, args, branches) => among(args) + branches.iter().map(|b| uses(&b.body, v)).max().unwrap_or(0),
    }
}
