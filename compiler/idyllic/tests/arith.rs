//! Integer addition: syntax, typing, and — because types can mention values —
//! computation during type checking.

use idyllic_diagnostics::OutputStyle;
use idyllic::pipeline::{self, Options};

/// The inferred type and the diagnostic headlines.
fn run(src: &str) -> (String, Vec<String>) {
    let out = pipeline::run(src, &Options { debug: false, style: OutputStyle::PLAIN, live: false });
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
    // a number is read at the type wanted, which `True` says is `Bool`
    let (ty, diags) = run("1 + True");
    assert_eq!(ty, "Bool");
    assert_eq!(diags, ["type mismatch: expected Bool, but this has type Int", "nothing in scope gives an Add Bool: an auto argument is wanted here"]);

    // `+` is `Add`'s method, at the type of its first operand: there is no
    // `Add String`, and `1` is no `String`
    let (_, diags) = run("\"a\" + 1");
    assert_eq!(
        diags,
        ["nothing in scope gives an Add String: an auto argument is wanted here", "type mismatch: expected String, but this has type Int"]
    );
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
fn an_int_is_64_bits_and_wraps_in_types_too() {
    // the type checker computes as the runtime does
    ok(
        "\\(f : [0] Int -> Type) (x : f (9223372036854775807 + 1)) -> (x : f (0 - 9223372036854775807 - 1))",
        "(f : [0] Int -> Type) -> (x : f -9223372036854775808) -> f -9223372036854775808",
    );
    let (_, diags) = run("9223372036854775808");
    assert_eq!(diags, ["parse error: 9223372036854775808 does not fit in an Int, which is 64 bits"]);
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
    assert!(ty.ends_with("-> ()"), "{ty}");
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

// A comparison on arithmetic that shares its parts, as `Std.Time.utc`'s
// does, is checked at once: forcing the stuck sum used to walk it as the
// tree it spells out, every shared part again each time, which took a
// second and a half for `utc` and multiplies with each level added here.
#[test]
fn a_comparison_on_shared_arithmetic_is_checked_at_once() {
    let src = "f : (ms : Int) -> Int\n\
               f ms =\n  \
                 let z = ms / 86400000 + 719468 in\n  \
                 let era = z / 146097 in\n  \
                 let doe = z - era * 146097 in\n  \
                 let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365 in\n  \
                 let doy = doe - (365 * yoe + yoe / 4 - yoe / 100) in\n  \
                 let mp = (5 * doy + 2) / 153 in\n  \
                 let q = (7 * mp + 1) / 11 in\n  \
                 let r = (3 * q + 5) / 13 in\n  \
                 if r < 10 then r + 3 else r - 9\n\n\
               f 1000\n";
    let started = std::time::Instant::now();
    ok(src, "Int");
    assert!(started.elapsed() < std::time::Duration::from_secs(30), "took {:?}", started.elapsed());
}
