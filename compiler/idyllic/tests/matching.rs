//! Dependent pattern matching (phase 1 of the roadmap): definitions by
//! equations, nested and literal patterns, guards, indexed families,
//! refinement, coverage, and recursion that terminates and so computes in
//! types.

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

fn accepts(src: &str) {
    let out = run(src);
    assert!(out.diagnostics.is_empty(), "{src}\nunexpected diagnostics: {:#?}", diags(&out));
}

fn rejects(src: &str, expected: &[&str]) {
    assert_eq!(diags(&run(src)), expected, "{src}");
}

const NAT: &str = "data Nat = Z | S Nat\n\nuse Nat.*\n\n";

const PLUS: &str = "(+) : Nat -> Nat -> Nat\nZ + m = m\nS n + m = S (n + m)\n\n";

const VEC: &str = "data Vec : Nat -> Type -> Type where\n  Nil : Vec Z a\n  Cons : a -> Vec n a -> Vec (S n) a\n\nuse Vec.*\n\n";

// ---------------------------------------------------------------------------
// What phase 1 was for
// ---------------------------------------------------------------------------

#[test]
fn append_adds_the_lengths_with_a_plus_of_its_own() {
    let src = format!(
        "{NAT}{PLUS}{VEC}\
         append : Vec n a -> Vec m a -> Vec (n + m) a\n\
         append Nil ys = ys\n\
         append (Cons x xs) ys = Cons x (append xs ys)\n\n\
         append (Cons 1 Nil) (Cons 2 (Cons 3 Nil))\n"
    );
    let out = run(&src);
    assert!(out.diagnostics.is_empty(), "{:#?}", diags(&out));
    assert_eq!(out.body_type.as_deref(), Some("Vec (S (S (S Z))) Int"));
}

#[test]
fn head_needs_no_case_for_the_empty_vector() {
    let src = format!(
        "{NAT}{VEC}\
         head : (v : [0..1] Vec (S n) a) -> a\n\
         head (Cons x xs) = x\n\n\
         head (Cons 7 Nil)\n"
    );
    assert_eq!(result(&src), "7 : Int");
}

#[test]
fn eulers_fifth_problem_runs() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/euler/p5.idl")).unwrap();
    assert_eq!(result(&src), "232792560 : Int");
}

// ---------------------------------------------------------------------------
// Everyday matching
// ---------------------------------------------------------------------------

#[test]
fn several_equations_with_literal_patterns() {
    let src = "fact : (n : [] Int) -> Int ! Div\nfact 0 = 1\nfact n = n * fact (n - 1)\n\nfact 5\n";
    assert_eq!(result(src), "120 : Int");
}

#[test]
fn guards_fall_through_to_the_next_one() {
    let src = "classify : (n : [] Int) -> Int\n\
               classify n\n  | n < 0 = 0\n  | n == 0 = 1\n  | otherwise = 2\n\n\
               classify 0 + classify 5 * 10 + classify (0 - 3) * 100\n";
    assert_eq!(result(src), "21 : Int");
}

#[test]
fn a_guard_that_fails_falls_through_to_the_next_equation() {
    let src = "f : (n : [] Int) -> Int\nf n | n < 10 = 1\nf n = 2\n\nf 3 + f 30 * 10\n";
    assert_eq!(result(src), "21 : Int");
}

#[test]
fn nested_wildcard_and_cons_patterns() {
    let src = "second : (xs : [] List Int) -> Int\n\
               second (_ :: y :: _) = y\n\
               second _ = 0\n\n\
               second (1 :: 2 :: 3 :: Nil) + second (1 :: Nil)\n";
    assert_eq!(result(src), "2 : Int");
}

#[test]
fn as_patterns_name_the_whole() {
    let src = format!(
        "{NAT}{VEC}\
         dup : (xs : [] Vec (S n) a) -> Vec (S (S n)) a\n\
         dup all@(Cons x rest) = Cons x all\n\n\
         dup (Cons 1 Nil)\n"
    );
    let out = run(&src);
    assert!(out.diagnostics.is_empty(), "{:#?}", diags(&out));
    assert_eq!(out.body_type.as_deref(), Some("Vec (S (S Z)) Int"));
}

#[test]
fn a_case_takes_patterns_too() {
    let src = "f : (xs : [] List Int) -> Int\n\
               f xs = case xs of\n  Nil -> 0\n  x :: Nil | x < 0 -> 1\n  x :: _ -> 2\n\n\
               f Nil + f (0 - 1 :: Nil) * 10 + f (5 :: 6 :: Nil) * 100\n";
    assert_eq!(result(src), "210 : Int");
}

#[test]
fn a_lambda_can_ignore_its_argument() {
    assert_eq!(result("(\\(_ : [0] Int) -> 1) 2"), "1 : Int");
}

