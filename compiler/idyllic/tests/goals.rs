//! Named holes, `?name` (phase 6 of the roadmap): a goal left for later, as
//! Idris and Agda have them. It is checked as whatever is wanted where it
//! is, reported as a warning with its type and the variables of its
//! definition, and stops the program if it is reached.

use idyllic::pipeline::{self, Evaluation, Options};
use idyllic_diagnostics::{OutputStyle, Severity};

const OPTIONS: Options = Options { debug: false, style: OutputStyle::PLAIN, live: false };

fn run(src: &str) -> pipeline::Output {
    pipeline::run(src, &OPTIONS)
}

/// The goals a program's check reports: each its label, and what its help
/// says is in scope.
fn goals(out: &pipeline::Output) -> Vec<(String, Vec<String>)> {
    out.diagnostics
        .iter()
        .filter(|d| d.message.starts_with("goal ?"))
        .map(|d| (d.labels.first().map(|l| l.message.clone()).unwrap_or_default(), d.help.clone()))
        .collect()
}

#[test]
fn a_goal_is_a_warning_with_its_type_and_context() {
    let out = run("double : Int -> Int\ndouble n = ?twice\n\nfst : Int\nfst = 1\n");
    assert!(!out.has_errors(), "{:#?}", out.diagnostics);
    let found = goals(&out);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "?twice : Int");
    assert!(found[0].1.iter().any(|h| h.contains("n : Int")), "{:?}", found[0].1);
    assert!(out.diagnostics.iter().all(|d| d.severity != Severity::Error));
}

#[test]
fn a_goal_takes_the_type_wanted_where_it_is() {
    let out = run("pick : Bool -> String\npick b = if b then \"yes\" else ?no\n");
    assert_eq!(goals(&out)[0].0, "?no : String");
}

#[test]
fn a_program_with_a_goal_it_never_reaches_runs() {
    let out = run("double : Int -> Int\ndouble n = ?twice\n\n1 + 2\n");
    assert!(matches!(out.evaluation, Some(Evaluation::Value(ref v)) if v == "3"), "{:?}", out.evaluation);
}

#[test]
fn reaching_a_goal_stops_the_program() {
    let out = run("pick : (b : Bool) -> (x : Int) -> Int\npick b x = if b then x else ?other\n\npick False 3\n");
    assert!(matches!(out.evaluation, Some(Evaluation::Exited(1))), "{:?}", out.evaluation);
}

#[test]
fn a_goals_context_is_its_definitions_own_variables() {
    let out = run("f : (a : Int) -> (b : Bool) -> Int\nf a b = let c = a + 1 in ?here\n");
    let help = goals(&out)[0].1.join("\n");
    for x in ["a : Int", "b : Bool", "c : Int"] {
        assert!(help.contains(x), "{x} in {help}");
    }
    // nothing of the prelude's
    assert!(!help.contains("putStrLn"), "{help}");
}
