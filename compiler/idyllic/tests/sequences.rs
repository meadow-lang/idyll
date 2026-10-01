//! Sequence literals, `#[a, b, c]` (the last of phase 1): built with the
//! constructors of whatever `@seq` type is expected, a `List` when nothing
//! says which, and the same form as a pattern.

use idyllic::pipeline::{self, Evaluation, Options};
use idyllic_diagnostics::OutputStyle;

fn run(src: &str) -> pipeline::Output {
    pipeline::run(src, &Options { debug: false, style: OutputStyle::PLAIN, live: false })
}

fn diags(out: &pipeline::Output) -> Vec<String> {
    out.diagnostics
        .iter()
        .map(|d| match d.labels.first() {
            Some(l) => format!("{}: {}", d.message, l.message),
            None => d.message.clone(),
        })
        .collect()
}

/// `value : type`, as the REPL prints it.
fn result(src: &str) -> String {
    let out = run(src);
    assert!(out.diagnostics.is_empty(), "{src}\nunexpected diagnostics: {:#?}", diags(&out));
    match (&out.evaluation, &out.body_type) {
        (Some(Evaluation::Value(v)), Some(ty)) => format!("{v} : {ty}"),
        other => panic!("{src}\nexpected a value, got {other:?}"),
    }
}

fn rejects(src: &str, expected: &[&str]) {
    assert_eq!(diags(&run(src)), expected, "{src}");
}

const VEC: &str = "data Nat = Z | S Nat

use Nat.*

@seq(nil = VNil, cons = VCons)
data Vec : Nat -> Type -> Type where
  VNil : Vec Z a
  VCons : a -> Vec n a -> Vec (S n) a

use Vec.*

";

#[test]
fn with_nothing_to_say_otherwise_a_sequence_is_a_list() {
    assert_eq!(result("#[1, 2, 3]\n"), "1 :: 2 :: 3 :: Nil : List Int");
    assert_eq!(result("#[#[1], #[2, 3]]\n"), "(1 :: Nil) :: (2 :: 3 :: Nil) :: Nil : List (List Int)");
    assert_eq!(result("(#[] : List Bool)\n"), "Nil : List Bool");
}

#[test]
fn an_indexed_family_gets_its_length_for_nothing() {
    let src = format!("{VEC}xs : Vec (S (S Z)) Int\nxs = #[1, 2]\n\nxs\n");
    assert_eq!(result(&src), "VCons 1 (VCons 2 VNil) : Vec (S (S Z)) Int");
    rejects(
        &format!("{VEC}(#[1, 2] : Vec (S Z) Int)\n"),
        &["type mismatch: expected Vec Z Int, but this has type Vec (S Z) Int"],
    );
}

#[test]
fn a_sequence_pattern_takes_apart_the_matched_type() {
    let src = format!(
        "{VEC}\
         sum : Vec n Int -> Int\n\
         sum #[] = 0\n\
         sum (VCons x rest) = x + sum rest\n\n\
         second : (xs : [] List Int) -> Int\n\
         second #[_, y] = y\n\
         second _ = 0\n\n\
         sum #[1, 2, 3] + second #[10, 20] + second #[1]\n"
    );
    assert_eq!(result(&src), "26 : Int");
}

#[test]
fn a_sequence_pattern_can_be_all_a_head_needs() {
    // the Nil case is impossible, and needs no equation
    let src = format!("{VEC}head : (v : [0..1] Vec (S n) a) -> a\nhead #[x] = x\nhead (VCons x _) = x\n\nhead #[7, 8]\n");
    assert_eq!(result(&src), "7 : Int");
}

#[test]
fn an_element_is_checked_against_the_type_it_must_have() {
    // so `id` is instantiated rather than kept whole
    let src = "id : {a : Type} -> a -> a\nid x = x\n\n(#[id, \\x -> x + 1] : List (Int -> Int))\n";
    let out = run(src);
    assert!(out.diagnostics.is_empty(), "{:#?}", diags(&out));
    assert_eq!(out.body_type.as_deref(), Some("List (Int -> Int)"));
}

#[test]
fn layout_is_suspended_inside_a_sequence() {
    let src = "f : Bool -> List Int\nf b = #[ case b of\n           True -> 1\n           False -> 2\n       , 3\n       ]\n\nf False\n";
    assert_eq!(result(src), "2 :: 3 :: Nil : List Int");
}

#[test]
fn only_a_sequence_type_can_be_built_or_matched() {
    let not = "a sequence literal needs a sequence type: the type wanted here is Int, which #[..] cannot build";
    rejects("(#[1] : Int)\n", &[not]);
    rejects("f : Int -> Int\nf #[] = 0\nf _ = 1\n\nf 3\n", &[not]);
}

#[test]
fn seq_names_the_right_constructors() {
    rejects(
        "@seq(nil = Cons, cons = Nil)\ndata L a = Nil | Cons a (L a)\n\nuse L.*\n\nNil\n",
        &["L cannot be built with #[..]: Cons is the empty sequence, so it has 0 fields; Cons has 2"],
    );
    rejects(
        "data Two = A | B\n\nuse Two.*\n\n@seq(nil = A, cons = B)\ndata L a = Nil | Cons a (L a)\n\nuse L.*\n\nNil\n",
        &["L has no constructor A: used here", "L has no constructor B: used here"],
    );
}

#[test]
fn an_array_is_not_a_sequence() {
    rejects("#[1; 2]\n", &["parse error: arrays, #[a; b], are not supported yet; a sequence is #[a, b]"]);
    rejects("#[1, 2,]\n", &["parse error: a sequence's elements are separated by single commas"]);
}
