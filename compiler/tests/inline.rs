//! Inlining (`src/axcut/inline.rs`): what it gets rid of, and that it changes
//! nothing else.  The abstract machine runs the inlined program as it runs
//! the original, so it says whether the two agree — and whether the inlined
//! one still keeps AxCut's ordering discipline and leaks nothing.

use idyllic::axcut::machine;
use idyllic::diagnostics::OutputStyle;
use idyllic::pipeline::{self, Evaluation, Options};

fn run(src: &str) -> pipeline::Output {
    pipeline::run(src, &Options { debug: false, style: OutputStyle::PLAIN })
}

/// The program as the native compiler gets it.
fn inlined(src: &str) -> String {
    let out = run(src);
    assert!(out.diagnostics.is_empty(), "{src}\n{:#?}", out.diagnostics);
    out.native.expect("it was lowered").to_string()
}

#[test]
fn a_function_called_where_it_is_written_is_its_body() {
    let text = inlined("(\\(x : Int) -> x + 1) 41");
    assert!(!text.contains("new"), "{text}");
    assert!(text.contains("extern add"), "{text}");
}

#[test]
fn a_let_bound_function_used_once_is_inlined() {
    let text = inlined("let f = \\(x : Int) -> x * 2 in f 21");
    assert!(!text.contains("apply"), "{text}");
}

#[test]
fn a_small_function_used_twice_is_copied_into_both_calls() {
    let text = inlined("let f : (Int -> Int) [2] = [\\x -> x * 2] in let [g] = f in g (g 5)");
    assert_eq!(text.matches("extern mul").count(), 2, "{text}");
}

#[test]
fn a_big_function_used_twice_is_not_duplicated() {
    let body = "x * 2 * 2 * 2 * 2 * 2 * 2 * 2 * 2 * 2 * 2";
    let text = inlined(&format!("let f : (Int -> Int) [2] = [\\x -> {body}] in let [g] = f in g (g 5)"));
    assert_eq!(text.matches("extern mul").count(), 10, "{text}");
    assert_eq!(text.matches("apply(").count(), 1, "{text}");
}

#[test]
fn a_call_of_a_definition_is_a_jump_and_never_a_closure() {
    let text = inlined("fib : (n : [] Int) -> Int\nfib n = if n < 2 then n else fib (n - 1) + fib (n - 2)\n\nfib 20\n");
    assert!(!text.contains("apply"), "{text}");
    assert_eq!(text.matches("jump fib_").count(), 3, "{text}");
}

#[test]
fn a_definition_that_is_not_recursive_is_inlined_and_gone() {
    let text = inlined("double : Int [2] -> Int\ndouble [n] = n + n\n\ndouble [21]\n");
    assert_eq!(text.matches("define ").count(), 1, "{text}");
    // and the box made for it is opened at compile time
    assert!(!text.contains("box("), "{text}");
}

#[test]
fn a_constructor_matched_on_at_once_is_never_built() {
    let text = inlined("data Maybe a = Nothing | Just a\n\ncase Just 3 of\n  Nothing -> 0\n  Just x -> x + 1\n");
    assert!(!text.contains("switch") && !text.contains("Just("), "{text}");
}

#[test]
fn a_join_point_reached_from_two_places_stays_one() {
    let text = inlined("(\\(b : Bool) -> (if b then 1 else 2) + 10) True");
    assert_eq!(text.matches("extern add").count(), 1, "{text}");
}

/// Every corpus program that runs must come to the same thing inlined.
#[test]
fn inlining_changes_no_answer() {
    let corpora = [
        include_str!("fixtures/corpus-handwritten.txt"),
        include_str!("fixtures/corpus-exhaustive.txt"),
        include_str!("fixtures/corpus-tutorial.txt"),
    ];
    let (mut compared, mut failures) = (0, Vec::new());
    for corpus in corpora {
        for src in corpus.replace("\r\n", "\n").split("\n%%%\n") {
            let out = run(src);
            let (Some(expected), Some(program)) = (&out.evaluation, &out.native) else { continue };
            let got = match machine::run(program, pipeline::FUEL) {
                Ok(o) if o.memory.leaked + o.memory.miscounted > 0 => Evaluation::Failed(o.memory.to_string()),
                Ok(o) => o.answer.map_or(Evaluation::Opaque, Evaluation::Value),
                Err(machine::MachineError::OutOfFuel) => Evaluation::OutOfFuel,
                Err(e) => Evaluation::Failed(e.to_string()),
            };
            compared += 1;
            if got != *expected {
                failures.push(format!("{src}\n  was {expected:?}, inlined {got:?}\n{program}"));
            }
        }
    }
    assert!(failures.is_empty(), "{} programs changed:\n\n{}", failures.len(), failures.join("\n\n"));
    assert!(compared > 50, "only {compared} programs compared; is the corpus being read?");
}

#[test]
fn inlining_saves_the_machine_work_too() {
    let src = "id : {a : Type} -> a -> a\nid x = x\n\nid (id (id (id (id 7))))\n";
    let out = run(src);
    let before = out.memory.expect("it ran").acquired();
    let after = machine::run(&out.native.unwrap(), pipeline::FUEL).unwrap().memory.acquired();
    assert!(after < before, "{after} blocks, from {before}");
}
