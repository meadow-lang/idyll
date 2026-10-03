//! Data types, `case`, `if`, lists with `::`, and the comparison and
//! subtraction primitives that make recursion able to stop.

use idyllic_diagnostics::OutputStyle;
use idyllic::pipeline::{self, Evaluation, Options};

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

// ---------------------------------------------------------------------------
// Arithmetic and comparison
// ---------------------------------------------------------------------------

#[test]
fn subtraction_and_comparison() {
    assert_eq!(result("10 - 3 - 2"), "5 : Int");
    assert_eq!(result("2 + 3 * 4 - 1"), "13 : Int");
    assert_eq!(result("1 < 2"), "True : Bool");
    assert_eq!(result("2 + 2 == 4"), "True : Bool");
    assert_eq!(result("3 < 1 + 1"), "False : Bool");
}

#[test]
fn comparison_computes_in_types() {
    assert_eq!(
        run("\\(f : [0] Bool -> Type) (p : f True) -> (p : f (1 < 2))").body_type.as_deref(),
        Some("(f : [0] Bool -> Type) -> (p : f True) -> f True")
    );
}

// ---------------------------------------------------------------------------
// if and case
// ---------------------------------------------------------------------------

#[test]
fn if_is_a_case_on_bool() {
    assert_eq!(result("if 1 < 2 then 10 else 20"), "10 : Int");
    assert_eq!(result("case 2 < 1 of { True -> 10; False -> 20 }"), "20 : Int");
    rejects("if 1 then 2 else 3", &["type mismatch: expected Bool, but this has type Int"]);
}

