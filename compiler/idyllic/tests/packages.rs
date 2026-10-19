//! Packages compiled apart (phase 3 of the roadmap): a package is checked
//! against its dependencies' compiled interfaces, and a program is linked
//! from every package it uses.

use idyllic::pipeline::{self, CompiledPackage, Deps, Evaluation, ModuleFile, Options, PackageSource};
use idyllic_diagnostics::OutputStyle;

const OPTIONS: Options = Options { debug: false, style: OutputStyle::PLAIN, live: false };

/// Where each package's uniques start: far enough apart for these tests.
const CHUNK: usize = 1 << 16;

fn diags(out: &pipeline::Output) -> Vec<String> {
    out.diagnostics
        .iter()
        .map(|d| match d.labels.first() {
            Some(l) => format!("{}: {}", d.message, l.message),
            None => d.message.clone(),
        })
        .collect()
}

fn package(name: &str, root: &str) -> PackageSource {
    let file = format!("{name}/src/Lib.idl");
    PackageSource { name: name.into(), version: None, linear: false, modules: vec![ModuleFile { path: Vec::new(), file, text: root.into() }] }
}

/// Compile a library against what it depends on.
fn library(name: &str, root: &str, all: &[&CompiledPackage], direct: &[(&str, &CompiledPackage)], slot: usize) -> CompiledPackage {
    let out = pipeline::compile_package_with(&package(name, root), Deps { all, direct, first_unique: slot * CHUNK }, &OPTIONS);
    assert!(out.diagnostics.is_empty(), "{name}: {:#?}", diags(&out));
    out.checked.and_then(|c| c.compiled).expect("a package that checks is compiled")
}

fn program(root: &str, all: &[&CompiledPackage], direct: &[(&str, &CompiledPackage)]) -> pipeline::Output {
    let deps = Deps { all, direct, first_unique: 9 * CHUNK };
    pipeline::run_package_with(&package("App", root), deps, &OPTIONS, Vec::new())
}

const UTIL: &str = "@pub\ndouble : Int -> Int\ndouble n = n * 2\n\n\
                    @pub\ndata Nat = Z | S Nat\n\nuse Nat.*\n\n\
                    @pub\nplus : Nat -> Nat -> Nat\nplus Z m = m\nplus (S n) m = S (plus n m)\n\n\
                    @pub\ngreeting : String\ngreeting = \"hello\"\n\n\
                    secret : Int\nsecret = 7\n";

#[test]
fn a_program_uses_a_package_compiled_apart() {
    let util = library("Util", UTIL, &[], &[], 1);
    let out = program(
        "use Util (double, Nat)\nuse Util.Nat.*\n\n\
         count : Nat -> Int\ncount Z = 0\ncount (S n) = 1 + count n\n\n\
         main () = let a = printInt (double 21) in let b = printInt (count (Util.plus (S Z) (S (S Z)))) in putStrLn Util.greeting\n",
        &[&util],
        &[("Util", &util)],
    );
    assert!(out.diagnostics.is_empty(), "{:#?}", diags(&out));
    assert_eq!(out.printed, "42\n3\nhello\n");
}

#[test]
fn another_packages_definitions_compute_in_types() {
    // `plus` is transparent: its body is in Util's interface
    let util = library("Util", UTIL, &[], &[], 1);
    let src = "use Util (Nat, plus)\nuse Util.Nat.*\n\n\
               data Eq : Nat -> Nat -> Type where\n  Refl : Eq x x\n\nuse Eq.*\n\n\
               (Refl : Eq (plus (S Z) (S Z)) (S (S Z)))\n";
    let out = program(src, &[&util], &[("Util", &util)]);
    assert!(out.diagnostics.is_empty(), "{:#?}", diags(&out));
    assert_eq!(out.body_type.as_deref(), Some("Eq (S (S Z)) (S (S Z))"));
}

#[test]
fn only_what_a_package_makes_pub_crosses() {
    let util = library("Util", UTIL, &[], &[], 1);
    let out = program("use Util (secret)\n\nsecret\n", &[&util], &[("Util", &util)]);
    assert_eq!(diags(&out), ["secret is private to Util: used here"]);
    // and a package is named only by what depends on it
    let out = program("Util.double 1\n", &[&util], &[]);
    assert_eq!(diags(&out), ["no module or type Util: nothing of this name is in scope"]);
}

