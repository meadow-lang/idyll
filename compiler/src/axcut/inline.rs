//! Inlining, for the native compiler.
//!
//! Lowering is faithful anlet graph: HashMap<Label, Vec<Label>> = p.definitions.iter().map(|d| (d.label.clone(), jumps_of naive: every function is a closure, every call
//! an `invoke`, every reference to a definition a jump to the label that
//! builds its value.  This pass does the three reductions that undo most of
//! that, on the convenient form of AxCut (before [`super::linearize`], while
//! variables are still used freely and named apart):
//!
//! ```text
//! new v = { m(Γ) ⇒ s }; .. invoke v m(Δ) ..   ⟶   .. s[Γ := Δ] ..
//! let v = m(Δ); .. switch v { m(Γ) ⇒ s } ..   ⟶   .. s[Γ := Δ] ..
//! jump l(Δ)   where define l : (Γ) = s         ⟶   s[Γ := Δ]
//! ```
//!
//! The first two are the cuts of the sequent calculus, done at compile time.
//! They fire when `v` is used *once*, so that code only moves and nothing is
//! duplicated; a `new` or `let` that is not used at all is dropped.  A
//! function used more often is copied into its calls (what it binds being
//! renamed) if it is small.  The third copies too, so it is for labels that
//! are not recursive and either small or jumped to from one place.
//!
//! Between them they turn `let f = \x -> .. in f 1` into the body of `f`,
//! and a call through a definition's value (`f_value`, a closure that
//! collects arguments) into a jump to the definition.  What is left — calls
//! to recursive functions — become direct calls in [`super::llvm`], which
//! LLVM can inline where it sees fit.
//!
//! The grades are not kept up: substituting an argument for a parameter lets
//! it be used as often as the parameter was, which its own grade never
//! promised.  [`super::grades`] has to look at the program before this does.

use super::{Branch, Clauses, Definition, Label, Program, Stmt, Symbol, Var};
use std::collections::{HashMap, HashSet};

/// A label with a body bigger than this is only inlined if one jump goes
/// there.
const SMALL: usize = 16;

pub fn inline(p: &Program) -> Program {
    let mut inliner = Inliner { continuations: p.continuations.clone(), next: 0 };
    let graph: HashMap<Label, Vec<Label>> = p.definitions.iter().map(|d| (d.label.clone(), jumps_of(&d.body))).collect();
    let mut sites: HashMap<Label, usize> = HashMap::new();
    for l in graph.values().flatten() {
        *sites.entry(l.clone()).or_default() += 1;
    }

    // callees before callers, so that what is inlined has been dealt with
    let mut order: Vec<Label> = Vec::new();
    fn visit(l: &Label, graph: &HashMap<Label, Vec<Label>>, order: &mut Vec<Label>, seen: &mut HashSet<Label>) {
        if graph.contains_key(l) && seen.insert(l.clone()) {
            graph[l].iter().for_each(|c| visit(c, graph, order, seen));
            order.push(l.clone());
        }
    }
    let mut seen = HashSet::new();
    p.definitions.iter().for_each(|d| visit(&d.label, &graph, &mut order, &mut seen));

    let main = p.definitions.first().map(|d| d.label.clone());
    let mut done: HashMap<Label, Definition> = HashMap::new();
    let mut inlinable: HashSet<Label> = HashSet::new();
    for label in &order {
        let d = p.definition(label).expect("the graph has only the program's labels");
        let body = inliner.jumps(d.body.clone(), &done, &inlinable);
        let body = inliner.shrink(body);
        let recursive = reaches(&graph, label, label);
        let wanted = size(&body) <= SMALL || sites.get(label) == Some(&1);
        if !recursive && wanted && Some(label) != main.as_ref() {
            inlinable.insert(label.clone());
        }
        done.insert(label.clone(), Definition { label: label.clone(), params: d.params.clone(), body });
    }

    // what nothing jumps to any more is gone
    let mut kept: Vec<Label> = Vec::new();
    let mut queue: Vec<Label> = main.into_iter().collect();
    while let Some(l) = queue.pop() {
        if !kept.contains(&l) {
            queue.extend(done.get(&l).map(|d| jumps_of(&d.body)).unwrap_or_default());
            kept.push(l);
        }
    }
    let definitions = p.definitions.iter().filter(|d| kept.contains(&d.label)).filter_map(|d| done.remove(&d.label)).collect();
    Program { definitions, grades: HashMap::new(), continuations: inliner.continuations }
}

/// Whether `from` jumps, in one step or several, to `to`.
fn reaches(graph: &HashMap<Label, Vec<Label>>, from: &Label, to: &Label) -> bool {
    let mut seen: HashSet<&Label> = HashSet::new();
    let mut queue: Vec<&Label> = graph.get(from).into_iter().flatten().collect();
    while let Some(l) = queue.pop() {
        if l == to {
            return true;
        }
        if seen.insert(l) {
            queue.extend(graph.get(l).into_iter().flatten());
        }
    }
    false
}

