//! Leaving out what a program never uses.
//!
//! Every program carries the prelude's definitions, and a linked one every
//! package's, most of which it never calls.  A top-level definition whose
//! value is pure (a function, a literal, a constructor applied to such) and
//! that nothing live refers to can be left out without changing what the
//! program does; so can a recursive definition nothing live calls.  What is
//! live is found from the program's expression and every impure top-level
//! definition, following references: to top-level definitions by the level
//! of their `let`, and to recursive ones by their unique.  Erased subterms
//! refer to nothing, since they are never computed.
//!
//! A definition left out keeps its `let`, with a type in place of its value,
//! which every back end already treats as nothing at run time; so the levels
//! of everything else are unchanged.
//!
//! The definitions in [`TypeCheckResult::roots`] are live whatever uses
//! them: the REPL's `:ir` shows what a block of declarations compiles to
//! before anything calls them.

use super::core::{Core, Note};
use super::{RecDef, TypeCheckResult};
use idyllic_builtins::{Builtin, Prim};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// The checked program with the top-level definitions nothing live uses left
/// out, and the recursive definitions that are live; `None` if there is no
/// program, only declarations.
pub fn prune(tc: &TypeCheckResult) -> Option<(Rc<Core>, HashSet<usize>)> {
    let program = tc.program.as_ref()?;
    let is_ctor = |u: usize| tc.ctors.contains_key(&u);
    Some(prune_with(program, &tc.toplevel, &tc.rec_defs, &is_ctor, &tc.roots))
}

/// `program` with the top-level definitions nothing live uses left out, and
/// the recursive definitions that are live.  `toplevel` is the unique of each
/// of the program's outer `let`s that is a top-level definition; `is_ctor`
/// says which constants are constructors; what is in `roots` is live.
fn prune_with(
    program: &Rc<Core>,
    toplevel: &[usize],
    rec_defs: &[RecDef],
    is_ctor: &dyn Fn(usize) -> bool,
    roots: &HashSet<usize>,
) -> (Rc<Core>, HashSet<usize>) {
    let toplevel_uniques = toplevel;
    let toplevel = toplevel.len();
    // the program starts in a context of the builtins and the primitives
    let base = Builtin::COUNT + Prim::ALL.len();
    let mut lets: Vec<(String, Rc<Core>)> = Vec::new();
    let mut body = program.clone();
    while let (Core::Let(x, rhs, rest), true) = (&*body, lets.len() < toplevel) {
        lets.push((x.clone(), rhs.clone()));
        body = rest.clone();
    }
    let recs: HashMap<usize, &RecDef> = rec_defs.iter().map(|d| (d.unique, d)).collect();
    let mut live_lets = vec![false; lets.len()];
    let mut live_recs: HashSet<usize> = HashSet::new();
    let mut work: Vec<(usize, Rc<Core>)> = vec![(base + lets.len(), body.clone())];
    for (i, (_, rhs)) in lets.iter().enumerate() {
        if !rhs.is_pure_value(is_ctor) || roots.contains(&toplevel_uniques[i]) {
            live_lets[i] = true;
            work.push((base + i, rhs.clone()));
        }
    }
    for d in rec_defs.iter().filter(|d| roots.contains(&d.unique)) {
        if live_recs.insert(d.unique) {
            work.push((d.lvl, d.core.clone()));
        }
    }
    while let Some((at, c)) = work.pop() {
        let mut levels = HashSet::new();
        let mut opaques = HashSet::new();
        references(&c, at, 0, &mut levels, &mut opaques);
        for l in levels {
            if let Some(i) = l.checked_sub(base).filter(|i| *i < lets.len()) {
                if !live_lets[i] {
                    live_lets[i] = true;
                    work.push((base + i, lets[i].1.clone()));
                }
            }
        }
        for u in opaques {
            if let Some(d) = recs.get(&u) {
                if live_recs.insert(u) {
                    work.push((d.lvl, d.core.clone()));
                }
            }
        }
    }
    let pruned = lets.into_iter().zip(live_lets).rev().fold(body, |acc, ((x, rhs), live)| {
        let rhs = if live { rhs } else { Rc::new(Core::Type(0)) };
        Rc::new(Core::Let(x, rhs, acc))
    });
    (pruned, live_recs)
}

/// The levels (in a context of `at` variables, `depth` binders in) and the
/// constants a term refers to, outside its erased parts.
fn references(c: &Core, at: usize, depth: usize, levels: &mut HashSet<usize>, opaques: &mut HashSet<usize>) {
    let mut go = |c: &Core, d: usize| references(c, at, d, levels, opaques);
    match c {
        Core::Var(i, _) => {
            if *i >= depth {
                if let Some(l) = at.checked_sub(i - depth + 1) {
                    levels.insert(l);
                }
            }
        }
        Core::Opaque(u, _) => {
            opaques.insert(*u);
        }
        Core::Runtime(Note::Erased, _) => {}
        Core::Lam(_, b) => go(b, depth + 1),
        Core::Let(_, e, b) => {
            go(e, depth);
            go(b, depth + 1);
        }
        Core::App(a, b) => {
            go(a, depth);
            go(b, depth);
        }
        Core::BoxIntro(a) | Core::Unbox(a) | Core::Runtime(_, a) => go(a, depth),
        Core::Case(s, alts) => {
            go(s, depth);
            for alt in alts.iter() {
                go(&alt.body, depth + alt.vars.len());
            }
        }
        Core::Handle(b, r, ops) => {
            go(b, depth);
            go(r, depth);
            for op in ops.iter() {
                go(&op.body, depth);
            }
        }
        // types, grades and rows are erased
        _ => {}
    }
}
