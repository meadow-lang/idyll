//! Representations: how each value of an elaborated program is represented
//! at run time, read off its type by the checker's `Typer`, as the shared
//! back end (Cut) needs them.

use idyllic::pipeline::{self, Options};
use idyllic_diagnostics::OutputStyle;
use idyllic_typecheck::core::{Core, Note};
use idyllic_typecheck::typer::{Rep, Scope, Typer};
use std::rc::Rc;

fn checked(src: &str) -> idyllic_typecheck::TypeCheckResult {
    let out = pipeline::compile(src, &Options { debug: false, style: OutputStyle::PLAIN, live: false });
    assert!(!out.has_errors(), "{src}\n{:#?}", out.diagnostics);
    out.checked.expect("checked")
}

/// Every binder of the program's own (not the prelude's), with its
/// representation, in the order they appear.
fn binders(src: &str) -> Vec<(String, Rep)> {
    let mut tc = checked(src);
    let program = tc.program.clone().expect("a program");
    let prelude = tc.prelude_lets.len();
    let mut out = Vec::new();
    let scope = tc.typer.scope();
    // the scope at each level the top-level `let`s reach, which is where
    // the recursive definitions are
    let mut scopes = vec![scope.clone()];
    walk(&mut tc.typer, &scope, &program, &mut out, &mut scopes);
    out.drain(..prelude.min(out.len()));
    for d in tc.rec_defs.clone() {
        let Some(s) = scopes.iter().find(|s| s.len() == d.lvl).cloned() else { continue };
        walk(&mut tc.typer, &s, &d.core, &mut out, &mut Vec::new());
    }
    out
}

fn walk(t: &mut Typer, s: &Scope, c: &Rc<Core>, out: &mut Vec<(String, Rep)>, scopes: &mut Vec<Scope>) {
    match &**c {
        Core::Let(x, rhs, body) => {
            walk(t, s, rhs, out, &mut Vec::new());
            let inner = t.define(s, x, rhs);
            scopes.push(inner.clone());
            let ty = t.var_type(&inner, inner.len() - 1);
            out.push((x.clone(), t.rep(&inner, &ty)));
            walk(t, &inner, body, out, scopes);
        }
        Core::Runtime(Note::Binds { ty: Some(ty), .. }, inner) => match &**inner {
            Core::Lam(x, body) => {
                let under = t.bind(s, x, ty);
                let x_ty = t.var_type(&under, under.len() - 1);
                out.push((x.clone(), t.rep(&under, &x_ty)));
                walk(t, &under, body, out, &mut Vec::new());
            }
            other => walk(t, s, &Rc::new(other.clone()), out, scopes),
        },
        Core::Runtime(Note::Erased, _) => {}
        Core::Runtime(_, inner) => walk(t, s, inner, out, scopes),
        Core::Lam(x, body) => {
            let under = t.bind_unknown(s, x);
            walk(t, &under, body, out, &mut Vec::new());
        }
        Core::App(f, a) => {
            walk(t, s, f, out, &mut Vec::new());
            walk(t, s, a, out, &mut Vec::new());
        }
        Core::Case(scrutinee, alts) => {
            walk(t, s, scrutinee, out, &mut Vec::new());
            for alt in alts.iter() {
                let inner = t.alt_scope(s, scrutinee, alt);
                for (x, ty) in alt.vars.iter().zip(t.alt_types(s, scrutinee, alt)) {
                    out.push((x.clone(), t.rep(&inner, &ty)));
                }
                walk(t, &inner, &alt.body, out, &mut Vec::new());
            }
        }
        Core::BoxIntro(e) | Core::Unbox(e) => walk(t, s, e, out, &mut Vec::new()),
        _ => {}
    }
}

fn rep_of(src: &str, name: &str) -> Rep {
    let all = binders(src);
    all.iter().find(|(x, _)| x == name).unwrap_or_else(|| panic!("{src}\nno binder {name} in {all:?}")).1
}

