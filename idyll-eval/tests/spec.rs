//! The reference interpreter is the specification: every program the
//! compiler runs must come to the answer the interpreter computes.
//!
//! What is checked, for every well-typed program in the corpus:
//!
//! * the program, lowered to Cut and run on Glade, Meadow's bytecode, as
//!   every Idyll program is run, agrees with the interpreter;
//! * and the interpreter never gets stuck: a program that type checks has
//!   somewhere to go at every step.
//!
//! Agreement is on the answer as the REPL shows it, on there being nothing
//! to show (a function, a type), and on what was printed. A program that
//! ends itself agrees on its exit code, and one stopped by an error of its
//! own (dividing by zero, a failed test) on being stopped: Glade words the
//! error its own way. Neither side can decide whether a program stops: the
//! interpreter gets a generous budget of steps, and a program that exhausts
//! it is taken to diverge, which whatever Glade does agrees with. A program
//! that uses random numbers or the clock agrees only on how it ended.
//!
//! The corpus is the compiler's test corpus and every REPL session in the
//! tutorial.  (Not the benchmarks: they are sized for native code, and would
//! only ever run out of fuel here.) It needs `meadow` on the path, as
//! running any Idyll program does.

use idyllic::pipeline::{self, Evaluation, Options, Output, Session};
use idyllic_diagnostics::OutputStyle;
use idyllic_world::World;
use std::path::Path;

/// Steps the interpreter is given.
const FUEL: u64 = 5_000_000;

const OPTIONS: Options = Options { debug: false, style: OutputStyle::PLAIN, live: false };

/// An answer, as both sides give it.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Shown(String),
    Opaque,
    Diverges,
    /// the program ended itself, with `exit`, and this code
    Exited(i32),
    /// an error of the program's own stopped it
    Stopped,
    /// what was printed on the way to the answer
    Printed(String, Box<Answer>),
}

fn answer(printed: String, shown: Option<String>) -> Answer {
    printing(printed, shown.map_or(Answer::Opaque, Answer::Shown))
}

fn printing(printed: String, answer: Answer) -> Answer {
    if printed.is_empty() {
        answer
    } else {
        Answer::Printed(printed, Box::new(answer))
    }
}

/// How it ended, without what was printed on the way.
fn ending(answer: Answer) -> Answer {
    match answer {
        Answer::Printed(_, answer) => ending(*answer),
        other => other,
    }
}

/// The same, with what it came to, if a value, left unshown.
fn unanswered(answer: Answer) -> Answer {
    match answer {
        Answer::Shown(_) => Answer::Opaque,
        Answer::Printed(printed, answer) => Answer::Printed(printed, Box::new(unanswered(*answer))),
        other => other,
    }
}

fn reference(out: &Output) -> Result<Option<Answer>, String> {
    let Some(checked) = &out.checked else { return Ok(None) };
    use idyll_eval::EvalError;
    let mut world = World::captured();
    let result = idyll_eval::evaluate_in(checked, FUEL, &mut world);
    let printed = world.into_output();
    match result {
        None => Ok(None),
        Some(Ok(o)) => Ok(Some(answer(printed, o.answer))),
        Some(Err(EvalError::OutOfFuel)) => Ok(Some(Answer::Diverges)),
        Some(Err(EvalError::Exited(code))) => Ok(Some(printing(printed, Answer::Exited(code)))),
        Some(Err(EvalError::TestFailed(_) | EvalError::Stopped(_))) => Ok(Some(printing(printed, Answer::Stopped))),
        Some(Err(e)) => Err(e.to_string()),
    }
}

/// Hold one program's run on Glade to the specification.  Returns whether
/// there was anything to compare.
fn check(what: &str, src: &str, out: &Output, failures: &mut Vec<String>) -> bool {
    if out.has_errors() {
        return false;
    }
    let spec = match reference(out) {
        Ok(Some(spec)) => spec,
        Ok(None) => return false,
        Err(why) => {
            failures.push(format!("{what}: the reference interpreter got {why}
{src}"));
            return true;
        }
    };
    if spec == Answer::Diverges {
        return true;
    }
    // a program run for its `main` answers nothing on Glade, which the
    // interpreter shows as `main`'s `()`
    let spec = if out.ran_main { unanswered(spec) } else { spec };
    let ran = match &out.evaluation {
        Some(Evaluation::Value(v)) => answer(out.printed.clone(), Some(v.clone())),
        Some(Evaluation::Opaque) => answer(out.printed.clone(), None),
        Some(Evaluation::TimedOut(_)) => Answer::Diverges,
        Some(Evaluation::Exited(code)) => printing(out.printed.clone(), Answer::Exited(*code)),
        Some(Evaluation::Stopped(_)) => printing(out.printed.clone(), Answer::Stopped),
        other => {
            failures.push(format!("{what}: the pipeline gave {other:?}
{src}"));
            return true;
        }
    };
    // what a random number or the clock gave cannot be expected again (the
    // interpreter's world is seeded, Glade's is not): only how it ended can
    let nondeterministic = out.cut.as_deref().is_some_and(|cut| {
        ["Random.", "Time.now", "Time.monotonic"].iter().any(|op| cut.contains(&format!("perform idyll:Prelude/{op}")))
    });
    let (ran, spec) = if nondeterministic { (ending(ran), ending(spec)) } else { (ran, spec) };
    if ran != spec {
        failures.push(format!("{what}: Glade gave {ran:?}, the specification {spec:?}
{src}"));
    }
    true
}

fn read(path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())).replace("\r\n", "\n")
}

fn report(checked: usize, at_least: usize, failures: Vec<String>) {
    eprintln!("compared {checked} programs");
    assert!(failures.is_empty(), "{} of {checked} disagree:\n\n{}", failures.len(), failures.join("\n\n"));
    assert!(checked >= at_least, "only {checked} programs were compared; is the corpus being read?");
}

#[test]
fn the_compiler_test_corpus() {
    // what does not end is stopped sooner than a REPL entry is
    std::env::set_var("IDYLL_TIMEOUT", "5");
    let (mut checked, mut failures) = (0, Vec::new());
    for corpus in ["handwritten", "exhaustive", "tutorial"] {
        let text = read(&format!("../compiler/idyllic/tests/corpus/{corpus}.txt"));
        for (i, src) in text.split("\n%%%\n").enumerate() {
            let out = pipeline::run(src, &OPTIONS);
            checked += usize::from(check(&format!("{corpus} #{i}"), src, &out, &mut failures));
        }
    }
    report(checked, 50, failures);
}

// only the source of each example is wanted here, not what it shows
#[allow(dead_code)]
#[path = "../../compiler/idyllic/tests/support/tutorial_examples.rs"]
mod tutorial_examples;

/// Each fenced block of the tutorial is a REPL session, and each example in
/// it is entered into that session, as the tutorial's own test does.
#[test]
fn every_tutorial_session() {
    std::env::set_var("IDYLL_TIMEOUT", "5");
    let (mut checked, mut failures) = (0, Vec::new());
    let mut session = Session::default();
    let mut block = 0;
    for ex in tutorial_examples::examples(&read("../TUTORIAL.md")) {
        if ex.block != block {
            session.reset();
            block = ex.block;
        }
        let entered = session.enter(&ex.source, &OPTIONS);
        let what = format!("TUTORIAL.md:{}", ex.line);
        checked += usize::from(check(&what, &entered.source, &entered.output, &mut failures));
    }
    report(checked, 40, failures);
}