/// The children of a statement: what follows it, and its clauses' bodies.
fn children(s: &Stmt) -> Vec<&Stmt> {
    match s {
        Stmt::Substitute(_, rest) | Stmt::Let(_, _, _, rest) => vec![rest],
        Stmt::New(_, _, clauses, rest) => clauses.iter().map(|(_, b)| &b.body).chain([&**rest]).collect(),
        Stmt::Switch(_, clauses) => clauses.iter().map(|(_, b)| &b.body).collect(),
        Stmt::Extern(_, _, branches) => branches.iter().map(|b| &b.body).collect(),
        Stmt::Invoke(..) | Stmt::Jump(..) => Vec::new(),
    }
}

fn size(s: &Stmt) -> usize {
    1 + children(s).into_iter().map(size).sum::<usize>()
}

/// Every jump in `s`, one entry for each.
fn jumps_of(s: &Stmt) -> Vec<Label> {
    let mut out: Vec<Label> = children(s).into_iter().flat_map(jumps_of).collect();
    if let Stmt::Jump(l, _) = s {
        out.push(l.clone());
    }
    out
}

/// How often `v` is used in `s`.
fn uses(s: &Stmt, v: &Var) -> usize {
    let among = |vs: &[Var]| vs.iter().filter(|x| *x == v).count();
    let here = match s {
        Stmt::Substitute(pairs, _) => pairs.iter().filter(|(_, old)| old == v).count(),
        Stmt::Let(_, _, args, _) | Stmt::Jump(_, args) | Stmt::Extern(_, args, _) => among(args),
        Stmt::New(_, captured, _, _) => among(captured),
        Stmt::Switch(x, _) => usize::from(x == v),
        Stmt::Invoke(x, _, args) => usize::from(x == v) + among(args),
    };
    here + children(s).into_iter().map(|c| uses(c, v)).sum::<usize>()
}

/// Rebuild `s` from the leaves up, giving `f` each statement once its
/// children are done.  What `f` returns is not looked into again.
fn transform(s: Stmt, f: &mut dyn FnMut(Stmt) -> Stmt) -> Stmt {
    let branch = |b: Branch, f: &mut dyn FnMut(Stmt) -> Stmt| Branch { params: b.params, body: transform(b.body, f) };
    let clauses = |cs: Clauses, f: &mut dyn FnMut(Stmt) -> Stmt| cs.into_iter().map(|(m, b)| (m, branch(b, f))).collect();
    let s = match s {
        Stmt::Substitute(pairs, rest) => Stmt::Substitute(pairs, Box::new(transform(*rest, f))),
        Stmt::Let(v, m, args, rest) => Stmt::Let(v, m, args, Box::new(transform(*rest, f))),
        Stmt::New(v, captured, cs, rest) => {
            let cs = clauses(cs, f);
            Stmt::New(v, captured, cs, Box::new(transform(*rest, f)))
        }
        Stmt::Switch(v, cs) => Stmt::Switch(v, clauses(cs, f)),
        Stmt::Extern(op, args, branches) => Stmt::Extern(op, args, branches.into_iter().map(|b| branch(b, f)).collect()),
        leaf @ (Stmt::Invoke(..) | Stmt::Jump(..)) => leaf,
    };
    f(s)
}

/// `s` with every variable in `map` called what `map` says, binders too.
fn rename(s: Stmt, map: &HashMap<Var, Var>) -> Stmt {
    let var = |v: Var| map.get(&v).cloned().unwrap_or(v);
    let vars = |vs: Vec<Var>| vs.into_iter().map(var).collect::<Vec<Var>>();
    let params = |b: Branch| Branch { params: vars(b.params), body: b.body };
    // the children are renamed by `transform`; this is one statement's own
    transform(s, &mut |s| match s {
        Stmt::Substitute(pairs, rest) => Stmt::Substitute(pairs.into_iter().map(|(new, old)| (var(new), var(old))).collect(), rest),
        Stmt::Let(v, m, args, rest) => Stmt::Let(var(v), m, vars(args), rest),
        Stmt::New(v, captured, cs, rest) => {
            Stmt::New(var(v), vars(captured), cs.into_iter().map(|(m, b)| (m, params(b))).collect(), rest)
        }
        Stmt::Switch(v, cs) => Stmt::Switch(var(v), cs.into_iter().map(|(m, b)| (m, params(b))).collect()),
        Stmt::Invoke(v, m, args) => Stmt::Invoke(var(v), m, vars(args)),
        Stmt::Jump(l, args) => Stmt::Jump(l, vars(args)),
        Stmt::Extern(op, args, branches) => Stmt::Extern(op, vars(args), branches.into_iter().map(params).collect()),
    })
}

/// A clause's body, with its parameters standing for `args`.
fn instantiate(b: Branch, args: Vec<Var>) -> Stmt {
    rename(b.body, &b.params.into_iter().zip(args).collect())
}

