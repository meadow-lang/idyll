//! Graded modal types, after Granule ("Quantitative Program Reasoning with
//! Graded Modal Types", Orchard, Liepelt & Eades, ICFP 2019) and, for the
//! dependent part, Graded Modal Dependent Type Theory (Moon, Eades & Orchard,
//! ESOP 2021): a linear base, interval grades over ℕ ∪ {∞}, a graded box
//! modality `a [r]` with promotion and unboxing, grades that are terms (so
//! functions can be polymorphic in them), and grading of types themselves.

use idyllic_diagnostics::OutputStyle;
use idyllic::pipeline::{self, Options};

/// The inferred type and the diagnostics as `headline: label`, of a program
/// whose unannotated binders are linear, the base these tests are about.
fn run(src: &str) -> (String, Vec<String>) {
    let src = format!("@linear\n\n{src}");
    let out = pipeline::run(&src, &Options { debug: false, style: OutputStyle::PLAIN, live: false });
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

fn rejects(src: &str, expected: &str) {
    let (_, diags) = run(src);
    assert_eq!(diags, [expected], "{src}");
}

// ---------------------------------------------------------------------------
// The default: unrestricted, unless the module is linear
// ---------------------------------------------------------------------------

#[test]
fn outside_a_linear_module_a_binder_that_says_nothing_is_unrestricted() {
    let plain = |src: &str| {
        let out = pipeline::run(src, &Options { debug: false, style: OutputStyle::PLAIN, live: false });
        let diags: Vec<String> = out.diagnostics.iter().map(|d| d.message.clone()).collect();
        (out.body_type.unwrap_or_default(), diags)
    };
    // printed as it is read: `[]` goes unsaid, and `1` is said
    assert_eq!(plain("\\(x : Int) -> x + x"), ("(x : Int) -> Int".into(), vec![]));
    assert_eq!(plain("\\(x : Int) -> 5"), ("(x : Int) -> Int".into(), vec![]));
    assert_eq!(plain("\\(x : [1] Int) -> x"), ("(x : [1] Int) -> Int".into(), vec![]));
    assert_eq!(plain("\\(x : [1] Int) -> x + x").1, ["usage mismatch"]);
    // a truth value is looked at once by the `if` that looks at it, whatever
    // the default (it has no fields to hand on)
    assert_eq!(plain("\\(x : [1] Bool) -> if x then 1 else 2").1, Vec::<String>::new());
    // a comparison looks at its operands as often as it likes (as Rust's
    // borrow theirs): what it compares is unrestricted
    assert_eq!(plain("\\(x : [1] Int) -> if x < 0 then 1 else 2").1, ["usage mismatch"]);
    assert_eq!(plain("\\(x : [] Int) -> if x < 0 then 1 else 2").1, Vec::<String>::new());
    // and in a linear module, the other way round
    assert_eq!(run("\\(x : [] Int) -> x + x"), ("(x : [] Int) -> Int".into(), vec![]));
}

// ---------------------------------------------------------------------------
// The linear base (paper §2.1)
// ---------------------------------------------------------------------------

#[test]
fn identity_is_linear() {
    // `id` is "the linear function par excellence"
    ok("(\\x -> x) : {a : Type} -> a -> a", "{a : Type} -> a -> a");
    ok("\\(x : Int) -> x", "(x : Int) -> Int");
}

#[test]
fn flip_is_linear_so_exchange_is_allowed() {
    ok(
        "(\\f y x -> f x y) : {a b c : Type} -> (a -> b -> c) -> b -> a -> c",
        "{a : Type} -> {b : Type} -> {c : Type} -> (a -> b -> c) -> b -> a -> c",
    );
}

#[test]
fn no_weakening_drop_is_rejected() {
    rejects("(\\x -> ()) : {a : Type} -> a -> Unit", "usage mismatch: x has grade 1, but is used never");
}

#[test]
fn no_contraction_copy_is_rejected() {
    rejects("\\(x : Int) -> x + x", "usage mismatch: x has grade 1, but is used 2 times");
}

// ---------------------------------------------------------------------------
// Exact grades on binders
// ---------------------------------------------------------------------------

#[test]
fn a_bare_number_is_an_exact_count() {
    ok("\\(x : [0] Int) -> 5", "(x : [0] Int) -> Int");
    ok("\\(x : [2] Int) -> x + x", "(x : [2] Int) -> Int");
    ok("\\(x : [3] Int) -> x + x + x", "(x : [3] Int) -> Int");
    // exact, not "at most": Nat's order is discrete
    rejects("\\(x : [2] Int) -> x", "usage mismatch: x has grade 2, but is used once");
    rejects("\\(x : [0] Int) -> x", "usage mismatch: x has grade 0, but is used once");
}

#[test]
fn several_names_share_a_grade() {
    ok("\\(x y : [2] Int) -> x + y + x + y", "(x : [2] Int) -> (y : [2] Int) -> Int");
}

#[test]
fn a_lambda_takes_its_grade_from_the_expected_type() {
    ok("(\\x -> x + x) : (x : [2] Int) -> Int", "(x : [2] Int) -> Int");
    rejects("(\\x -> x) : (x : [2] Int) -> Int", "usage mismatch: x has grade 2, but is used once");
    rejects(
        "(\\(x : [1] Int) -> x) : (x : [2] Int) -> Int",
        "binder grade does not match the expected function type: x is written with grade 1, but the type says 2",
    );
}

#[test]
fn function_types_with_different_grades_are_different_types() {
    rejects(
        "\\(f : (x : [2] Int) -> Int) -> (f : Int -> Int)",
        "type mismatch: expected Int -> Int, but this has type (x : [2] Int) -> Int",
    );
}

#[test]
fn definition_parameters_carry_grades() {
    ok("let dup (n : [2] Int) : Int = n + n in dup 3", "Int");
    rejects("let bad (n : Int) : Int = n + n in bad 3", "usage mismatch: n has grade 1, but is used 2 times");
}

// ---------------------------------------------------------------------------
// Ranges (paper §3: the Interval semiring over Ext Nat)
// ---------------------------------------------------------------------------

#[test]
fn zero_to_one_is_affine() {
    ok("\\(x : [0..1] Int) -> x", "(x : [0..1] Int) -> Int");
    ok("\\(x : [0..1] Int) -> 7", "(x : [0..1] Int) -> Int");
    rejects("\\(x : [0..1] Int) -> x + x", "usage mismatch: x has grade 0..1, but is used 2 times");
}

#[test]
fn one_to_infinity_is_relevant() {
    ok("\\(x : [1..∞] Int) -> x + x + x", "(x : [1..∞] Int) -> Int");
    rejects("\\(x : [1..∞] Int) -> 7", "usage mismatch: x has grade 1..∞, but is used never");
    // `inf` is the ASCII spelling
    ok("\\(x : [1..inf] Int) -> x", "(x : [1..∞] Int) -> Int");
}

#[test]
fn zero_to_infinity_is_unrestricted_and_may_be_left_empty() {
    ok("\\(x : [0..∞] Int) -> 5", "(x : [] Int) -> Int");
    ok("\\(x : [] Int) -> x + x + x + x", "(x : [] Int) -> Int");
    ok("Int []", "Type");
    ok("\\(b : Int []) -> let [x] = b in x + x", "(b : Int []) -> Int");
}

#[test]
fn ranges_scale_like_intervals() {
    // `opt` may or may not use its argument, and is called twice: 0..2
    let opt = "let opt (n : [0..1] Int) : Int = 1 in ";
    ok(&format!("{opt}\\(y : [0..2] Int) -> opt y + opt y"), "(y : [0..2] Int) -> Int");
    rejects(&format!("{opt}\\(y : Int) -> opt y"), "usage mismatch: y has grade 1, but is used 0..1 times");
    // an interval usage fits any interval containing it
    ok(&format!("{opt}\\(y : [0..5] Int) -> opt y"), "(y : [0..5] Int) -> Int");
}

#[test]
fn an_empty_range_is_rejected() {
    rejects(
        "\\(x : [2..1] Int) -> x",
        "empty grade range: 2 is greater than 1, so no number of uses lies between them",
    );
}

#[test]
fn paper_from_maybe_style_affine_default() {
    // fromMaybe : ∀ t. t [0..1] → Maybe t → t   — the default may go unused
    ok("(\\[d] x -> x) : {t : Type} -> t [0..1] -> t -> t", "{t : Type} -> t [0..1] -> t -> t");
    // a box the body never opens has to allow for that
    ok("(\\[d] [x] -> d) : {t : Type} -> t [0..1] -> t [0] -> t", "{t : Type} -> t [0..1] -> t [0] -> t");
}

// ---------------------------------------------------------------------------
// Types are terms, and graded (GrTT)
// ---------------------------------------------------------------------------

#[test]
fn a_term_is_not_charged_for_its_types() {
    // `a` appears in three types and an annotation, and is still used 0 times
    ok("\\(a : [0] Type) (x : a) -> (x : a)", "(a : [0] Type) -> (x : a) -> a");
    // but computing with it counts
    rejects("\\(a : [0] Type) -> a", "usage mismatch: a has grade 0, but is used once");
}

#[test]
fn a_type_is_a_computation_when_it_is_the_result() {
    // returning a type built from `a` uses `a`, exactly like returning `x + 1`
    ok("\\(a : Type) -> a [3]", "(a : Type) -> Type");
    ok("\\(a : [2] Type) -> a -> a", "(a : [2] Type) -> Type");
    rejects("\\(a : Type) -> a -> a", "usage mismatch: a has grade 1, but is used 2 times");
}

#[test]
fn the_second_grade_counts_mentions_in_the_rest_of_the_type() {
    // (a : (0, 2) Type): erased at run time, mentioned exactly twice after
    ok("(\\a x -> x) : (a : (0, 2) Type) -> (x : a) -> a", "(a : (0, 2) Type) -> (x : a) -> a");
    rejects(
        "(\\a x -> x) : (a : (0, 1) Type) -> (x : a) -> a",
        "usage mismatch: a has grade 1 in this type, but the type mentions it 2 times",
    );
    // a binder the type never mentions
    ok("(\\n -> n) : (n : (1, 0) Int) -> Int", "(n : (1, 0) Int) -> Int");
    rejects(
        "\\(f : Int -> Type) -> ((n : (1, 0) Int) -> f n : Type)",
        "usage mismatch: n has grade 0 in this type, but the type mentions it once",
    );
}

#[test]
fn mentions_are_scaled_by_the_functions_they_pass_through() {
    // `f` uses its argument twice, so `f n` mentions `n` twice
    ok(
        "\\(f : (m : [2] Int) -> Type) -> (n : (0, 2) Int) -> f n",
        "(f : (m : [2] Int) -> Type) -> Type",
    );
}

#[test]
fn the_second_grade_is_part_of_the_type() {
    rejects(
        "\\(f : (n : (1, 0) Int) -> Int) -> (f : Int -> Int)",
        "type mismatch: expected Int -> Int, but this has type (n : (1, 0) Int) -> Int",
    );
}

#[test]
fn an_inferred_lambda_cannot_promise_a_second_grade() {
    rejects(
        "\\(n : (1, 0) Int) -> n",
        "a type-level grade needs a function type to check it against: the type of this function is inferred, so nothing holds it to n's second grade",
    );
}

#[test]
fn arguments_to_erased_parameters_are_erased() {
    // `y` is passed only to a grade-0 parameter, so it is used 0 · 1 = 0 times
    ok("let k (n : [0] Int) : Int = 1 in \\(y : [0] Int) -> k y", "(y : [0] Int) -> Int");
}

// ---------------------------------------------------------------------------
// Application scales (the graded `app` rule)
// ---------------------------------------------------------------------------

#[test]
fn a_function_that_uses_its_argument_twice_uses_the_arguments_variables_twice() {
    let dup = "let dup (n : [2] Int) : Int = n + n in ";
    ok(&format!("{dup}\\(y : [2] Int) -> dup y"), "(y : [2] Int) -> Int");
    rejects(&format!("{dup}\\(y : Int) -> dup y"), "usage mismatch: y has grade 1, but is used 2 times");
    // 2 · (1 + 1) = 4
    ok(&format!("{dup}\\(y : [4] Int) -> dup (y + y)"), "(y : [4] Int) -> Int");
}

#[test]
fn higher_order_arguments_are_linear_unless_graded() {
    rejects(
        "(\\f x -> f (f x)) : {a : Type} -> (a -> a) -> a -> a",
        "usage mismatch: f has grade 1, but is used 2 times",
    );
    ok(
        "(\\[f] x -> f (f x)) : {a : Type} -> (a -> a) [2] -> a -> a",
        "{a : Type} -> (a -> a) [2] -> a -> a",
    );
}

// ---------------------------------------------------------------------------
// let
// ---------------------------------------------------------------------------

#[test]
fn a_let_charges_its_right_hand_side_once_per_use() {
    ok("\\(x : [2] Int) -> let y = x in y + y", "(x : [2] Int) -> Int");
    rejects("\\(x : Int) -> let y = x in y + y", "usage mismatch: x has grade 1, but is used 2 times");
    // an unused definition costs nothing: 0 · 1
    ok("\\(x : [0] Int) -> let y = x in 1", "(x : [0] Int) -> Int");
}

#[test]
fn closed_definitions_can_be_used_freely() {
    ok("let one = 1 in one + one + one", "Int");
    ok("let id : {a : Type} -> a -> a; id x = x in id (id 3)", "Int");
}

#[test]
fn a_recursive_definition_uses_what_it_captures_without_restriction() {
    rejects(
        "\\(x : Int) -> let loop : Int -> Int ! Div = \\n -> loop (n + x) in loop 1",
        "usage mismatch: x has grade 1, but is used an unrestricted number of times",
    );
    ok("\\(x : [] Int) -> let loop : Int -> Int ! Div = \\n -> loop (n + x) in loop 1", "(x : [] Int) -> Int ! Div");
}

// ---------------------------------------------------------------------------
// The graded modality (paper §2.2, §3)
// ---------------------------------------------------------------------------

#[test]
fn the_modality_is_postfix() {
    ok("Int [2]", "Type");
    ok("Int [0] []", "Type");
    ok("\\(f : Type -> Type) -> f Int [2]", "(f : Type -> Type) -> Type");
    ok("\\(b : (Int -> Int) [2]) -> b", "(b : (Int -> Int) [2]) -> (Int -> Int) [2]");
}

#[test]
fn brackets_after_a_function_are_still_an_argument() {
    // what `f [x]` means is settled by the type of `f`
    ok("\\(f : Int [2] -> Int) (x : [2] Int) -> f [x]", "(f : Int [2] -> Int) -> (x : [2] Int) -> Int");
}

#[test]
fn paper_drop_and_copy() {
    // drop : t [0] -> ()      copy uses its argument twice
    ok("(\\[x] -> ()) : {a : Type} -> a [0] -> Unit", "{a : Type} -> a [0] -> ()");
    ok("\\(b : Int [2]) -> let [x] = b in x + x", "(b : Int [2]) -> Int");
    // the grade is exact
    rejects("\\(b : Int [2]) -> let [x] = b in x", "usage mismatch: x has grade 2, but is used once");
    // and the box itself is still a linear value: it must be unboxed
    rejects("\\(b : Int [0]) -> 1", "usage mismatch: b has grade 1, but is used never");
}

#[test]
fn paper_foo_promotion_scales_the_context() {
    // foo : Int [3] -> Int [6] -> Int [3]
    // foo = \x' -> \y' -> let [x] = x' in let [y] = y' in [x+y+y]
    let foo = |y_grade: u32| {
        format!(
            "(\\x' -> \\y' -> let [x] = x' in let [y] = y' in [x + y + y]) \
             : Int [3] -> Int [{y_grade}] -> Int [3]"
        )
    };
    ok(&foo(6), "Int [3] -> Int [6] -> Int [3]");
    rejects(&foo(5), "usage mismatch: y has grade 5, but is used 6 times");
}

#[test]
fn unboxing_patterns() {
    ok("(\\[x] [y] -> [x + y + y]) : Int [3] -> Int [6] -> Int [3]", "Int [3] -> Int [6] -> Int [3]");
    // with no signature, the box's grade is whatever the body needs
    ok("let copy [x] : Int = x + x in copy", "(x : Int [2]) -> Int");
}

#[test]
fn promotion_of_closed_terms_is_free_at_any_grade() {
    ok("[3] : Int [7]", "Int [7]");
    ok("[29] : Int []", "Int []");
}

#[test]
fn a_promotions_grade_is_inferred_from_how_it_is_used() {
    ok("let [x] = [3] in x + x", "Int");
    ok("\\(y : [2] Int) -> let [x] = [y] in x + x", "(y : [2] Int) -> Int");
    rejects("[3]", "unsolved metavariable: could not infer the grade of this box");
}

#[test]
fn only_boxes_can_be_unboxed() {
    rejects("let [x] = 3 in x", "this is not a box: it has type Int, so it cannot be unboxed");
}

#[test]
fn box_grades_are_part_of_the_type() {
    rejects("\\(b : Int [2]) -> (b : Int [3])", "type mismatch: expected Int [3], but this has type Int [2]");
    // no approximation: an exact 1 is not accepted as a 0..1
    rejects(
        "\\(b : Int [1]) -> (b : Int [0..1])",
        "type mismatch: expected Int [0..1], but this has type Int [1]",
    );
}

// ---------------------------------------------------------------------------
// Grade polymorphism
// ---------------------------------------------------------------------------

#[test]
fn grades_are_terms_of_type_grade() {
    ok("Grade", "Type");
    ok("let two : Grade = 1 + 1 in \\(x : [two] Int) -> x + x", "(x : [2] Int) -> Int");
    ok("(0..1 : Grade)", "Grade");
    ok("(2 * 3 + 1 : Grade)", "Grade");
    // elsewhere a numeral is still an Int
    ok("2 * 3 + 1", "Int");
}

#[test]
fn a_function_can_be_polymorphic_in_a_grade() {
    ok(
        "(\\f [x] -> f [x]) : {a b : Type} -> {r : Grade} -> (a [r] -> b) -> a [r] -> b",
        "{a : Type} -> {b : Type} -> {r : Grade} -> (a [r] -> b) -> a [r] -> b",
    );
}

#[test]
fn a_grade_variable_is_instantiated_at_each_use() {
    let app = "let app : {a b : Type} -> {r : Grade} -> (a [r] -> b) -> a [r] -> b; app f [x] = f [x] in ";
    ok(
        &format!("{app}\\(g : Int [2] -> Int) (y : [2] Int) -> app g [y]"),
        "(g : Int [2] -> Int) -> (y : [2] Int) -> Int",
    );
    rejects(
        &format!("{app}\\(g : Int [2] -> Int) (y : Int) -> app g [y]"),
        "usage mismatch: y has grade 1, but is used 2 times",
    );
}

#[test]
fn symbolic_grades_are_compared_up_to_the_semiring_laws() {
    ok(
        "(\\b -> b) : {r : Grade} -> Int [r + r] -> Int [2 * r]",
        "{r : Grade} -> Int [2 * r] -> Int [2 * r]",
    );
    ok(
        "(\\b -> b) : {r s : Grade} -> Int [r * s + 1] -> Int [1 + s * r]",
        "{r : Grade} -> {s : Grade} -> Int [1 + r * s] -> Int [1 + r * s]",
    );
    rejects(
        "(\\b -> b) : {r : Grade} -> Int [r] -> Int [r + 1]",
        "type mismatch: expected Int [1 + r], but this has type Int [r]",
    );
}

#[test]
fn symbolic_usage_is_checked_monomial_by_monomial() {
    // x is used r + r times
    ok(
        "(\\[f] [x] -> f [x] + f [x]) : {r : Grade} -> (Int [r] -> Int) [2] -> Int [2 * r] -> Int",
        "{r : Grade} -> (Int [r] -> Int) [2] -> Int [2 * r] -> Int",
    );
    rejects(
        "(\\[f] [x] -> f [x] + f [x]) : {r : Grade} -> (Int [r] -> Int) [2] -> Int [r] -> Int",
        "usage mismatch: x has grade r, but is used 2 * r times",
    );
    // a coefficient may fit without being equal: r + 0 fits r + 0..1
    ok(
        "(\\f [x] -> f [x]) : {r : Grade} -> (Int [r] -> Int) -> Int [r + (0..1)] -> Int",
        "{r : Grade} -> (Int [r] -> Int) -> Int [(0..1) + r] -> Int",
    );
}

#[test]
fn an_explicit_grade_argument() {
    ok("let rep : (r : [0] Grade) -> Int [r] -> Int [r]; rep r b = b in rep 3", "Int [3] -> Int [3]");
}

// ---------------------------------------------------------------------------
// Boxes and dependency
// ---------------------------------------------------------------------------

#[test]
fn unboxing_a_literal_box_computes() {
    // n is erased (grade 0) and used only in a type, where it reduces to 2
    ok(
        "\\(f : [0] Int -> Type) (p : f 4) -> let [n] = ([2] : Int [0]) in (p : f (n + 2))",
        "(f : [0] Int -> Type) -> (p : f 4) -> f 4",
    );
}

#[test]
fn a_type_may_mention_an_unboxed_variable_without_it_escaping() {
    // the result type depends on `n`, which is `unbox b`
    ok(
        "\\(f : [0] Int -> Type) (b : Int [0]) (g : (n : [0] Int) -> f n) -> let [n] = b in g n",
        "(f : [0] Int -> Type) -> (b : Int [0]) -> (g : (n : [0] Int) -> f n) -> f (unbox b)",
    );
}

// ---------------------------------------------------------------------------
// Regression: `let` is a real binder in Core
// ---------------------------------------------------------------------------

#[test]
fn a_nested_let_evaluates_to_the_right_thing() {
    // Before `Core::Let`, the inner body was evaluated against the wrong
    // context slot and `y` came out as `p`.
    ok(
        "\\(f : [0] Int -> Type) (p : f 1) -> let y = (let x = 1 in x) in (p : f y)",
        "(f : [0] Int -> Type) -> (p : f 1) -> f 1",
    );
}

#[test]
fn a_let_under_a_lambda_survives_being_applied() {
    ok(
        "let g : Int -> Int = \\n -> let m = n + 1 in m \
         in \\(f : [0] Int -> Type) (p : f 3) -> (p : f (g 2))",
        "(f : [0] Int -> Type) -> (p : f 3) -> f 3",
    );
}

// ---------------------------------------------------------------------------
// Syntax
// ---------------------------------------------------------------------------

#[test]
fn an_oversized_grade_is_rejected() {
    rejects(
        "\\(x : [99999999999999999999999] Int) -> x",
        // (a grade is written as an `Int` is, and caught as one)
        "parse error: 99999999999999999999999 does not fit in an Int, which is 64 bits",
    );
}

#[test]
fn the_old_prefix_forms_are_gone() {
    rejects("Box", "name not in scope: Box: used here");
    let (ty, diags) = run("\\(0 x : Int) -> x");
    assert_eq!(ty, "NONE");
    assert!(diags[0].starts_with("parse error"), "{diags:#?}");
}

/// What goes in a box is a value: a constructor with an implicit argument,
/// boxed where nothing yet says its type (inside a pair, whose parts are
/// inferred), is given that argument before it is boxed. It was boxed as
/// the function of a type it is, which no box may hold ("universe too
/// large").
#[test]
fn a_box_holds_a_value_its_implicits_given() {
    ok("x : (Maybe Int [], Int)\nx = ([Nothing], 1)\n\nx", "(Maybe Int [], Int)");
    ok("x : (List Bool [0..1], Int)\nx = ([Nil], 1)\n\nx", "(List Bool [0..1], Int)");
}
