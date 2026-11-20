//! Operators: any can be written infix, grouped by its fixity (`infixl 6
//! <+>`, as in Haskell and Meadow; `infixl 9` if nothing says); the
//! prelude's pipes, `|>` and `<|`; and sections, `(+ 1)` and `(1 +)`.

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

const OPS: &str = "infixl 6 <+>\ninfixr 5 <:>\n\n\
                   (<+>) : Int -> Int -> Int\na <+> b = a * 10 + b\n\n\
                   (<:>) : Int -> List Int -> List Int\nx <:> xs = x :: xs\n\n";

#[test]
fn an_operator_groups_as_its_fixity_says() {
    // to the left, and looser than `*`
    assert_eq!(result(&format!("{OPS}(1 <+> 2 <+> 3, 1 <+> 2 * 3)\n")), "(123, 16) : (Int, Int)");
    // to the right
    assert_eq!(result(&format!("{OPS}1 <:> 2 <:> Nil\n")), "1 :: 2 :: Nil : List Int");
}

#[test]
fn an_operator_nothing_declares_is_infixl_9() {
    let src = "(<+>) : Int -> Int -> Int\na <+> b = a * 10 + b\n\n(1 <+> 2 * 3, 1 <+> 2 <+> 3)\n";
    assert_eq!(result(src), "(36, 123) : (Int, Int)");
}

#[test]
fn a_fixity_may_be_declared_for_several_operators_and_named_in_parentheses() {
    let src = "infixl 6 <+>, (<->)\n\n\
               (<+>) : Int -> Int -> Int\na <+> b = a + b\n\n\
               (<->) : Int -> Int -> Int\na <-> b = a - b\n\n\
               10 <-> 2 <+> 3 * 2\n";
    assert_eq!(result(src), "14 : Int");
}

#[test]
fn infixl_is_a_name_anywhere_but_before_a_number() {
    assert_eq!(result("infixl : Int\ninfixl = 3\n\ninfixl + 1\n"), "4 : Int");
}

#[test]
fn a_pipeline_reads_in_the_order_it_runs() {
    let src = "double : Int -> Int\ndouble n = n * 2\n\n(3 |> double |> (+ 1), double <| 1 + 2, 1 + 2 |> double)\n";
    assert_eq!(result(src), "(7, 6, 6) : (Int, Int, Int)");
}

#[test]
fn a_section_waits_for_the_operand_it_is_missing() {
    let src = "((+ 1) 2, (10 -) 3, (- 1) 10, (:: Nil) 7, (\"a\" ++) \"b\")\n";
    assert_eq!(result(src), "(3, 7, 9, 7 :: Nil, \"ab\") : (Int, Int, Int, List Int, String)");
}

#[test]
fn a_sections_operand_is_everything_on_its_side() {
    assert_eq!(result("((* 2 + 3) 4, (1 + 2 *) 4)\n"), "(20, 12) : (Int, Int)");
    assert_eq!(result(&format!("{OPS}(1 <+>) 5\n")), "15 : Int");
}

#[test]
fn a_sections_type_names_no_parameter() {
    rejects("x : Int\nx = (+ 1)\n\nx\n", &["type mismatch: expected Int, but this has type Int -> Int"]);
}

#[test]
fn operators_of_one_level_that_do_not_group_alike_ask_for_parentheses() {
    rejects("1 < 2 == True\n", &["parse error: `<` and `==` are both `infix 4`, and do not chain: write parentheses"]);
    rejects(
        "infixl 6 <+>\ninfixr 6 <->\n\n(<+>) : Int -> Int -> Int\na <+> b = a\n\n(<->) : Int -> Int -> Int\na <-> b = a\n\n1 <+> 2 <-> 3\n",
        &["parse error: `<+>` is `infixl 6` and `<->` is `infixr 6`, which do not group the same way: write parentheses"],
    );
}

#[test]
fn a_fixity_declaration_is_checked() {
    rejects("infixr 6 +\n\n1\n", &["the fixity of `+` cannot change: `+` is `infixl 6`, not `infixr 6`"]);
    rejects(
        "infixl 6 <+>\ninfixl 7 <+>\n\n1\n",
        &["two fixities for `<+>`: this says `infixl 7`, but `infixl 6` was declared first"],
    );
    rejects("infixl 10 <+>\n\n1\n", &["parse error: a fixity's level is from 0 to 9"]);
    rejects("(1, 2 +) 3\n", &["parse error: a section takes one operand, not a tuple"]);
}

#[test]
fn a_declaration_may_repeat_a_built_in_fixity() {
    assert_eq!(result("infixl 6 +\ninfixl 1 |>\n\n1 + 2 |> (* 2)\n"), "6 : Int");
}

// `and` and `or` look at their right operand only if they have to: `loop`
// is never called here
#[test]
fn and_and_or_short_circuit() {
    let src = "loop : (n : Int) -> Bool ! Div\nloop n = loop n\n\n\
               safe : (n : Int) -> Bool ! Div\nsafe n = n > 0 or loop n\n\n\
               careful : (n : Int) -> Bool ! Div\ncareful n = n < 0 and loop n\n\n\
               (safe 1, careful 1)\n";
    assert_eq!(result(src), "(True, False) : (Bool, Bool)");
}

#[test]
fn and_binds_tighter_than_or_and_both_looser_than_a_comparison() {
    assert_eq!(result("(True or False and False, 1 < 2 and 2 < 1, False and False or True)\n"), "(True, False, True) : (Bool, Bool, Bool)");
}

// A name in backticks is written between its arguments, as an operator is:
// `infixl 9` unless a declaration says otherwise
#[test]
fn a_name_in_backticks_is_an_operator() {
    let src = "infixl 7 `times`\n\n\
               times : Int -> Int -> Int\na `times` b = a * b\n\n\
               plus : Int -> Int -> Int\nplus a b = a + b\n\n\
               (1 `plus` 2 `times` 3, 2 `times` 3 + 1, (`plus` 10) 5, (10 `plus`) 1)\n";
    assert_eq!(result(src), "(9, 7, 15, 11) : (Int, Int, Int, Int)");
}
