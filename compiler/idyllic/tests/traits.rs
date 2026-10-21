//! Traits (phase 4 of the roadmap), as sugar, as Idris has them: a trait is
//! a record of its methods, a method a function of the record found for it
//! (an auto argument), and an instance a hint, a record of the methods it
//! gives, the trait's defaults for those it leaves out.

use idyllic::pipeline::{self, Evaluation, ModuleFile, Options, PackageSource};
use idyllic_diagnostics::OutputStyle;

const OPTIONS: Options = Options { debug: false, style: OutputStyle::PLAIN, live: false };

/// A trait with a default, and two instances: one taking the default, one
/// giving its own.
const DESCRIBE: &str = "trait Describe (a : Type) where\n  describe : a -> String\n  twice : a -> String\n  twice x = describe x ++ describe x\n\n\
                        instance Describe Int where\n  describe n = showInt n\n\n\
                        instance Describe Bool where\n  describe True = \"yes\"\n  describe False = \"no\"\n  twice b = \"twice \" ++ describe b\n\n";

fn value(src: &str) -> String {
    let out = pipeline::run(src, &OPTIONS);
    let said: Vec<String> = out.diagnostics.iter().map(|d| format!("{}: {:?}", d.message, d.labels.first().map(|l| &l.message))).collect();
    assert!(said.is_empty(), "{src}\n{said:#?}");
    match out.evaluation {
        Some(Evaluation::Value(v)) => v,
        other => panic!("{src}\n{other:?}"),
    }
}

/// The headlines of a program that does not check.
fn rejected(src: &str) -> Vec<String> {
    let out = pipeline::run(src, &OPTIONS);
    assert!(out.has_errors(), "{src} checked");
    out.diagnostics.iter().map(|d| d.message.clone()).collect()
}

#[test]
fn a_method_is_found_by_the_type_it_is_used_at() {
    assert_eq!(value(&format!("{DESCRIBE}(describe 3, describe True)\n")), "(\"3\", \"yes\")");
}

#[test]
fn a_method_left_out_is_the_traits_default() {
    // `Int` leaves `twice` to the default; `Bool` gives its own
    assert_eq!(value(&format!("{DESCRIBE}(twice 4, twice False)\n")), "(\"44\", \"twice no\")");
}

#[test]
fn an_instance_may_want_instances_of_its_own() {
    // a list's, from its elements'; its recursion finds itself
    let list = "instance Describe a => Describe (List a) where\n  describe Nil = \".\"\n  describe (x :: xs) = describe x ++ \",\" ++ describe xs\n\n";
    assert_eq!(value(&format!("{DESCRIBE}{list}(describe (1 :: 2 :: Nil), describe ((True :: Nil) :: Nil))\n")), "(\"1,2,.\", \"yes,.,.\")");
    // and a function wanting one says so with a constraint
    let both = "both : {a : Type} -> Describe a => a -> a -> String\nboth x y = describe x ++ \"&\" ++ describe y\n\n";
    assert_eq!(value(&format!("{DESCRIBE}{both}both 1 2\n")), "\"1&2\"");
}

#[test]
fn a_named_instance_is_given_by_hand_and_not_searched() {
    let loud = "instance loud : Describe Int where\n  describe n = showInt n ++ \"!\"\n\n";
    // two instances of `Describe Int`, and no question which: a named one
    // is only ever given
    assert_eq!(value(&format!("{DESCRIBE}{loud}(describe 3, describe @{{loud}} 3, twice @{{loud}} 3)\n")), "(\"3\", \"3!\", \"3!3!\")");
}

