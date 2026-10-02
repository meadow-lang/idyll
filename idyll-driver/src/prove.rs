//! **Proof search**: a term of a type, found for you.  The REPL's `:prove`,
//! and a query starting with `?` in the `Ctrl-F` finder.
//!
//! A type is a proposition and a term of it a proof, so asking for a term of
//! `{a : Type} -> Maybe a -> a -> a` is asking for a proof of it.  The search
//! is the plain kind Agda's `auto` began as: it builds candidate terms
//! from the goal's shape, smallest first, and the type checker decides
//! which of them are terms of the goal.  Every answer shown has been checked,
//! by the same checker as everything else, with grades and dependent types
//! and all; the search only proposes.
//!
//! What it proposes, for a goal of a function type: a name whose type is the
//! goal, and a lambda for each argument with a term of the result under it.
//! For any other goal: a variable in scope of its type, a literal of a
//! builtin type, a constant, and a function or constructor whose result
//! is the goal, applied to terms found for its arguments in turn, to a
//! depth of three.  What a name's type has to be, it learns from the
//! type's shape (see [`crate::find::shape`]): enough to rule out most of
//! what cannot fit, and never enough to let anything through that the
//! checker would not.
//!
//! It does not split cases: a goal that needs a `case` is beyond it.

use crate::find::shape::{self, Sig, Subst, Ty, DECL};
use crate::find::{Decl, Kind};
use idyllic::pipeline::{self, Options};
use std::collections::HashSet;

/// How deep a term may be nested: `f (g (h x))` is three.
const DEPTH: usize = 3;

/// How many candidates are made before the checker sees any.
const BUDGET: usize = 600;

/// How many terms are tried for each argument of a function.
const PER_ARGUMENT: usize = 4;

/// Terms of `goal` (an Idyll type), after the declarations `earlier`, the
/// smallest first and at most `limit` of them; or why the goal is not a
/// type.
pub fn prove(earlier: &str, goal: &str, decls: &[Decl], opts: &Options, limit: usize) -> Result<Vec<String>, String> {
    let goal = goal.trim();
    if goal.is_empty() {
        return Err("prove what? `:prove` takes a type".into());
    }
    let opts = Options { live: false, ..*opts };
    // the goal, as the checker reads it
    let source = format!("{earlier}__goal : {goal}\n__goal = _\n");
    let signature = earlier.len()..earlier.len() + "__goal : ".len() + goal.len();
    let checked = pipeline::check(&source, &opts);
    let wrong: Vec<_> = checked
        .diagnostics
        .iter()
        .filter(|d| d.is_error() && d.labels.iter().any(|l| signature.contains(&l.span.start)))
        .cloned()
        .collect();
    let found = checked.declarations.iter().find(|d| d.name == "__goal");
    let ty = match found {
        Some(d) if wrong.is_empty() => d.ty.clone(),
        _ => {
            let mut sources = checked.sources.clone();
            sources.set_name(0, "<goal>");
            return Err(idyllic_diagnostics::render_map(&wrong, &sources, opts.style));
        }
    };
    let goal_ty = Sig::of_core(&ty).as_ty();
    let mut search = Search {
        heads: decls
            .iter()
            .filter(|d| d.kind.is_value() && (d.kind != Kind::Operation || goal.contains('!')))
            .filter_map(|d| Some((d.spelling.clone(), d.shape.clone()?)))
            .collect(),
        made: 0,
    };
    let mut candidates: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for depth in 0..=DEPTH {
        let mut these = search.terms(&goal_ty, &[], depth);
        // smallest first, within a depth
        these.sort_by_key(String::len);
        for t in these {
            if seen.insert(t.clone()) {
                candidates.push(t);
            }
        }
        if search.made >= BUDGET {
            break;
        }
    }
    let mut proofs = Vec::new();
    for batch in candidates.chunks(64) {
        proofs.extend(verify(earlier, goal, batch, &opts));
        if proofs.len() >= limit {
            break;
        }
    }
    proofs.truncate(limit);
    Ok(proofs)
}

/// Which of `candidates` the checker takes as terms of `goal`, in order:
/// all of them checked at once, each as a definition of its own.
fn verify(earlier: &str, goal: &str, candidates: &[String], opts: &Options) -> Vec<String> {
    let mut source = earlier.to_string();
    let mut ranges = Vec::new();
    for (i, c) in candidates.iter().enumerate() {
        let start = source.len();
        source.push_str(&format!("__proof{i} : {goal}\n__proof{i} = {c}\n\n"));
        ranges.push(start..source.len());
    }
    let checked = pipeline::check(&source, opts);
    let errors: Vec<_> = checked.diagnostics.iter().filter(|d| d.is_error()).collect();
    // an error that says nowhere it is could be any of them's: each is
    // asked about alone
    if errors.iter().any(|d| d.labels.is_empty()) {
        return if candidates.len() == 1 {
            Vec::new()
        } else {
            candidates.iter().flat_map(|c| verify(earlier, goal, std::slice::from_ref(c), opts)).collect()
        };
    }
    let failed: HashSet<usize> = errors
        .iter()
        .flat_map(|d| d.labels.iter())
        .filter_map(|l| ranges.iter().position(|r| r.contains(&l.span.start)))
        .collect();
    candidates.iter().enumerate().filter(|(i, _)| !failed.contains(i)).map(|(_, c)| c.clone()).collect()
}