/// The variables `s` binds.
fn binders(s: &Stmt, out: &mut Vec<Var>) {
    match s {
        Stmt::Substitute(pairs, _) => out.extend(pairs.iter().map(|(new, _)| new.clone())),
        Stmt::Let(v, ..) => out.push(v.clone()),
        Stmt::New(v, _, clauses, _) => {
            out.push(v.clone());
            clauses.iter().for_each(|(_, b)| out.extend(b.params.iter().cloned()));
        }
        Stmt::Switch(_, clauses) => clauses.iter().for_each(|(_, b)| out.extend(b.params.iter().cloned())),
        Stmt::Extern(_, _, branches) => branches.iter().for_each(|b| out.extend(b.params.iter().cloned())),
        Stmt::Invoke(..) | Stmt::Jump(..) => {}
    }
    children(s).into_iter().for_each(|c| binders(c, out));
}

struct Inliner {
    continuations: HashSet<Var>,
    next: usize,
}

impl Inliner {
    /// A copy of a body for one use of it: its parameters are the use's
    /// arguments, and what it binds gets names of its own.
    fn copy(&mut self, params: &[Var], body: &Stmt, args: Vec<Var>) -> Stmt {
        let mut map: HashMap<Var, Var> = params.iter().cloned().zip(args).collect();
        let mut bound = Vec::new();
        binders(body, &mut bound);
        for v in bound {
            self.next += 1;
            let base = v.split('.').next().unwrap_or(&v);
            let copy = format!("{base}.{}", self.next);
            if self.continuations.contains(&v) {
                self.continuations.insert(copy.clone());
            }
            map.insert(v, copy);
        }
        rename(body.clone(), &map)
    }

    /// Replace the jumps to `inlinable` labels by their bodies.
    fn jumps(&mut self, s: Stmt, done: &HashMap<Label, Definition>, inlinable: &HashSet<Label>) -> Stmt {
        transform(s, &mut |s| match s {
            Stmt::Jump(l, args) if inlinable.contains(&l) => self.copy(&done[&l].params, &done[&l].body, args),
            other => other,
        })
    }

    /// Do the cuts that can be done now, until there are none.
    fn shrink(&mut self, mut s: Stmt) -> Stmt {
        loop {
            let mut changed = false;
            s = transform(s, &mut |s| match s {
                Stmt::New(v, captured, clauses, rest) => match uses(&rest, &v) {
                    0 => {
                        changed = true;
                        *rest
                    }
                    _ if !invoked(&rest, &v, &clauses) => Stmt::New(v, captured, clauses, rest),
                    // its one use: the clause moves there
                    1 => {
                        changed = true;
                        transform(*rest, &mut |s| match s {
                            Stmt::Invoke(x, m, args) if x == v => instantiate(clause(&clauses, &m).clone(), args),
                            other => other,
                        })
                    }
                    // used more, but small: each call gets a copy, and the
                    // function stays for whoever else wants it
                    _ if clauses.iter().all(|(_, b)| size(&b.body) <= SMALL) => {
                        changed = true;
                        let rest = transform(*rest, &mut |s| match s {
                            Stmt::Invoke(x, m, args) if x == v => {
                                let b = clause(&clauses, &m);
                                self.copy(&b.params, &b.body, args)
                            }
                            other => other,
                        });
                        Stmt::New(v, captured, clauses, Box::new(rest))
                    }
                    _ => Stmt::New(v, captured, clauses, rest),
                },
                Stmt::Let(v, m, fields, rest) => match uses(&rest, &v) {
                    0 => {
                        changed = true;
                        *rest
                    }
                    1 if switched(&rest, &v, &m, fields.len()) => {
                        changed = true;
                        transform(*rest, &mut |s| match s {
                            Stmt::Switch(x, clauses) if x == v => instantiate(clause(&clauses, &m).clone(), fields.clone()),
                            other => other,
                        })
                    }
                    _ => Stmt::Let(v, m, fields, rest),
                },
                other => other,
            });
            if !changed {
                return s;
            }
        }
    }
}

fn clause<'c>(clauses: &'c Clauses, m: &Symbol) -> &'c Branch {
    &clauses.iter().find(|(n, _)| n == m).expect("`invoked` and `switched` have looked").1
}

/// Whether `v` is invoked in `s`, and every time with a clause we have.
fn invoked(s: &Stmt, v: &Var, clauses: &Clauses) -> bool {
    fn count(s: &Stmt, v: &Var, clauses: &Clauses) -> (usize, usize) {
        let known = |m: &Symbol, args: &[Var]| clauses.iter().any(|(n, b)| n == m && b.params.len() == args.len());
        let here = match s {
            Stmt::Invoke(x, m, args) if x == v => (1, usize::from(known(m, args))),
            _ => (0, 0),
        };
        children(s).into_iter().map(|c| count(c, v, clauses)).fold(here, |a, b| (a.0 + b.0, a.1 + b.1))
    }
    let (all, known) = count(s, v, clauses);
    all > 0 && all == known
}

/// Whether some `switch` in `s` is on `v`, with a clause for `m` and its
/// `fields`.
fn switched(s: &Stmt, v: &Var, m: &Symbol, fields: usize) -> bool {
    matches!(s, Stmt::Switch(x, clauses) if x == v && clauses.iter().any(|(n, b)| n == m && b.params.len() == fields))
        || children(s).into_iter().any(|c| switched(c, v, m, fields))
}
