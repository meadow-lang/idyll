//! Top-level declarations, Granule-style: a signature `f : a` and a defining
//! equation `f p1 p2 = e` with patterns; the variables of a signature's
//! implicit binders, `{a : Type} -> ..`, scope over the equation.

use idyllic_diagnostics::OutputStyle;
use idyllic::pipeline::{self, Options};

struct Checked {
    /// `name : type` for each definition, in source order
    decls: Vec<String>,
    body: Option<String>,
    /// `headline: label`
    diags: Vec<String>,
}

fn run(src: &str) -> Checked {
    let out = pipeline::run(src, &Options { debug: false, style: OutputStyle::PLAIN, live: false });
    let diags = out
        .diagnostics
        .iter()
        .map(|d| match d.labels.first() {
            Some(l) => format!("{}: {}", d.message, l.message),
            None => d.message.clone(),
        })
        .collect();
    Checked { decls: out.decls.iter().map(|(x, ty)| format!("{x} : {ty}")).collect(), body: out.body_type, diags }
}

fn ok(src: &str, decls: &[&str], body: Option<&str>) {
    let got = run(src);
    assert!(got.diags.is_empty(), "{src}\nunexpected diagnostics: {:#?}", got.diags);
    assert_eq!(got.decls, decls, "{src}");
    assert_eq!(got.body.as_deref(), body, "{src}");
}

fn rejects(src: &str, expected: &[&str]) {
    assert_eq!(run(src).diags, expected, "{src}");
}

// ---------------------------------------------------------------------------
// Signatures and equations
// ---------------------------------------------------------------------------

#[test]
fn a_signature_and_an_equation() {
    ok("id : {a : Type} -> a -> a\nid x = x\n", &["id : {a : Type} -> a -> a"], None);
}

#[test]
fn a_program_may_end_in_an_expression() {
    ok("id : {a : Type} -> a -> a\nid x = x\n\nid 3\n", &["id : {a : Type} -> a -> a"], Some("Int"));
}

#[test]
fn a_signature_is_optional_when_the_type_can_be_inferred() {
    ok("one = 1\nsucc (n : Int) = n + one\n", &["one : Int", "succ : (n : Int) -> Int"], None);
    // and can be written inline, as in a `let`
    ok("one : Int = 1\n", &["one : Int"], None);
}

#[test]
fn a_signature_may_come_after_its_equation_or_far_from_it() {
    ok("f x = x\ng = f 1\nf : Int -> Int\n", &["f : Int -> Int", "g : Int"], None);
}

#[test]
fn a_signature_may_run_over_several_lines() {
    ok(
        "flip : {a b c : Type}\n    -> (a -> b -> c)\n    -> b -> a -> c\nflip f y x = f x y\n",
        &["flip : {a : Type} -> {b : Type} -> {c : Type} -> (a -> b -> c) -> b -> a -> c"],
        None,
    );
}

#[test]
fn comments_and_blank_lines_mean_nothing() {
    // a comment-only line used to glue the next declaration onto the last
    ok(
        "-- the identity\nid : {a : Type} -> a -> a\nid x = x   -- linear\n\n  -- indented comment\n   \none = id 1\n\n",
        &["id : {a : Type} -> a -> a", "one : Int"],
        None,
    );
    // and a blank line used to end a `let` block
    ok("let x = 1\n\n    -- two\n    y = 2\n in x + y\n", &["x : Int", "y : Int"], Some("Int"));
}

// ---------------------------------------------------------------------------
// Order and recursion
// ---------------------------------------------------------------------------

#[test]
fn definitions_may_be_used_before_they_are_made() {
    ok("two = one + one\none = 1\n", &["two : Int", "one : Int"], None);
}

#[test]
fn recursion_needs_a_signature() {
    ok("loop : Int -> Int ! Div\nloop n = loop (n + 1)\n", &["loop : Int -> Int ! Div"], None);
    // and a recursion that may not end says so
    rejects(
        "loop : Int -> Int\nloop n = loop (n + 1)\n",
        &["loop may not terminate: no argument gets smaller on every round of its recursion, so it has the effect Div"],
    );
    // just the one error: with no signature there is nothing to check the
    // body against, so it is not elaborated into a pile of unsolved metas
    rejects(
        "loop n = loop (n + 1)\n",
        &["the recursive binding loop needs a type signature: its type cannot be inferred from a recursive definition"],
    );
}