struct Search {
    /// every name that can head a term, spelled as the prompt spells it,
    /// with its type's shape
    heads: Vec<(String, Sig)>,
    /// how many candidates have been made, against [`BUDGET`]
    made: usize,
}

impl Search {
    /// A name's type, its variables made the name's own: flexible, and
    /// apart from the goal's.  (A goal never has a name's variables in it --
    /// see [`loosen`] -- so every use can share the one offset.)
    fn instance(&self, sig: &Sig) -> Sig {
        Sig { args: sig.args.iter().map(|a| shape::rename(a, DECL)).collect(), result: shape::rename(&sig.result, DECL) }
    }

    /// Terms of `goal` with `locals` in scope, nested at most `depth` deep.
    fn terms(&mut self, goal: &Ty, locals: &[(String, Ty)], depth: usize) -> Vec<String> {
        if self.made >= BUDGET {
            return Vec::new();
        }
        let mut out = Vec::new();
        let fits = |a: &Ty, b: &Ty| Subst::rigid().unify(a, b);
        // what is in scope with the goal's type, a function's or not
        for (name, ty) in locals.iter().rev() {
            if fits(ty, goal) {
                out.push(name.clone());
            }
        }
        let heads = self.heads.clone();
        for (name, sig) in &heads {
            let whole = self.instance(sig).as_ty();
            if (matches!(goal, Ty::Fun(..)) || sig.args.is_empty()) && fits(&whole, goal) {
                out.push(name.clone());
            }
        }
        match goal {
            Ty::Fun(args, result) => {
                // a lambda for each argument
                let names: Vec<String> = (0..args.len()).map(|i| fresh_name(locals.len() + i)).collect();
                let mut inner = locals.to_vec();
                inner.extend(names.iter().cloned().zip(args.iter().cloned()));
                let named: HashSet<String> = out.iter().cloned().collect();
                for body in self.terms(result, &inner, depth) {
                    // `\x -> f x` is `f`, which is there already
                    let eta = body.strip_suffix(&format!(" {}", names.join(" "))).is_some_and(|f| named.contains(f));
                    if !eta {
                        out.push(format!("\\{} -> {body}", names.join(" ")));
                    }
                }
            }
            Ty::Con(name, args) if args.is_empty() => out.extend(literals(name).iter().map(|l| l.to_string())),
            _ => {}
        }
        if depth > 0 && !matches!(goal, Ty::Fun(..)) {
            let mut callable: Vec<(String, Sig)> = locals
                .iter()
                .rev()
                .filter_map(|(n, t)| match t {
                    Ty::Fun(args, r) => Some((n.clone(), Sig { args: args.clone(), result: (**r).clone() })),
                    _ => None,
                })
                .collect();
            callable.extend(heads.iter().filter(|(_, s)| !s.args.is_empty()).cloned());
            for (name, sig) in callable {
                if self.made >= BUDGET {
                    break;
                }
                let sig = self.instance(&sig);
                let mut s = Subst::rigid();
                if !s.unify(&sig.result, goal) {
                    continue;
                }
                let mut each: Vec<Vec<String>> = Vec::new();
                for a in &sig.args {
                    let a = loosen(&s.resolve(a));
                    let mut found = self.terms(&a, locals, depth - 1);
                    found.truncate(PER_ARGUMENT);
                    if found.is_empty() {
                        break;
                    }
                    each.push(found);
                }
                if each.len() < sig.args.len() {
                    continue;
                }
                for args in product(&each, 8) {
                    out.push(apply(&name, &args));
                }
            }
        }
        self.made += out.len();
        out
    }
}

/// A type's variables that are still a name's own, unknown: anything at all
/// will do for them, as far as the search can tell; the checker decides.
fn loosen(t: &Ty) -> Ty {
    match t {
        Ty::Var(v) if *v >= DECL => Ty::Any,
        Ty::Con(n, args) => Ty::Con(n.clone(), args.iter().map(loosen).collect()),
        Ty::Fun(args, r) => Ty::Fun(args.iter().map(loosen).collect(), Box::new(loosen(r))),
        other => other.clone(),
    }
}

/// Literals of a builtin type, the simplest first.
fn literals(ty: &str) -> &'static [&'static str] {
    match ty {
        "Int" => &["0", "1"],
        "Bool" => &["True", "False"],
        "String" => &["\"\""],
        "()" => &["()"],
        _ => &[],
    }
}

/// `x`, `y`, `z`, then `x3`, ...
fn fresh_name(i: usize) -> String {
    match i {
        0 => "x".into(),
        1 => "y".into(),
        2 => "z".into(),
        _ => format!("x{i}"),
    }
}

/// `f a (g b)`.
fn apply(head: &str, args: &[String]) -> String {
    let mut out = head.to_string();
    for a in args {
        if a.contains(' ') && !(a.starts_with('(') && a.ends_with(')')) {
            out.push_str(&format!(" ({a})"));
        } else {
            out.push_str(&format!(" {a}"));
        }
    }
    out
}

/// Every way of taking one from each, the first `limit` of them.
fn product(each: &[Vec<String>], limit: usize) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = vec![Vec::new()];
    for options in each {
        out = out
            .iter()
            .flat_map(|prefix| {
                options.iter().map(move |o| {
                    let mut p = prefix.clone();
                    p.push(o.clone());
                    p
                })
            })
            .take(limit)
            .collect();
    }
    out
}
