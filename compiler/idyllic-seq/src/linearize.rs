//! Inferring explicit substitutions (the paper's §2.3.1).
//!
//! [`super::lower`] uses variables the way a lambda calculus does: as often
//! as it likes, from anywhere in scope.  AxCut's environment is an ordered
//! list of which `let` and `new` consume the tail, `switch` and `invoke` the
//! last entry, and which `invoke` and `jump` must leave *exactly* as the
//! receiving clause or label expects it.  This pass puts a `substitute` in
//! front of each such statement to arrange that:
//!
//! * what the statement consumes is moved to the end, in order;
//! * what is still needed afterwards is kept, in its current order;
//! * something both consumed *and* needed afterwards (or consumed twice) is
//!   duplicated, under a fresh name — contraction;
//! * everything else is dropped — weakening.
//!
//! So variables are dropped as early and copied as late as possible.  A
//! substitution that would change nothing is left out.  `extern`s consume
//! nothing and need no arranging.
//!
//! This is also where a closure's environment is decided: `new` captures the
//! free variables of its clauses, which the lowering left unsaid.
//!
//! This pass works from the code alone and knows nothing of grades.  That
//! is deliberate: the type checker has *already* promised how often each
//! variable is used, and [`super::grades`] checks the two against each
//! other — a variable is never copied here more often than its grade allows.
//! At run time each copy is a reference count going up and each drop one
//! going down, and nothing else ever touches a count ([`super::machine`]).

use super::{free_vars, Branch, Clauses, Definition, Program, Stmt, Var};
use std::collections::HashMap;

pub fn linearize(p: &Program) -> Program {
    let definitions = p
        .definitions
        .iter()
        .map(|d| Definition {
            label: d.label.clone(),
            params: d.params.clone(),
            body: stmt(&d.body, &d.params, &Renaming::new()),
        })
        .collect();
    Program { definitions, grades: p.grades.clone(), continuations: p.continuations.clone() }
}

/// What a variable of the input is called now, where that differs: the copy
/// a closure captured goes by a new name inside the closure.
type Renaming = HashMap<Var, Var>;

fn current(renaming: &Renaming, v: &Var) -> Var {
    renaming.get(v).unwrap_or(v).clone()
}

/// An environment rearranged for a statement: what is kept, then what the
/// statement consumes.
struct Arranged {
    pairs: Vec<(Var, Var)>,
    kept: Vec<Var>,
    consumed: Vec<Var>,
}

impl Arranged {
    /// Put the substitution in front of `s`, unless it is the identity.
    fn before(self, env: &[Var], s: Stmt) -> Stmt {
        let unchanged = self.pairs.len() == env.len() && self.pairs.iter().zip(env).all(|((new, old), v)| new == old && old == v);
        if unchanged {
            s
        } else {
            Stmt::Substitute(self.pairs, Box::new(s))
        }
    }
}

/// Rearrange `env` to end with `consumed`, keeping only what is `live`
/// afterwards.
fn arrange(env: &[Var], live: &[Var], consumed: &[Var]) -> Arranged {
    let kept: Vec<Var> = env.iter().filter(|v| live.contains(v)).cloned().collect();
    let mut tail: Vec<Var> = Vec::new();
    for v in consumed {
        // consuming it must not take it from whoever else needs it
        let mut name = v.clone();
        while kept.contains(&name) || tail.contains(&name) {
            name.push('\'');
        }
        tail.push(name);
    }
    let pairs = kept.iter().map(|v| (v.clone(), v.clone())).chain(tail.iter().cloned().zip(consumed.iter().cloned())).collect();
    Arranged { pairs, kept, consumed: tail }
}

/// The variables (by their current names) that `clauses` need besides their
/// own parameters, in the order `env` has them.
fn needed_by(clauses: &[&Branch], env: &[Var], renaming: &Renaming) -> Vec<Var> {
    let mut needed: Vec<Var> = Vec::new();
    for b in clauses {
        for source in free_vars(&b.body) {
            let v = current(renaming, &source);
            if !b.params.contains(&source) && !needed.contains(&v) {
                needed.push(v);
            }
        }
    }
    needed.sort_by_key(|v| env.iter().position(|e| e == v));
    needed
}

fn live_after(rest: &Stmt, bound: &Var, renaming: &Renaming) -> Vec<Var> {
    free_vars(rest).iter().filter(|v| *v != bound).map(|v| current(renaming, v)).collect()
}

fn extended(env: &[Var], more: &[Var]) -> Vec<Var> {
    env.iter().chain(more).cloned().collect()
}

