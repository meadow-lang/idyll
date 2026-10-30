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

#[test]
fn comparisons_are_the_preludes_partial_eq_and_partial_ord() {
    // at Int the primitives; at the prelude's other types their instances
    assert_eq!(value("(1 < 2, 2 > 1, 3 <= 3, 4 >= 5, 1 != 2, 2 == 2)\n"), "(True, True, True, False, True, True)");
    assert_eq!(value("(\"a\" < \"b\", 'a' <= 'b', (1, True) == (1, True), (1 :: 2 :: Nil) != (1 :: Nil), Just 3 == Just 3)\n"), "(True, True, True, True, True)");
    // a type of one's own, by an instance; `>`, `<=` and `>=` its defaults
    let size = "data Size = Small | Large\n\nuse Size.*\n\n\
                rank : Size -> Int\nrank Small = 0\nrank Large = 1\n\n\
                instance PartialEq Size where\n  x == y = rank x == rank y\n\n\
                instance PartialOrd Size where\n  x < y = rank x < rank y\n\n";
    assert_eq!(value(&format!("{size}(Small < Large, Large > Small, Small >= Large, Small != Large)\n")), "(True, True, False, True)");
    // and a type with none cannot be compared
    assert_eq!(rejected("data Color = Red\n\nuse Color.*\n\nRed == Red\n"), ["nothing in scope gives a PartialEq Color"]);
    // `x < 0` at Int is the primitive
    let text = pipeline::compile("f : Int -> Bool\nf x = x < 0\n\nf 1\n", &OPTIONS).cut.expect("lowered");
    assert!(text.contains("prim lt("), "{text}");
}