#[test]
fn division_and_remainder() {
    assert_eq!(result("17 / 5 * 10 + 17 % 5"), "32 : Int");
    let out = run("1 / 0");
    assert!(out.diagnostics.is_empty());
    assert_eq!(out.evaluation, Some(Evaluation::Stopped("division by zero".into())));
}

#[test]
fn an_operator_can_be_defined_by_equations() {
    let src = format!("{NAT}{PLUS}S Z + S (S Z)\n");
    assert_eq!(result(&src), "S (S (S Z)) : Nat");
}

#[test]
fn a_program_may_shadow_the_prelude() {
    let src = format!("{NAT}{VEC}Nil\n");
    let out = run(&src);
    assert!(out.diagnostics.is_empty(), "{:#?}", diags(&out));
    assert_eq!(out.body_type.as_deref(), Some("{a : Type} -> Vec Z a"));
}

// ---------------------------------------------------------------------------
// Coverage
// ---------------------------------------------------------------------------

#[test]
fn a_missing_case_is_reported_as_a_pattern() {
    rejects(
        &format!("{NAT}pred : Nat -> Nat\npred (S n) = n\n"),
        &["the patterns do not cover every case: missing: Z"],
    );
    rejects(
        "data C = R | G | B\n\nuse C.*\n\nboth : C -> C -> Int\nboth R R = 1\nboth R _ = 2\nboth G B = 3\n",
        &["the patterns do not cover every case: missing: G R, G G, B _"],
    );
    // a guard can fail, so it does not cover its case
    rejects(
        "f : (n : [] Int) -> Int\nf n | n < 0 = 1\n",
        &["the patterns do not cover every case: missing: _"],
    );
}

#[test]
fn an_equation_nothing_reaches_is_reported() {
    rejects(
        &format!("{NAT}f : (n : [0..1] Nat) -> Int\nf _ = 1\nf Z = 2\n"),
        &["unreachable: the cases before this one cover everything it matches"],
    );
}

#[test]
fn equations_agree_on_how_many_arguments() {
    rejects(
        &format!("{NAT}f : (n : [0..1] Nat) -> Int\nf Z = 1\nf n m = 2\n"),
        &["equations with different numbers of arguments: this one has 2, but the first has 1"],
    );
    rejects("f : Int -> Int\nf 1 2 = 3\n", &["too many patterns: what is left to match has type Int, which is not a function"]);
}

#[test]
fn strings_are_not_patterns_yet() {
    rejects(
        "f : String -> Int\nf \"a\" = 1\nf _ = 2\n",
        &["strings cannot be matched on yet: use a guard with a comparison instead"],
    );
}

// ---------------------------------------------------------------------------
// Dependent matching
// ---------------------------------------------------------------------------

const EQ: &str = "data Eq : Nat -> Nat -> Type where\n  Refl : Eq x x\n\nuse Eq.*\n\n";

#[test]
fn matching_refl_teaches_the_types_an_equation() {
    accepts(&format!(
        "{NAT}{PLUS}{EQ}\
         sym : Eq x y -> Eq y x\nsym Refl = Refl\n\n\
         cong : Eq x y -> Eq (S x) (S y)\ncong Refl = Refl\n\n\
         plusZero : (n : Nat) -> Eq (n + Z) n\n\
         plusZero Z = Refl\n\
         plusZero (S k) = cong (plusZero k)\n"
    ));
}

#[test]
fn impossible_cases_can_be_left_out_or_said_so() {
    let fin = "data Fin : Nat -> Type where\n  FZ : Fin (S n)\n  FS : Fin n -> Fin (S n)\n\nuse Fin.*\n\n";
    accepts(&format!("{NAT}{fin}noFin : Fin Z -> Int\nnoFin x impossible\n"));
    accepts("data Void\n\nuse Void.*\n\nabsurd : Void -> Int\nabsurd v impossible\n");
    rejects(
        &format!("{NAT}f : Nat -> Int\nf n impossible\n"),
        &["not impossible: some values match these patterns"],
    );
}

#[test]
fn a_dot_pattern_is_what_matching_worked_out() {
    let same = "same : (n : [] Nat) -> (m : [0] Nat) -> (p : [0..1] Eq n m) -> Nat\n";
    accepts(&format!("{NAT}{EQ}{same}same n .n Refl = n\n"));
    rejects(
        &format!("{NAT}{EQ}{same}same n .Z Refl = n\n"),
        &["dot pattern does not match: matching has worked out that this is n, not Z"],
    );
}

#[test]
fn a_case_on_a_variable_refines_the_goal() {
    accepts(&format!(
        "{NAT}{VEC}tail : (xs : [0..1] Vec (S n) a) -> Vec n a\ntail xs = case xs of\n  Cons y ys -> ys\n"
    ));
}