#[test]
fn mutual_recursion() {
    ok(
        "ping : Int -> Int ! Div\nping n = pong (n + 1)\npong : Int -> Int ! Div\npong n = ping n\n",
        &["ping : Int -> Int ! Div", "pong : Int -> Int ! Div"],
        None,
    );
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

#[test]
fn equations_take_granules_unboxing_patterns() {
    ok(
        "drop : {a : Type} -> a [0] -> ()\ndrop [x] = ()\n\ncopy : Int [2] -> Int\ncopy [x] = x + x\n",
        &["drop : {a : Type} -> a [0] -> ()", "copy : Int [2] -> Int"],
        None,
    );
    rejects("copy : Int [2] -> Int\ncopy [x] = x\n", &["usage mismatch: x has grade 2, but is used once"]);
}

#[test]
fn equations_take_annotated_and_graded_patterns() {
    ok("dup (n : [2] Int) : Int = n + n\n", &["dup : (n : [2] Int) -> Int"], None);
    rejects("@linear\n\ndup (n : Int) : Int = n + n\n", &["usage mismatch: n has grade 1, but is used 2 times"]);
    // which is a linear module's default; elsewhere `n` may be used freely
    ok("dup (n : Int) : Int = n + n\n", &["dup : (n : Int) -> Int"], None);
}

// ---------------------------------------------------------------------------
// Implicit binders
// ---------------------------------------------------------------------------

#[test]
fn an_implicit_binder_is_an_ordinary_binder_in_braces() {
    // braces and parentheses mix in one telescope; several names share a pair
    // (quantifying over types puts it a universe up)
    ok("{a b : Type} {r : Grade} (f : a [r] -> b) -> a [r] -> b\n", &[], Some("Type 1"));
    ok(
        "@linear\n\n(\\f [x] -> f [x]) : {a b : Type} {r : Grade} (f : a [r] -> b) -> a [r] -> b\n",
        &[],
        Some("{a : Type} -> {b : Type} -> {r : Grade} -> (f : a [r] -> b) -> a [r] -> b"),
    );
    // erased unless it says otherwise, and then the grade is shown
    ok("{n : [1] Int} -> Int\n", &[], Some("Type"));
    ok("(\\(x : Int) -> x) : {n : [0..1] Int} -> Int -> Int\n", &[], Some("{n : [0..1] Int} -> Int -> Int"));
    ok("{a : (0, 2) Type} -> a -> a\n", &[], Some("Type 1"));
    rejects(
        "{a : (0, 1) Type} -> a -> a\n",
        &["usage mismatch: a has grade 1 in this type, but the type mentions it 2 times"],
    );
}

#[test]
fn forall_is_gone_and_is_just_a_name() {
    ok("let forall = 1 in forall + 1\n", &["forall : Int"], Some("Int"));
    assert!(run("forall {a : Type} . a -> a\n").diags[0].starts_with("parse error"));
}

#[test]
fn explicit_let_blocks_still_use_braces() {
    // `{` after `let` opens a block of declarations, not a binder
    ok("let { x = 1; y = 2 } in x + y\n", &["x : Int", "y : Int"], Some("Int"));
    ok("let { id : {a : Type} -> a -> a; id x = x } in id 1\n", &["id : {a : Type} -> a -> a"], Some("Int"));
}

#[test]
fn quantified_variables_scope_over_the_equation() {
    ok("id : {a : Type} -> a -> a\nid x = (x : a)\n", &["id : {a : Type} -> a -> a"], None);
    // they are erased, so the equation cannot compute with them
    rejects(
        "ty : {a : Type} -> () [0] -> Type\nty [u] = a\n",
        &["usage mismatch: a has grade 0, but is used once"],
    );
    // and a pattern may shadow one
    ok("k : {a : Type} -> a -> a\nk a = a\n", &["k : {a : Type} -> a -> a"], None);
}

#[test]
fn implicit_arguments_are_inferred_at_each_use() {
    let prelude = "id : {a : Type} -> a -> a\nid x = x\n\n";
    ok(&format!("{prelude}id (id True)\n"), &["id : {a : Type} -> a -> a"], Some("Bool"));
    // a bare name keeps its implicit binder
    ok(&format!("{prelude}id\n"), &["id : {a : Type} -> a -> a"], Some("{a : Type} -> a -> a"));
    // and instantiates where a particular type is wanted
    ok(&format!("{prelude}(id : Int -> Int)\n"), &["id : {a : Type} -> a -> a"], Some("Int -> Int"));
}

#[test]
fn a_polymorphic_argument_stays_polymorphic_when_nothing_says_otherwise() {
    // An implicit argument is not filled in while the expected type is still
    // unknown, so `(\x -> x) id` is `id` itself.
    let prelude = "id : {a : Type} -> a -> a\nid x = x\n\n";
    let id_ty = "{a : Type} -> a -> a";
    let sig = format!("id : {id_ty}");
    ok(&format!("{prelude}id id id 2\n"), &[&sig], Some("Int"));
    ok(&format!("{prelude}(\\x -> x) id\n"), &[&sig], Some(id_ty));
    // the result really is polymorphic: here it is used at two types
    ok(
        &format!("@linear\n\n{prelude}let [f] = ([id id] : ({{a : Type}} -> a -> a) [2]) in f (f True)\n"),
        &[&sig],
        Some("Bool"),
    );
    // and a known expected type still instantiates, as before
    ok(&format!("{prelude}(id id : Int -> Int)\n"), &[&sig], Some("Int -> Int"));
    // But `id id` cannot give the outer `id` the inner one whole: `a` would
    // be `{a : Type} -> a -> a`, which is in `Type 1`, not `Type`.  So the
    // inner one is instantiated, and nothing says at what.
    rejects(
        &format!("{prelude}id id\n"),
        &["unsolved metavariable: could not infer the implicit argument a"],
    );
}

#[test]
fn a_polymorphic_definition_can_be_passed_where_one_is_wanted() {
    ok(
        "id : {a : Type} -> a -> a\nid x = x\n\napply : ({a : Type} -> a -> a) -> Int\napply f = f 1\n\napply id\n",
        &["id : {a : Type} -> a -> a", "apply : ({a : Type} -> a -> a) -> Int"],
        Some("Int"),
    );
}

#[test]
fn an_implicit_argument_nothing_determines_is_reported() {
    rejects(
        "phantom : {a : Type} -> Int -> Int\nphantom n = n\n\nphantom 1\n",
        &["unsolved metavariable: could not infer the implicit argument a"],
    );
}

#[test]
fn grade_polymorphic_declarations() {
    ok(
        "@linear\n\napp : {a b : Type} -> {r : Grade} -> (a [r] -> b) -> a [r] -> b\napp f [x] = f [x]\n\n\
         copy : Int [2] -> Int\ncopy [x] = x + x\n\napp copy [4]\n",
        &["app : {a : Type} -> {b : Type} -> {r : Grade} -> (a [r] -> b) -> a [r] -> b", "copy : Int [2] -> Int"],
        Some("Int"),
    );
}

// ---------------------------------------------------------------------------
// let blocks hold the same declarations
// ---------------------------------------------------------------------------

#[test]
fn a_let_block_is_a_block_of_declarations() {
    ok(
        "let id : {a : Type} -> a -> a\n    id x = x\n    n = id 2\n in id n\n",
        &["id : {a : Type} -> a -> a", "n : Int"],
        Some("Int"),
    );
    // nested, they are not the program's declarations
    ok("\\(u : Unit) -> let one : Int; one = 1 in u\n", &[], Some("(u : ()) -> ()"));
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[test]
fn a_signature_needs_a_definition() {
    rejects("f : Int -> Int\n", &["the signature for f has no definition: declared here, but never defined"]);
}

#[test]
fn duplicates_are_reported() {
    rejects("x = 1\nx = 2\n", &["duplicate binding of x: bound again here"]);
    rejects("x : Int\nx : Int\nx = 1\n", &["duplicate binding of x: bound again here"]);
}

#[test]
fn an_expression_can_only_come_last() {
    let got = run("1 + 1\nx = 2\n");
    assert_eq!(got.diags, ["parse error: an expression can only come last in a program"]);
}

#[test]
fn a_definition_is_checked_against_its_signature() {
    rejects("n : Int\nn = True\n", &["type mismatch: expected Int, but this has type Bool"]);
}

// ---------------------------------------------------------------------------
// Natives and @terminating
// ---------------------------------------------------------------------------

#[test]
fn a_native_is_declared_with_its_type_and_must_be_one() {
    rejects(
        "@native\nfrobnicate : Int -> Int\n\n1",
        &["there is no native called frobnicate: @native declares something every back end implements, and this is not one of them"],
    );
    // the prelude's array natives are declared already; a second
    // declaration is checked the same way, and its arity must fit
    rejects(
        "@native\narrayLen : {a : Type} -> Array a -> Int -> Int\n\n1",
        &["the type of the native arrayLen does not fit it: it takes 1 argument at run time, after any implicit ones, and has no effects"],
    );
    rejects("@native\nf x = x\n\n1", &["parse error: @native goes on a signature, f : a"]);
}

#[test]
fn terminating_vouches_for_a_recursion_the_checker_cannot_see_end() {
    ok(
        "@terminating\ncountdown : (n : [] Int) -> Int\ncountdown n = if n < 1 then 0 else countdown (n - 1)\n\ncountdown 5",
        &["countdown : (n : Int) -> Int"],
        Some("Int"),
    );
    rejects(
        "@terminating\ndata T = A\n\n1",
        &["parse error: @terminating goes on a definition's signature, f : a"],
    );
}

#[test]
fn a_closing_parenthesis_closes_the_blocks_opened_inside_it() {
    // the `)` ends the `of` block, as Haskell's parse-error rule would, and
    // the `in` then the `let`'s
    ok(
        "f : (m : Maybe Int) -> Int\nf m =\n  let x = (case m of\n    Just n -> n\n    Nothing -> 0) in\n  x + 1\n\nf (Just 4)",
        &["f : (m : Maybe Int) -> Int"],
        Some("Int"),
    );
    // and so does a `]`, and indentation inside either is free
    ok(
        "g : (m : Maybe Int) -> List Int\ng m = #[case m of\n    Just n -> n\n    Nothing -> 0, 7]\n\ng Nothing",
        &["g : (m : Maybe Int) -> List Int"],
        Some("List Int"),
    );
    ok("h : Int\nh =\n  let y = (1 +\n 2) in\n  y\n\nh", &["h : Int"], Some("Int"));
}
