//! One spelling, several meanings, and the types to tell them apart, as
//! Meadow has it (its `buildtools/meadow/tests/overloading.rs`).
//!
//! A module's own top-level names and the names its `use`s bring in live in
//! one layer, where a clash is not settled by which came last but by type:
//! whichever meaning fits where the name is used is the one meant. When none
//! does, or more than one still does once the definition is checked, that is
//! an error that lists what was in scope. Locals still shadow, and so does
//! the prelude.

use idyllic::pipeline::{self, Evaluation, ModuleFile, Options, PackageSource};
use idyllic_diagnostics::OutputStyle;

const OPTIONS: Options = Options { debug: false, style: OutputStyle::PLAIN, live: false };

/// A module with its own `Just`, beside the prelude's, which `use Maybe.*`
/// brings.
const BOX: &str = "use Maybe.*\nuse Box.*\n\ndata Box = Just Int | Empty\n\n\
                   unbox : Box -> Int\nunbox (Just n) = n\nunbox Empty = 0\n\n";

fn diags(out: &pipeline::Output) -> Vec<String> {
    out.diagnostics.iter().map(|d| d.message.clone()).collect()
}

/// Everything the diagnostics say, help included.
fn said(out: &pipeline::Output) -> String {
    out.diagnostics
        .iter()
        .map(|d| {
            let mut s = d.message.clone();
            for h in &d.help {
                s.push('\n');
                s.push_str(h);
            }
            s
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn value(src: &str) -> String {
    let out = pipeline::run(src, &OPTIONS);
    assert!(out.diagnostics.is_empty(), "{src}\n{:#?}", diags(&out));
    match out.evaluation {
        Some(Evaluation::Value(v)) => v,
        other => panic!("{src}\n{other:?}"),
    }
}

fn rejected(src: &str) -> pipeline::Output {
    let out = pipeline::run(src, &OPTIONS);
    assert!(out.has_errors(), "{src} checked");
    out
}

/// A package called `App`: the root's text, then each other module's path
/// and text.
fn package(root: &str, others: &[(&str, &str)]) -> PackageSource {
    let mut modules = vec![ModuleFile { path: Vec::new(), file: "src/Main.idl".into(), text: root.into() }];
    for (path, text) in others {
        let path: Vec<String> = path.split('.').map(String::from).collect();
        let file = format!("src/{}.idl", path.join("/"));
        modules.push(ModuleFile { path, file, text: (*text).into() });
    }
    PackageSource { name: "App".into(), version: None, linear: false, modules }
}

fn package_value(root: &str, others: &[(&str, &str)]) -> String {
    let out = pipeline::run_package(&package(root, others), &OPTIONS, Vec::new());
    assert!(out.diagnostics.is_empty(), "{root}\n{:#?}", diags(&out));
    match out.evaluation {
        Some(Evaluation::Value(v)) => v,
        other => panic!("{root}\n{other:?}"),
    }
}

// --- choosing by type ---------------------------------------------------------

#[test]
fn a_constructor_clashing_with_an_import_is_chosen_by_type() {
    assert_eq!(value(&format!("{BOX}(unbox (Just 3), case Just \"s\" of\n  Just s -> s\n  Nothing -> \"\")\n")), "(3, \"s\")");
}

#[test]
fn a_pattern_is_chosen_by_what_it_matches() {
    let src = format!("{BOX}orZero : Maybe Int -> Int\norZero (Just n) = n\norZero Nothing = 0\n\norZero (Just 4) + unbox Empty\n");
    assert_eq!(value(&src), "4");
}

#[test]
fn a_value_clashing_across_imports_is_chosen_by_type() {
    // `length` of a list and of an array, both brought in
    let src = "use Std.Collections.List (length)\nuse Std.Collections.Array (length)\n\n\
               (length (1 :: 2 :: 3 :: Nil), length #[4; 5])\n";
    assert_eq!(value(src), "(3, 2)");
}

#[test]
fn a_value_clashing_across_modules_is_chosen_by_type() {
    let shapes = "use Shape.*\n\ndata Shape = Circle Int | Square Int\n\n\
                  size : Shape -> Int\nsize (Circle r) = 3 * r * r\nsize (Square w) = w * w\n";
    let main = "use Shapes (Shape, size)\nuse Shapes.Shape.*\nuse Box.*\n\ndata Box = Box Int\n\n\
                size : Box -> Int\nsize (Box n) = n\n\n(size (Circle 2), size (Box 5))\n";
    assert_eq!(package_value(main, &[("Shapes", shapes)]), "(12, 5)");
}

#[test]
fn names_tied_together_are_chosen_together() {
    // two `length`s and two `Just`s: no one name settles it, but only one
    // pair has a type
    let src = format!("{BOX}use Std.Collections.List (length)\n\nlength : Box -> Int\nlength b = unbox b\n\nlength (Just 5)\n");
    assert_eq!(value(&src), "5");
}

#[test]
fn a_local_binding_waits_for_the_body_around_it() {
    assert_eq!(value(&format!("{BOX}let b = Just 7 in unbox b\n")), "7");
}

#[test]
fn a_local_shadows_rather_than_overloads() {
    assert_eq!(value(&format!("{BOX}let unbox = \\(x : Int) -> x + 1 in unbox 1\n")), "2");
}

#[test]
fn the_prelude_is_shadowed_not_overloaded() {
    // `map` is not overloaded by the prelude's anything: this one is meant
    assert_eq!(value("map : (Int -> Int) -> Int -> Int\nmap f x = f x\n\nmap (\\(n : Int) -> n + 1) 41\n"), "42");
}

#[test]
fn a_name_in_a_signature_is_chosen_by_its_type() {
    // the type `Box` and the module's `Box` constructor, from a `use`: in a
    // type, a type is wanted
    let src = format!("{BOX}twice : Box -> Box\ntwice (Just n) = Just (n + n)\ntwice Empty = Empty\n\nunbox (twice (Just 4))\n");
    assert_eq!(value(&src), "8");
}

// --- when the types do not settle it -----------------------------------------

#[test]
fn nothing_settling_it_is_ambiguous() {
    let out = rejected(&format!("{BOX}let f = \\x -> case x of {{ Just n -> 1; _ -> 0 }} in 2\n"));
    let e = said(&out);
    assert!(e.contains("ambiguous `Just`"), "{e}");
    assert!(e.contains("Maybe.Just") && e.contains("Box.Just"), "{e}");
}

#[test]
fn a_top_level_definition_settles_its_own_names() {
    // a later use does not reach back: `v`'s type is its own business
    let e = said(&rejected(&format!("{BOX}v = Just 7\n\nunbox v\n")));
    assert!(e.starts_with("ambiguous `Just`: 2 of the candidates in scope fit"), "{e}");
    assert_eq!(value(&format!("{BOX}v : Box\nv = Just 7\n\nunbox v\n")), "7");
}

#[test]
fn no_candidate_fitting_names_the_type_wanted() {
    let out = rejected(&format!("{BOX}f : Bool -> Bool\nf b = b\n\nf (Just 1)\n"));
    let e = said(&out);
    assert!(e.starts_with("no `Just` in scope has the type needed here, `Bool`"), "{e}");
    assert!(e.contains("candidates in scope:"), "{e}");
    assert!(e.contains("Box.Just : (_ : [1] Int) -> Box, from this module"), "{e}");
}

#[test]
fn no_combination_fitting_is_one_error() {
    // each name fits alone -- either `length` takes *something*, either
    // `Just` makes *something* -- but no `length` takes what any `Just` makes
    let src = format!("{BOX}use Std.Collections.List (length)\nuse Shape.*\n\ndata Shape = Circle Int\n\n\
                       length : Shape -> Int\nlength (Circle r) = r\n\nlength (Just 5)\n");
    let out = rejected(&src);
    assert_eq!(diags(&out), ["no `length` in scope fits here"], "{}", said(&out));
}

#[test]
fn candidates_from_other_modules_say_where_they_are_from() {
    let shapes = "use Shape.*\n\ndata Shape = Circle Int\n\nsize : Shape -> Int\nsize (Circle r) = r\n";
    let main = "use Shapes (Shape, size)\nuse Box.*\n\ndata Box = Box Int\n\nsize : Box -> Int\nsize (Box n) = n\n\nsize True\n";
    let out = pipeline::run_package(&package(main, &[("Shapes", shapes)]), &OPTIONS, Vec::new());
    let e = said(&out);
    assert!(e.contains("size : (_ : [1] Shape) -> Int, from `Shapes`"), "{e}");
    assert!(e.contains("size : (_ : [1] Box) -> Int, from this module"), "{e}");
}

// --- duplicates ----------------------------------------------------------------

#[test]
fn defining_a_name_twice_in_one_module_is_still_an_error() {
    let out = rejected("a : Int -> Int\na x = 1\n\na : Int -> Int\na x = 2\n\na 0\n");
    assert!(!diags(&out).is_empty());
}

#[test]
fn the_same_name_in_two_modules_is_not_a_duplicate() {
    let a = "twice : Int -> Int\ntwice x = x + x\n";
    let main = "use A (twice)\n\ntwice : String -> String\ntwice s = s ++ s\n\n(twice 2, twice \"ab\")\n";
    assert_eq!(package_value(main, &[("A", a)]), "(4, \"abab\")");
}
