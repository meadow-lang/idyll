//! Integer addition: syntax, typing, and — because types can mention values —
//! computation during type checking.

use idyllic::diagnostics::OutputStyle;
use idyllic::pipeline::{self, Options};

/// The inferred type and the diagnostic headlines.
fn run(src: &str) -> (String, Vec<String>) {
    let out = pipeline::run(src, &Options { debug: false, style: OutputStyle::PLAIN });
    let diags = out
        .diagnostics
        .iter()
        .map(|d| match d.labels.first() {
            Some(l) => format!("{}: {}", d.message, l.message),
            None => d.message.clone(),
        })
        .collect();
    (out.body_type.unwrap_or_else(|| "NONE".into()), diags)
}

fn ok(src: &str, ty: &str) {
    let (got, diags) = run(src);
    assert!(diags.is_empty(), "{src}\nunexpected diagnostics: {diags:#?}");
    assert_eq!(got, ty, "{src}");
}

#[test]
fn addition_has_type_int() {
    ok("1 + 2", "Int");
    ok("1 + 2 + 3", "Int");
    ok("0xFF + 0b1", "Int");
}

#[test]
fn precedence_tighter_than_annotation_and_arrow_looser_than_application() {
    ok("1 + 2 : Int", "Int");
    ok("let f : Int -> Int = \\n -> n in f 1 + f 2", "Int");
    ok("\\(n : Int) -> n + 1", "(n : Int) -> Int");
    ok("(1 + 2)", "Int");
}

#[test]
fn operands_must_be_ints() {
    let (ty, diags) = run("1 + True");
    assert_eq!(ty, "Int");
    assert_eq!(diags, ["type mismatch: expected Int, but this has type Bool"]);

    let (_, diags) = run("\"a\" + 1");
    assert_eq!(diags, ["type mismatch: expected Int, but this has type String"]);
}

#[test]
fn addition_computes_during_type_checking() {
    // `f (2 + 2)` and `f 4` are the same type, so `x` can be used at `f 4`
    ok(
        "\\(f : [0] Int -> Type) (x : f (2 + 2)) -> (x : f 4)",
        "(f : [0] Int -> Type) -> (x : f 4) -> f 4",
    );
    // associativity is observable: (1 + 2) + 3
    ok("\\(f : [0] Int -> Type) (x : f (1 + 2 + 3)) -> (x : f 6)", "(f : [0] Int -> Type) -> (x : f 6) -> f 6");
    // and it is genuinely compared, not ignored
    let (_, diags) = run("\\(f : [0] Int -> Type) (x : f (2 + 2)) -> (x : f 5)");
    assert_eq!(diags, ["type mismatch: expected f 5, but this has type f 4"]);
}

#[test]
fn arbitrary_precision() {
    ok(
        "\\(f : [0] Int -> Type) (x : f (99999999999999999999999999999 + 1)) -> (x : f 100000000000000000000000000000)",
        "(f : [0] Int -> Type) -> (x : f 100000000000000000000000000000) -> f 100000000000000000000000000000",
    );
}

#[test]
fn let_bound_definitions_unfold_through_addition() {
    ok(
        "let two = 1 + 1 in \\(f : [0] Int -> Type) (x : f (two + two)) -> (x : f 4)",
        "(f : [0] Int -> Type) -> (x : f 4) -> f 4",
    );
}

#[test]
fn addition_on_a_variable_is_stuck_but_still_comparable() {
    ok(
        "\\(f : [0] Int -> Type) (n : [0] Int) (x : f (n + 1)) -> (x : f (n + 1))",
        "(f : [0] Int -> Type) -> (n : [0] Int) -> (x : f (n + 1)) -> f (n + 1)",
    );
    // no algebraic laws: structural comparison only
    let (_, diags) = run("\\(f : [0] Int -> Type) (n : [0] Int) (x : f (n + 1)) -> (x : f (1 + n))");
    assert_eq!(diags, ["type mismatch: expected f (1 + n), but this has type f (n + 1)"]);
    // a literal prefix still computes: (1 + 2) + n
    ok(
        "\\(f : [0] Int -> Type) (n : [0] Int) (x : f (1 + 2 + n)) -> (x : f (3 + n))",
        "(f : [0] Int -> Type) -> (n : [0] Int) -> (x : f (3 + n)) -> f (3 + n)",
    );
}

#[test]
fn addition_drives_inference() {
    // the binder's type is discovered from its use as an operand
    ok("\\x -> x + 1", "(x : Int) -> Int");
    ok("(\\x -> x + 1) 2", "Int");
    // a hole as an operand is an Int, and nothing determines which
    let (ty, diags) = run("1 + _");
    assert_eq!(ty, "Int");
    assert_eq!(diags, ["unsolved hole: _ : Int, and nothing determines it"]);
}

#[test]
fn a_hole_solved_later_lets_a_stuck_addition_compute() {
    // After `go _`, the expected types are `f ?n` and `f (?n + 1)`, the
    // latter stuck.  Checking `q : f 3` solves `?n := 3`; only then can
    // `?n + 1` compute to 4 and accept `p : f 4`.
    let src = "\\(f : [0] Int -> Type) (q : f 3) (p : f 4) \
                 (go : (n : Int) -> f n -> f (n + 1) -> Unit) -> go _ q p";
    let (ty, diags) = run(src);
    assert!(ty.ends_with("-> Unit"), "{ty}");
    assert_eq!(diags, ["found hole: _ : Int  =  3"], "{src}");

    // unification does not invert arithmetic: `4 =? ?n + 1` is not solved
    let (_, diags) = run("\\(f : [0] Int -> Type) (p : f 4) (go : (n : Int) -> f (n + 1) -> Unit) -> go _ p");
    assert_eq!(diags.len(), 2, "{diags:#?}");
    assert!(diags[0].starts_with("type mismatch: expected f (?"), "{diags:#?}");
    assert!(diags[1].starts_with("unsolved hole"), "{diags:#?}");
}

#[test]
fn display_of_operator_terms() {
    // nested on the right needs parentheses, on the left does not
    let (_, diags) = run("\\(f : [0] Int -> Type) (a b c : [0] Int) (x : f (a + (b + c))) -> (x : f (a + b + c))");
    assert_eq!(diags, ["type mismatch: expected f (a + b + c), but this has type f (a + (b + c))"]);
}