#[test]
fn a_chain_of_packages_links_in_order() {
    // Text uses Util, App uses both; Util's definitions are computed before
    // Text's, and Text's before App's
    let util = library("Util", UTIL, &[], &[], 1);
    let text = library(
        "Text",
        "use Util (double, greeting)\n\n@pub\nshout : Int\nshout = double (double 5)\n\n@pub\nhello : String\nhello = greeting\n",
        &[&util],
        &[("Util", &util)],
        2,
    );
    let out = program(
        "use Text (shout, hello)\nuse Util (double)\n\nmain () = let a = printInt (double shout) in putStrLn hello\n",
        &[&util, &text],
        &[("Util", &util), ("Text", &text)],
    );
    assert!(out.diagnostics.is_empty(), "{:#?}", diags(&out));
    assert_eq!(out.printed, "40\nhello\n");
    // it ran to the end: a `main` run has no answer to show
    assert_eq!(out.evaluation, Some(Evaluation::Opaque));
}

#[test]
fn a_packages_types_are_its_own() {
    // two packages each declaring a `Shape` have two types
    let a = library("A", "@pub\ndata Shape = Circle Int\n", &[], &[], 1);
    let b = library("B", "@pub\ndata Shape = Circle Int\n\n@pub\nradius : Shape -> Int\nradius s = case s of\n  Shape.Circle r -> r\n", &[], &[], 2);
    let out = program("use A (Shape)\nuse B (radius)\n\nradius (Shape.Circle 1)\n", &[&a, &b], &[("A", &a), ("B", &b)]);
    assert_eq!(diags(&out), ["type mismatch: expected Shape, but this has type Shape"]);
}

#[test]
fn a_program_runs_from_compiled_packages_alone() {
    // what a build does when nothing changed: every package, the program's
    // own too, read back compiled, linked, and run
    let util = library("Util", UTIL, &[], &[], 1);
    let root = "use Util (double)\n\nmain () = printInt (double 21)\n";
    let deps = Deps { all: &[&util], direct: &[("Util", &util)], first_unique: 2 * CHUNK };
    let out = pipeline::check_package_with(&package("App", root), deps, &OPTIONS, true);
    assert!(out.diagnostics.is_empty(), "{:#?}", diags(&out));
    assert!(out.evaluation.is_none(), "only checked");
    let app = out.checked.and_then(|c| c.compiled).expect("compiled");
    assert!(app.main);
    let out = pipeline::run_compiled(&[&util, &app], &OPTIONS, Vec::new());
    assert!(out.diagnostics.is_empty(), "{:#?}", diags(&out));
    assert_eq!(out.printed, "42\n");
    assert!(out.ran_main);
    // and a program with an expression shows its value
    let out = pipeline::check_package_with(&package("App", "use Util (double)\n\ndouble 4\n"), deps, &OPTIONS, true);
    let app = out.checked.and_then(|c| c.compiled).expect("compiled");
    let out = pipeline::run_compiled(&[&util, &app], &OPTIONS, Vec::new());
    assert_eq!(out.body_type.as_deref(), Some("Int"));
    assert!(matches!(&out.evaluation, Some(Evaluation::Value(v)) if v == "8"), "{:?}", out.evaluation);
}

#[test]
fn a_compiled_package_survives_being_saved() {
    // a build keeps compiled packages on disk between runs
    let util = library("Util", UTIL, &[], &[], 1);
    let bytes = postcard::to_stdvec(&util).expect("serializes");
    let back: CompiledPackage = postcard::from_bytes(&bytes).expect("deserializes");
    let out = program("use Util (double, greeting)\n\nmain () = let a = printInt (double 21) in putStrLn greeting\n", &[&back], &[("Util", &back)]);
    assert!(out.diagnostics.is_empty(), "{:#?}", diags(&out));
    assert_eq!(out.printed, "42\nhello\n");
}
