//! Auto arguments (phase 4 of the roadmap): an implicit argument found by
//! search, `{auto d : t}` or `t => ..`, among the auto binders in scope and
//! the definitions marked `@hint`, as Idris finds them. The machinery traits
//! are made of.

use idyllic::pipeline::{self, Evaluation, Options};
use idyllic_diagnostics::OutputStyle;

const OPTIONS: Options = Options { debug: false, style: OutputStyle::PLAIN, live: false };

/// A record to search for, two hints for it, and a function wanting one.
const DESCRIBE: &str = "record Describe (a : Type) = { describe : a -> String }\n\n\
                        @hint\ndescribeInt : Describe Int\ndescribeInt = Describe { describe = \\(n : Int) -> showInt n }\n\n\
                        @hint\ndescribeBool : Describe Bool\ndescribeBool = Describe { describe = \\(b : Bool) -> if b then \"yes\" else \"no\" }\n\n\
                        say : {a : Type} -> {auto d : Describe a} -> a -> String\nsay x = d.describe x\n\n";

fn run(src: &str) -> pipeline::Output {
    pipeline::run(src, &OPTIONS)
}

fn value(src: &str) -> String {
    let out = run(src);
    let said: Vec<String> = out.diagnostics.iter().map(|d| d.message.clone()).collect();
    assert!(said.is_empty(), "{src}\n{said:#?}");
    match out.evaluation {
        Some(Evaluation::Value(v)) => v,
        other => panic!("{src}\n{other:?}"),
    }
}

/// The headlines of a program that does not check.
fn rejected(src: &str) -> Vec<String> {
    let out = run(src);
    assert!(out.has_errors(), "{src} checked");
    out.diagnostics.iter().map(|d| d.message.clone()).collect()
}

#[test]
fn a_hint_gives_an_auto_argument() {
    assert_eq!(value(&format!("{DESCRIBE}(say 3, say True)\n")), "(\"3\", \"yes\")");
}

#[test]
fn an_auto_binder_in_scope_is_found_first() {
    // `C a =>` binds one with no name; what is inside finds it, recursion
    // and all
    let all = "sayAll : {a : Type} -> Describe a => List a -> String\nsayAll Nil = \"\"\nsayAll (x :: xs) = say x ++ \";\" ++ sayAll xs\n\n";
    assert_eq!(value(&format!("{DESCRIBE}{all}sayAll (1 :: 2 :: Nil)\n")), "\"1;2;\"");
    // and it is taken over a hint of the same type: the innermost wins
    let shadow = "twice : {auto d : Describe Int} -> Int -> String\ntwice n = say n ++ say n\n\n";
    assert_eq!(value(&format!("{DESCRIBE}{shadow}twice 4\n")), "\"44\"");
}

#[test]
fn a_hint_may_want_auto_arguments_of_its_own() {
    let list = "@hint\ndescribeList : {a : Type} -> Describe a => Describe (List a)\n\
                describeList = Describe { describe = \\(xs : List a) -> \"[\" ++ items xs ++ \"]\" }\n\n\
                items : {a : Type} -> Describe a => List a -> String\nitems Nil = \"\"\nitems (x :: xs) = say x ++ \";\" ++ items xs\n\n";
    assert_eq!(value(&format!("{DESCRIBE}{list}(say (1 :: 2 :: Nil), say ((True :: Nil) :: Nil))\n")), "(\"[1;2;]\", \"[[yes;];]\")");
}

#[test]
fn a_search_waits_for_its_type_to_be_known() {
    // `xs` is a list of something until `4 ::` says what
    let list = "@hint\ndescribeList : {a : Type} -> Describe a => Describe (List a)\n\
                describeList = Describe { describe = \\(xs : List a) -> \"many\" }\n\n";
    assert_eq!(value(&format!("{DESCRIBE}{list}let xs = Nil in say (4 :: xs)\n")), "\"many\"");
}

#[test]
fn hints_are_checked_before_what_wants_them() {
    // declared after their use, the record too
    let src = "say : {a : Type} -> {auto d : Describe a} -> a -> String\nsay x = d.describe x\n\n\
               @hint\ndescribeInt : Describe Int\ndescribeInt = Describe { describe = \\(n : Int) -> showInt n }\n\n\
               record Describe (a : Type) = { describe : a -> String }\n\nsay 7\n";
    assert_eq!(value(src), "\"7\"");
}

#[test]
fn nothing_giving_one_is_an_error() {
    assert_eq!(rejected(&format!("{DESCRIBE}say \"s\"\n")), ["nothing in scope gives a Describe String"]);
}

#[test]
fn two_hints_giving_one_is_an_error() {
    let again = "@hint\ndescribeInt2 : Describe Int\ndescribeInt2 = Describe { describe = \\(n : Int) -> \"\" }\n\n";
    assert_eq!(rejected(&format!("{DESCRIBE}{again}say 1\n")), ["more than one hint gives a Describe Int"]);
}

#[test]
fn what_nothing_says_cannot_be_looked_for() {
    // which `Describe` is never known: the list's element type is not
    let list = "@hint\ndescribeList : {a : Type} -> Describe a => Describe (List a)\n\
                describeList = Describe { describe = \\(xs : List a) -> \"many\" }\n\n";
    let said = rejected(&format!("{DESCRIBE}{list}say Nil\n"));
    assert!(said.iter().any(|s| s == "cannot tell what to look for"), "{said:?}");
}

#[test]
fn a_constraint_is_printed_as_written() {
    let out = run(&format!("{DESCRIBE}shout : {{a : Type}} -> Describe a => a -> String\nshout x = say x ++ \"!\"\n\nshout\n"));
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(out.body_type.as_deref(), Some("{a : Type} -> Describe a => a -> String"));
}

#[test]
fn a_proof_is_found_as_any_auto_argument_is() {
    // search works for any type: `LTE n m`, by its hints, erased
    let lte = "data Nat = Z | S Nat\n\nuse Nat.*\n\n\
               data LTE : Nat -> Nat -> Type where\n  LTEZero : LTE Z n\n  LTESucc : LTE n m -> LTE (S n) (S m)\n\nuse LTE.*\n\n\
               @hint\nlteZero : {n : Nat} -> LTE Z n\nlteZero = LTEZero\n\n\
               @hint\nlteSucc : {n m : Nat} -> {auto p : LTE n m} -> LTE (S n) (S m)\nlteSucc = LTESucc p\n\n\
               smaller : (n : Nat) -> (m : Nat) -> {auto p : [0] LTE n m} -> Int\nsmaller n m = 1\n\n";
    // the proof comes after the arguments it is about, and is found then
    assert_eq!(value(&format!("{lte}smaller (S Z) (S (S Z))\n")), "1");
    assert_eq!(rejected(&format!("{lte}smaller (S Z) Z\n")), ["nothing in scope gives a LTE (S Z) Z"]);
}

#[test]
fn hint_goes_on_a_signature() {
    let said = rejected("@hint\ndata T = T\n\n1\n");
    assert!(!said.is_empty());
}