#[test]
fn only_one_alternative_runs_so_usages_join() {
    // x is used once whichever way it goes
    assert_eq!(result("(\\(b : Bool) (x : Int) -> if b then x else x + 1) False 1"), "2 : Int");
    // used in one alternative only: 0..1, which a linear variable does not allow
    rejects(
        "@linear\n\n\\(b : Bool) (x : Int) -> if b then x else 0",
        &["usage mismatch: x has grade 1, but is used 0..1 times"],
    );
    assert_eq!(
        run("\\(b : Bool) (x : [0..1] Int) -> if b then x else 0").body_type.as_deref(),
        Some("(b : Bool) -> (x : [0..1] Int) -> Int")
    );
    // twice in one and once in the other: 1..2
    rejects(
        "@linear\n\n\\(b : Bool) (x : Int) -> if b then x + x else x",
        &["usage mismatch: x has grade 1, but is used 1..2 times"],
    );
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

const MAYBE: &str = "data Maybe a = Nothing | Just a\n\nuse Maybe.*\n\n";

#[test]
fn a_declaration_introduces_a_type_and_its_constructors() {
    let out = run("data Maybe a = Nothing | Just a\n");
    assert!(out.diagnostics.is_empty(), "{:?}", diags(&out));
    let decls: Vec<String> = out.decls.iter().map(|(x, ty)| format!("{x} : {ty}")).collect();
    assert_eq!(
        decls,
        [
            "Maybe : (a : Type) -> Type",
            "Maybe.Nothing : {a : Type} -> Maybe a",
            "Maybe.Just : {a : Type} -> a -> Maybe a",
        ]
    );
}

#[test]
fn constructors_build_and_case_takes_apart() {
    assert_eq!(result(&format!("{MAYBE}Just 3")), "Just 3 : Maybe Int");
    assert_eq!(result(&format!("{MAYBE}Just (Just True)")), "Just (Just True) : Maybe (Maybe Bool)");
    assert_eq!(result(&format!("{MAYBE}case Just 3 of\n  Nothing -> 0\n  Just x -> x + 1\n")), "4 : Int");
}

#[test]
fn a_constructor_is_a_function_too() {
    assert_eq!(result(&format!("{MAYBE}(\\(f : Int -> Maybe Int) -> f 2) Just")), "Just 2 : Maybe Int");
}

#[test]
fn the_papers_from_maybe() {
    // the default may go unused, so it is affine
    let src = format!(
        "{MAYBE}fromMaybe : {{a : Type}} -> a [0..1] -> Maybe a -> a\nfromMaybe [d] m = case m of\n  Nothing -> d\n  Just x -> x\n\n"
    );
    assert_eq!(result(&format!("{src}fromMaybe [1] (Just 100)")), "100 : Int");
    assert_eq!(result(&format!("{src}fromMaybe [1] Nothing")), "1 : Int");
}

#[test]
fn fields_are_linear() {
    rejects(
        &format!("@linear\n\n{MAYBE}\\(m : Maybe Int) -> case m of {{ Nothing -> 0; Just x -> x + x }}"),
        &["usage mismatch: x has grade 1, but is used 2 times"],
    );
    // a field to be used otherwise is a box
    assert_eq!(
        result(&format!("{MAYBE}(\\(m : Maybe (Int [2])) -> case m of {{ Nothing -> 0; Just b -> let [x] = b in x + x }}) (Just [21])")),
        "42 : Int"
    );
}

#[test]
fn a_case_must_cover_the_type_and_nothing_else() {
    rejects(
        &format!("{MAYBE}\\(m : Maybe Int) -> case m of {{ Just x -> x }}"),
        &["the patterns do not cover every case: missing: Nothing"],
    );
    rejects(
        &format!("{MAYBE}\\(m : Maybe Int) -> case m of {{ Nothing -> 0; Just x -> x; Nothing -> 1 }}"),
        &["unreachable: the cases before this one cover everything it matches"],
    );
    rejects(
        &format!("{MAYBE}\\(m : Maybe Int) -> case m of {{ Nothing -> 0; Just -> 1 }}"),
        &["Just has 1 fields, not 0: one pattern for each field"],
    );
    rejects(
        &format!("{MAYBE}\\(m : Maybe Int) -> case m of {{ Nothing -> 0; Just x -> x; Nil -> 2 }}"),
        &["Nil is not a constructor of this type: what is being matched has type Maybe Int"],
    );
}

#[test]
fn types_can_take_several_parameters_and_be_recursive() {
    let src = "data Two a b = Two a b\nuse Two.*\ndata Tree a = Leaf | Node (Tree a) a (Tree a)\n\nuse Tree.*\n\n\
               size : {a : Type} -> Tree (a [0]) -> Int\n\
               size t = case t of\n  Leaf -> 0\n  Node l x r -> let [y] = x in size l + 1 + size r\n\n";
    assert_eq!(result(&format!("{src}Two 1 True")), "Two 1 True : Two Int Bool");
    assert_eq!(result(&format!("{src}size (Node (Node Leaf [1] Leaf) [2] Leaf)")), "2 : Int");
}

// ---------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------

#[test]
fn cons_is_an_infix_constructor_and_associates_to_the_right() {
    assert_eq!(result("1 :: 2 :: 3 :: Nil"), "1 :: 2 :: 3 :: Nil : List Int");
    assert_eq!(result("(1 :: Nil) :: Nil"), "(1 :: Nil) :: Nil : List (List Int)");
    // looser than arithmetic, tighter than comparison
    assert_eq!(result("1 + 1 :: Nil"), "2 :: Nil : List Int");
}

#[test]
fn recursion_over_lists_terminates() {
    let src = "sum : List Int -> Int\nsum xs = case xs of\n  Nil -> 0\n  y :: ys -> y + sum ys\n\n\
               upto : (n : [] Int) -> List Int ! Div\nupto n = if n == 0 then Nil else n :: upto (n - 1)\n\n";
    assert_eq!(result(&format!("{src}upto 4")), "4 :: 3 :: 2 :: 1 :: Nil : List Int");
    assert_eq!(result(&format!("{src}sum (upto 100)")), "5050 : Int");
}

#[test]
fn a_long_list_needs_no_stack() {
    // 20 000 nested calls, each waiting on the next: continuations are heap
    // blocks, so there is no stack to overflow
    let src = "upto : (n : [] Int) -> List Int ! Div\nupto n = if n == 0 then Nil else n :: upto (n - 1)\n\n\
               sum : List Int -> Int\nsum xs = case xs of\n  Nil -> 0\n  y :: ys -> y + sum ys\n\n\
               sum (upto 20000)\n";
    assert_eq!(result(src), "200010000 : Int");
}

#[test]
fn consuming_a_list_frees_it_as_it_goes() {
    // a linear traversal: every cell is unique, released as it is matched and
    // reused by the next allocation
    let src = "build : (n : [] Int) -> List Int -> List Int ! Div\nbuild n acc = if n == 0 then acc else build (n - 1) (n :: acc)\n\n\
               sum : List Int -> Int -> Int\nsum xs acc = case xs of\n  Nil -> acc\n  y :: ys -> sum ys (acc + y)\n\n\
               sum (build 1000 Nil) 0 + sum (build 1000 Nil) 0\n";
    let out = run(src);
    assert_eq!(out.evaluation, Some(Evaluation::Value("1001000".into())));
    let m = out.memory.expect("it ran");
    assert!(m.was_linear(), "{m}");
    // the second list is built entirely out of the first one's cells
    assert!(m.fresh <= 1002, "{m}");
    assert_eq!((m.leaked, m.miscounted), (0, 0));
}