fn stmt(s: &Stmt, env: &[Var], renaming: &Renaming) -> Stmt {
    let now = |vs: &[Var]| -> Vec<Var> { vs.iter().map(|v| current(renaming, v)).collect() };
    match s {
        // the input has none; one that is there is taken at its word
        Stmt::Substitute(pairs, rest) => {
            let pairs: Vec<(Var, Var)> = pairs.iter().map(|(new, old)| (new.clone(), current(renaming, old))).collect();
            let env: Vec<Var> = pairs.iter().map(|(new, _)| new.clone()).collect();
            Stmt::Substitute(pairs, Box::new(stmt(rest, &env, &Renaming::new())))
        }
        Stmt::Let(v, m, args, rest) => {
            let a = arrange(env, &live_after(rest, v, renaming), &now(args));
            let rest = stmt(rest, &extended(&a.kept, std::slice::from_ref(v)), renaming);
            let fields = a.consumed.clone();
            a.before(env, Stmt::Let(v.clone(), m.clone(), fields, Box::new(rest)))
        }
        Stmt::New(v, _, clauses, rest) => {
            let branches: Vec<&Branch> = clauses.iter().map(|(_, b)| b).collect();
            let captured = needed_by(&branches, env, renaming);
            let a = arrange(env, &live_after(rest, v, renaming), &captured);
            // Inside the closure the captured variables go by the names they
            // were captured under, and nothing else is in scope: a clause
            // runs with its parameters followed by the closure environment.
            let mut inside = Renaming::new();
            for b in &branches {
                for source in free_vars(&b.body) {
                    let at = captured.iter().position(|c| *c == current(renaming, &source));
                    if let (false, Some(at)) = (b.params.contains(&source), at) {
                        inside.insert(source, a.consumed[at].clone());
                    }
                }
            }
            let clauses: Clauses = clauses
                .iter()
                .map(|(m, b)| {
                    let body = stmt(&b.body, &extended(&b.params, &a.consumed), &inside);
                    (m.clone(), Branch { params: b.params.clone(), body })
                })
                .collect();
            let rest = stmt(rest, &extended(&a.kept, std::slice::from_ref(v)), renaming);
            let closure = a.consumed.clone();
            a.before(env, Stmt::New(v.clone(), closure, clauses, Box::new(rest)))
        }
        Stmt::Switch(v, clauses) => {
            let branches: Vec<&Branch> = clauses.iter().map(|(_, b)| b).collect();
            let a = arrange(env, &needed_by(&branches, env, renaming), &now(std::slice::from_ref(v)));
            // every clause gets the rest of the environment, then the fields
            let clauses: Clauses = clauses
                .iter()
                .map(|(m, b)| {
                    let body = stmt(&b.body, &extended(&a.kept, &b.params), renaming);
                    (m.clone(), Branch { params: b.params.clone(), body })
                })
                .collect();
            let scrutinee = a.consumed[0].clone();
            a.before(env, Stmt::Switch(scrutinee, clauses))
        }
        Stmt::Invoke(v, m, args) => {
            let mut consumed = now(args);
            consumed.push(current(renaming, v));
            let mut a = arrange(env, &[], &consumed);
            let receiver = a.consumed.pop().expect("the receiver was pushed last");
            let args = a.consumed.clone();
            a.consumed.push(receiver.clone());
            a.before(env, Stmt::Invoke(receiver, m.clone(), args))
        }
        Stmt::Jump(l, args) => {
            let a = arrange(env, &[], &now(args));
            let args = a.consumed.clone();
            a.before(env, Stmt::Jump(l.clone(), args))
        }
        // like a `let` of two fields
        Stmt::Handle(h, k, ret, rest) => {
            let a = arrange(env, &live_after(rest, ret, renaming), &now(&[h.clone(), k.clone()]));
            let rest = stmt(rest, &extended(&a.kept, std::slice::from_ref(ret)), renaming);
            let (h, k) = (a.consumed[0].clone(), a.consumed[1].clone());
            a.before(env, Stmt::Handle(h, k, ret.clone(), Box::new(rest)))
        }
        // like a jump
        Stmt::Perform(op, xs, k) => {
            let consumed: Vec<Var> = xs.iter().chain([k]).cloned().collect();
            let a = arrange(env, &[], &now(&consumed));
            let mut consumed = a.consumed.clone();
            let k = consumed.pop().expect("the continuation");
            a.before(env, Stmt::Perform(op.clone(), consumed, k))
        }
        Stmt::Extern(op, args, branches) => {
            let branches = branches
                .iter()
                .map(|b| Branch { params: b.params.clone(), body: stmt(&b.body, &extended(env, &b.params), renaming) })
                .collect();
            Stmt::Extern(op.clone(), now(args), branches)
        }
    }
}