#[test]
fn derive_writes_display_debug_partial_eq_and_partial_ord() {
    let shape = "@derive(Display, Debug, PartialEq, PartialOrd)\ndata Shape a = Dot | Circle Int | Box a a\n\nuse Shape.*\n\n\
                 dot : Shape Int\ndot = Dot\n\n";
    // a constructor and its fields, each by its own instance, as Meadow's
    assert_eq!(value(&format!("{shape}(display (Box \"a\" \"b\"), debug (Box \"a\" \"b\"), display dot)\n")), r#"("Box(a, b)", "Box(\"a\", \"b\")", "Dot")"#);
    // the same constructor with equal fields; by constructor, then fields
    assert_eq!(value(&format!("{shape}(dot == dot, (Circle 1 : Shape Int) == Circle 2, dot < Circle 0, Box 2 1 < Box 1 2, Box 1 2 < Box 1 3)\n")), "(True, False, True, False, True)");
    // a parameter's instance is wanted for the type's
    let tree = "@derive(PartialEq)\ndata Pair a = Two a a\n\nuse Pair.*\n\ndata Color = Red\n\nuse Color.*\n\n";
    assert_eq!(rejected(&format!("{tree}Two Red Red == Two Red Red\n")), ["nothing in scope gives a PartialEq (Pair Color)"]);
    // the prelude's Maybe and Result are derived
    assert_eq!(value("let x = Just 3 in (\"${x}\", x == Just 3, x < Just 4)\n"), "(\"Just(3)\", True, True)");
    // and only the four can be
    assert!(rejected("@derive(Ord)\ndata T = A\n\n1\n").iter().any(|m| m.contains("parse error")));
    // a field of a type with an error in it says so, once: what is wanted
    // of it is not looked for (every instance would fit, and its wants be
    // looked for in turn, which had taken as long as it liked)
    assert_eq!(rejected("@derive(Debug, PartialEq)\ndata J = N | O (Maybe Foo) | P (List (Foo, Int))\n\n1\n"), ["name not in scope: Foo", "name not in scope: Foo"]);
}

/// A trait with an associated type, and two instances.
const CONTAINER: &str = "trait Container (c : Type) where\n  Elem : Type\n  empty : c\n  insert : Elem -> c -> c\n  toList : c -> List Elem\n\n\
                         instance Container (List Int) where\n  Elem = Int\n  empty = Nil\n  insert x xs = x :: xs\n  toList xs = xs\n\n\
                         record Bag = { items : List String }\n\n\
                         instance Container Bag where\n  Elem = String\n  empty = Bag { items = Nil }\n  insert x b = Bag { items = x :: b.items }\n  toList b = b.items\n\n";

#[test]
fn an_associated_type_is_a_field_that_is_a_type() {
    // each instance says what it is, and the methods' types read it off
    // the instance they are of
    assert_eq!(
        value(&format!("{CONTAINER}(toList (insert 1 (insert 2 (empty : List Int))), toList (insert \"a\" (empty : Bag)))\n")),
        "(1 :: 2 :: Nil, \"a\" :: Nil)"
    );
    // `Elem c` is the type the instance for `c` gives
    assert_eq!(value(&format!("{CONTAINER}e : Elem Bag\ne = \"an element\"\n\ne\n")), "\"an element\"");
    // and a function generic in the container says it of any
    let first = "firstOr : {c : Type} -> {auto d : Container c} -> Elem c -> c -> Elem c\n\
                 firstOr d xs = case toList xs of\n  Nil -> d\n  (x :: rest) -> x\n\n";
    assert_eq!(value(&format!("{CONTAINER}{first}(firstOr 0 (empty : List Int), firstOr \"none\" (insert \"b\" (empty : Bag)))\n")), "(0, \"b\")");
}

#[test]
fn an_associated_type_is_what_its_instance_says() {
    // so a `Bag`'s element is no `Int`
    assert!(rejected(&format!("{CONTAINER}insert 1 (empty : Bag)\n")).iter().any(|m| m == "type mismatch"));
    // and an instance given by hand gives its own
    let named = "instance chars : Container (List Char) where\n  Elem = Char\n  empty = Nil\n  insert x xs = x :: xs\n  toList xs = xs\n\n";
    assert_eq!(value(&format!("{CONTAINER}{named}e : Elem (List Char) @{{chars}}\ne = 'x'\n\ne\n")), "'x'");
}

#[test]
fn an_associated_type_is_declared_in_a_linear_module() {
    // its accessor's parameters only say which instance, so they are
    // erased rather than, in a linear module, to be used once
    // (the list's instance alone: a `Bag`'s reads `b.items`, which uses `b`
    // at most once, not once)
    let list = &CONTAINER[..CONTAINER.find("record Bag").expect("the list's instance first")];
    let src = format!("@linear\n\n{list}e : Elem (List Int)\ne = 3\n\ne\n");
    assert_eq!(value(&src), "3");
}

#[test]
fn an_instance_may_give_its_own_parameter_as_an_associated_type() {
    // `Elem = a`: a type passed where a type is wanted is used nowhere, so
    // the instance's erased `a` may be it
    let generic = "trait Container (c : Type) where\n  Elem : Type\n  toList : c -> List Elem\n\n\
                   instance Container (List a) where\n  Elem = a\n  toList xs = xs\n\n";
    assert_eq!(value(&format!("{generic}e : Elem (List String)\ne = \"s\"\n\n(toList (1 :: 2 :: Nil), e)\n")), "(1 :: 2 :: Nil, \"s\")");
}

#[test]
fn a_subtrait_and_a_record_may_name_an_associated_type_of_what_they_hold() {
    let container = "trait Container (c : Type) where\n  Elem : Type\n  toList : c -> List Elem\n\n\
                     instance Container (List Int) where\n  Elem = Int\n  toList xs = xs\n\n";
    // a subtrait's method, `Elem c` found through its superclass; the
    // subtrait lives where its superclass does
    let shown = "trait Container c => Shown (c : Type) where\n  showFirst : c -> Elem c -> String\n\n\
                 instance Shown (List Int) where\n  showFirst xs d = case toList xs of\n    Nil -> showInt d\n    (x :: r) -> showInt x\n\n";
    assert_eq!(value(&format!("{container}{shown}(showFirst (5 :: Nil) 0, showFirst (Nil : List Int) 7)\n")), "(\"5\", \"7\")");
    // and a record's field, `Elem c @{box}`, read off the field before it
    let record = "record Boxed (c : Type) = { box : Container c, fallback : Elem c @{box} }\n\n";
    let named = "instance chars : Container (List Char) where\n  Elem = Char\n  toList xs = xs\n\n";
    // (it checks; a field whose representation another field says is
    // `any`, which wants a box, so it is not run here)
    let src = format!("{container}{record}{named}b : Boxed (List Char)\nb = Boxed {{ box = chars, fallback = 'x' }}\n\nb.fallback\n");
    let out = pipeline::compile(&src, &OPTIONS);
    assert!(!out.has_errors(), "{src}\n{:#?}", out.diagnostics);
}

#[test]
fn an_associated_type_waits_for_its_instance_inside_a_type() {
    // `pick positive`: `Elem c -> Bool` is checked against `Int -> Bool`
    // before the instance for `c` is found, and is asked again once it is
    let src = "trait Container (c : Type) where\n  Elem : Type\n  toList : c -> List Elem\n\n\
               instance Container (List Int) where\n  Elem = Int\n  toList xs = xs\n\n\
               data Box (c : Type) (a : Type) = Box (c -> Maybe a)\n\nuse Box.*\n\n\
               pick : {c : Type} -> {auto d : Container c} -> (f : Elem c -> Bool) -> Box c (Elem c)\n\
               pick f = Box (\\(x : c) -> case toList x of\n  Nil -> Nothing\n  (y :: r) -> if f y then Just y else Nothing)\n\n\
               positive : Int -> Bool\npositive n = n > 0\n\n\
               b : Box (List Int) Int\nb = pick positive\n\n\
               case b of\n  Box f -> f (3 :: Nil)\n";
    assert_eq!(value(src), "Just 3");
}