#[test]
fn matching_cannot_decide_what_does_not_compute() {
    let src = format!(
        "{NAT}{VEC}\
         f : Nat -> Nat\nf n = f n\n\n\
         g : Vec (f Z) Int -> Int\ng Nil = 0\ng (Cons x xs) = 1\n"
    );
    let out = run(&src);
    assert!(diags(&out).iter().any(|d| d.starts_with("cannot tell whether this pattern applies")), "{:#?}", diags(&out));
}

#[test]
fn fields_have_the_grade_of_what_they_came_out_of() {
    rejects(
        &format!("@linear\n\n{NAT}isZero : Nat -> Bool\nisZero Z = True\nisZero (S n) = False\n"),
        &["usage mismatch: n has grade 1, but is used never"],
    );
    accepts(&format!("{NAT}isZero : (m : [0..1] Nat) -> Bool\nisZero Z = True\nisZero (S n) = False\n"));
    // and an erased value cannot be looked at
    rejects(
        &format!("{NAT}f : (m : [0] Nat) -> Int\nf Z = 0\nf (S n) = 1\n"),
        &["usage mismatch: m has grade 0, but is used once"],
    );
}

// ---------------------------------------------------------------------------
// Indexed families
// ---------------------------------------------------------------------------

#[test]
fn a_constructor_builds_its_own_type() {
    rejects(
        &format!("{NAT}data T : Nat -> Type where\n  C : Nat -> Nat\n"),
        &["C does not build a T: a constructor's type ends in T, applied to its parameters and then its indices"],
    );
    rejects(
        &format!("{NAT}data T (a : Type) : Nat -> Type where\n  C : T Int Z\n"),
        &["C does not build a T: a constructor's type ends in T, applied to its parameters and then its indices"],
    );
}

#[test]
fn a_kind_ends_in_type() {
    rejects(
        &format!("{NAT}data T : Nat -> Nat where\n"),
        &["a data type's kind ends in Type: this is Nat"],
    );
}

#[test]
fn indices_need_constructor_types() {
    rejects(
        &format!("{NAT}data T : Nat -> Type = C\n"),
        &["T has indices, so its constructors need types: declare them with where, as in  C : .. -> T i"],
    );
}

#[test]
fn only_a_forced_argument_may_be_large() {
    // `a` is one of `Vec`'s indices, so the type says what it is
    accepts(&format!("{NAT}{VEC}"));
    // but a type hidden inside a value is a set of all sets
    rejects(
        "data Box : Type where\n  Box : {a : Type} -> a -> Box\n",
        &["field too large for its data type: this field's type lives in Type 1, but Box lives in Type"],
    );
}

// ---------------------------------------------------------------------------
// Termination
// ---------------------------------------------------------------------------

#[test]
fn recursion_that_terminates_computes_in_types() {
    let even = "even : (n : [0..1] Nat) -> Bool\neven Z = True\neven (S n) = odd n\n\n\
                odd : (n : [0..1] Nat) -> Bool\nodd Z = False\nodd (S n) = even n\n\n";
    let beq = "data BEq : Bool -> Bool -> Type where\n  BRefl : BEq x x\n\nuse BEq.*\n\n";
    accepts(&format!("{NAT}{even}{beq}t : BEq (even (S (S Z))) True\nt = BRefl\n"));
    // Ackermann's function is not structural in any one argument
    let ack = "ack : (m : [] Nat) -> (n : [] Nat) -> Nat\n\
               ack Z n = S n\n\
               ack (S m) Z = ack m (S Z)\n\
               ack (S m) (S n) = ack m (ack (S m) n)\n\n";
    accepts(&format!("{NAT}{EQ}{ack}t : Eq (ack (S Z) (S Z)) (S (S (S Z)))\nt = Refl\n"));
}

#[test]
fn recursion_that_may_not_terminate_has_to_say_so_and_stays_out_of_types() {
    rejects(
        &format!("{NAT}loop : Nat -> Nat\nloop n = loop n\n"),
        &["loop may not terminate: no argument gets smaller on every round of its recursion, so it has the effect Div"],
    );
    let src = format!("{NAT}{EQ}loop : Nat -> Nat ! Div\nloop n = loop n\n\nt : Eq (loop Z) Z\nt = Refl\n");
    rejects(&src, &["a type must be pure: computing this type has the effects Div"]);
}

#[test]
fn a_case_on_a_stuck_comparison_is_stuck_not_anything() {
    // it used to evaluate to the error value, which unifies with anything
    let src = "data IEq : Int -> Int -> Type where\n  IRefl : IEq x x\n\nuse IEq.*\n\n\
               \\(n : [0] Int) -> (IRefl : IEq (if n < 2 then 0 else 1) 5)\n";
    assert!(diags(&run(src)).iter().any(|d| d.starts_with("type mismatch")), "{:?}", diags(&run(src)));
}