#[test]
fn traits_and_instances_cross_modules() {
    let shows = "trait Describe (a : Type) where\n  describe : a -> String\n  twice : a -> String\n  twice x = describe x ++ describe x\n\n\
                 instance Describe Int where\n  describe n = showInt n\n";
    let main = "use Shows (Describe, describe, twice)\n\ndata Color = Red | Blue\n\nuse Color.*\n\n\
                instance Describe Color where\n  describe Red = \"red\"\n  describe Blue = \"blue\"\n\n(twice 4, twice Blue)\n";
    let modules = vec![
        ModuleFile { path: Vec::new(), file: "src/Main.idl".into(), text: main.into() },
        ModuleFile { path: vec!["Shows".into()], file: "src/Shows.idl".into(), text: shows.into() },
    ];
    let package = PackageSource { name: "App".into(), version: None, linear: false, modules };
    let out = pipeline::run_package(&package, &OPTIONS, Vec::new());
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
    assert_eq!(out.evaluation, Some(Evaluation::Value("(\"44\", \"blueblue\")".into())));
}

#[test]
fn two_instances_for_types_that_unify_overlap() {
    let again = "instance Describe Int where\n  describe n = \"\"\n\n";
    assert_eq!(rejected(&format!("{DESCRIBE}{again}1\n")), ["this instance Describe Int overlaps another"]);
    // as do one for every list and one for lists of `Int`s
    let lists = "instance Describe a => Describe (List a) where\n  describe xs = \"a list\"\n\n\
                 instance Describe (List Int) where\n  describe xs = \"ints\"\n\n";
    assert_eq!(rejected(&format!("{DESCRIBE}{lists}1\n")), ["this instance Describe (List Int) overlaps another"]);
}

#[test]
fn a_superclass_is_a_field_found_for_the_instance() {
    // `Loud` has `Describe` inside it: its default, and a function wanting
    // only `Loud a`, use `describe` through it
    let loud = "trait Describe a => Loud (a : Type) where\n  shout : a -> String\n  shout x = describe x ++ \"!\"\n\n\
                instance Loud Int where\n  shout n = describe n ++ \"!!\"\n\ninstance Loud Bool where\n\n\
                louder : {a : Type} -> Loud a => a -> String\nlouder x = shout x ++ \" (\" ++ describe x ++ \")\"\n\n";
    assert_eq!(value(&format!("{DESCRIBE}{loud}(shout 3, shout True, louder 4, louder False)\n")), "(\"3!!\", \"yes!\", \"4!! (4)\", \"no! (no)\")");
    // an instance needs its superclasses' instances
    let missing = "trait Describe a => Loud (a : Type) where\n  shout : a -> String\n\ninstance Loud String where\n  shout s = s\n\n1\n";
    assert_eq!(rejected(&format!("{DESCRIBE}{missing}")), ["nothing in scope gives a Describe String"]);
}

#[test]
fn an_instance_gives_its_traits_methods() {
    // a method it leaves out that has no default
    let none = "trait Size (a : Type) where\n  size : a -> Int\n  name : a -> String\n\ninstance Size Int where\n  size n = n\n\n1\n";
    assert!(rejected(none).iter().any(|e| e.contains("is missing name")), "{:?}", rejected(none));
    // a default for something the trait does not declare
    let stray = "trait Size (a : Type) where\n  size : a -> Int\n  name n = \"\"\n\n1\n";
    assert!(!rejected(stray).is_empty());
    // a method with no instance for its type
    assert_eq!(rejected(&format!("{DESCRIBE}describe \"s\"\n")), ["nothing in scope gives a Describe String"]);
}

#[test]
fn a_string_with_holes_displays_or_debugs_each() {
    assert_eq!(value(r#"let name = "world" in "hello ${name}!""#), "\"hello world!\"");
    assert_eq!(value(r#""${1 + 2} and ${(3, "s"):?}""#), r#""3 and (3, \"s\")""#);
    // a hole holds any expression, a string with holes of its own too
    assert_eq!(value(r#""<${"x" ++ "${4}"}>""#), "\"<x4>\"");
    // `\$` is a dollar, and a hole of a type with no `Display` says so
    assert_eq!(value(r#""\${x}""#), r#""${x}""#);
    let none = "data Color = Red\n\nuse Color.*\n\n\"${Red}\"\n";
    assert_eq!(rejected(none), ["nothing in scope gives a Display Color"]);
    // the prelude's `display` and `++`, whatever else is called that
    assert_eq!(value("display : Int -> Int\ndisplay n = n\n\n\"${5}\"\n"), "\"5\"");
}