#[test]
fn primitive_types_have_their_own_representations() {
    let src = "let n = 2 + 3 in let s = \"a\" in let b = n < 3 in let u = () in n\n";
    assert_eq!(rep_of(src, "n"), Rep::I64);
    assert_eq!(rep_of(src, "s"), Rep::Str);
    assert_eq!(rep_of(src, "b"), Rep::Bool);
    assert_eq!(rep_of(src, "u"), Rep::Unit);
}

#[test]
fn functions_and_data_are_pointers() {
    let src = "data Box = Box Int\n\nuse Box.*\n\nlet f = \\(y : Int) -> y + 1 in let b = Box 3 in f 1\n";
    assert_eq!(rep_of(src, "f"), Rep::Ptr);
    assert_eq!(rep_of(src, "y"), Rep::I64);
    assert_eq!(rep_of(src, "b"), Rep::Ptr);
}

#[test]
fn a_type_variable_is_a_representation_variable() {
    let src = "id : {a : Type} -> a -> a\nid y = y\n\nid 3\n";
    let all = binders(src);
    let a = all.iter().position(|(x, _)| x == "a").expect("the type parameter is a binder");
    assert_eq!(all[a].1, Rep::Erased, "{all:?}");
    match rep_of(src, "y") {
        Rep::Var(_) => {}
        other => panic!("y should be a representation variable, is {other:?}: {all:?}"),
    }
}

#[test]
fn fields_are_instantiated_from_the_scrutinee() {
    // `x :: xs` in a `List Int`: `x` is an `Int`, `xs` a list
    let src = "sum : List Int -> Int\nsum Nil = 0\nsum (x :: xs) = x + sum xs\n\nsum (1 :: 2 :: Nil)\n";
    // (a name can be bound more than once: the match compiler names a
    // function's argument after a pattern's variable)
    let has = |src: &str, name: &str, rep: Rep| binders(src).iter().any(|(x, r)| x == name && *r == rep);
    assert!(has(src, "x", Rep::I64), "{:?}", binders(src));
    assert!(has(src, "xs", Rep::Ptr), "{:?}", binders(src));
    // and of an indexed family, by unifying its type with the scrutinee's
    let vec = "data Nat = Z | S Nat\nuse Nat.*\n\ndata Vec : Nat -> Type -> Type where\n  VNil : Vec Z a\n  VCons : a -> Vec n a -> Vec (S n) a\nuse Vec.*\n\n";
    let total = format!("{vec}total : Vec n Int -> Int\ntotal VNil = 0\ntotal (VCons x xs) = x + total xs\n\ntotal (VCons 1 VNil)\n");
    assert!(has(&total, "x", Rep::I64), "{:?}", binders(&total));
    let src = "data Pair = Pair Int String\n\nuse Pair.*\n\ncase Pair 1 \"a\" of\n  Pair n s -> if n == 1 then s else s\n";
    assert_eq!(rep_of(src, "n"), Rep::I64);
    assert_eq!(rep_of(src, "s"), Rep::Str);
}

#[test]
fn every_program_in_the_corpora_has_representations() {
    // nothing panics, and nothing a program of its own binds is unknown
    // unless its type really is not known when compiled
    let corpora = [include_str!("corpus/handwritten.txt"), include_str!("corpus/tutorial.txt")];
    let (mut programs, mut anys) = (0, Vec::new());
    for corpus in corpora {
        for src in corpus.replace("\r\n", "\n").split("\n%%%\n") {
            // a program that uses Std has Std's definitions among its own
            // `let`s, some of whose types really are unknown when compiled
            // (`subst`'s `px : p x`)
            if src.contains("use Std.") {
                continue;
            }
            let out = pipeline::compile(src, &Options { debug: false, style: OutputStyle::PLAIN, live: false });
            if out.has_errors() || out.checked.as_ref().and_then(|c| c.program.as_ref()).is_none() {
                continue;
            }
            programs += 1;
            for (x, rep) in binders(src) {
                if rep == Rep::Any {
                    anys.push(format!("{x} in:\n{src}"));
                }
            }
        }
    }
    assert!(programs > 40, "only {programs} programs");
    assert!(anys.is_empty(), "{} binders of unknown representation:\n\n{}", anys.len(), anys.join("\n\n"));
}

