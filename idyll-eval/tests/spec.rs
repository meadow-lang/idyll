//! The reference interpreter is the specification: every program the
//! compiler runs must come to the answer the interpreter computes.
//!
//! What is checked, for every well-typed program in the corpus:
//!
//! * the AxCut machine, running the program as lowered, agrees with the
//!   interpreter;
//! * so does the machine running it inlined, which is what native code is
//!   compiled from;
//! * and the interpreter never gets stuck: a program that type checks has
//!   somewhere to go at every step.
//!
//! Agreement is on the answer as the REPL shows it, and on there being
//! nothing to show (a function, a type).  Neither implementation can decide
//! whether a program stops, so both get a generous budget; a program that
//! exhausts it in both is taken to diverge, which is agreement too.
//!
//! The corpus is the compiler's test corpus and every REPL session in the
//! tutorial.  (Not the benchmarks: they are sized for native code, and would
//! only ever run out of fuel here.)

use idyllic::pipeline::{self, Evaluation, Options, Output, Session};
use idyllic_diagnostics::OutputStyle;
use idyllic_seq::machine::{self, MachineError};
use std::path::Path;

/// Steps the interpreter is given, and statements the machine is.
const FUEL: u64 = 5_000_000;

const OPTIONS: Options = Options { debug: false, style: OutputStyle::PLAIN };

/// An answer, as both sides give it.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Shown(String),
    Opaque,
    Diverges,
}

fn reference(out: &Output) -> Result<Option<Answer>, String> {
    let Some(checked) = &out.checked else { return Ok(None) };
    match idyll_eval::evaluate(checked, FUEL) {
        None => Ok(None),
        Some(Ok(o)) => Ok(Some(o.answer.map_or(Answer::Opaque, Answer::Shown))),
        Some(Err(idyll_eval::EvalError::OutOfFuel)) => Ok(Some(Answer::Diverges)),
        Some(Err(e)) => Err(e.to_string()),
    }
}

fn machine(program: &idyllic_seq::Program) -> Result<Answer, String> {
    match machine::run(program, FUEL) {
        Ok(o) if o.memory.leaked + o.memory.miscounted > 0 => Err(o.memory.to_string()),
        Ok(o) => Ok(o.answer.map_or(Answer::Opaque, Answer::Shown)),
        Err(MachineError::OutOfFuel) => Ok(Answer::Diverges),
        Err(e) => Err(e.to_string()),
    }
}

/// Hold one program's output to the specification.  Returns whether there
/// was anything to compare.
fn check(what: &str, src: &str, out: &Output, failures: &mut Vec<String>) -> bool {
    if out.has_errors() {
        return false;
    }
    let spec = match reference(out) {
        Ok(Some(spec)) => spec,
        Ok(None) => return false,
        Err(why) => {
            failures.push(format!("{what}: the reference interpreter got {why}\n{src}"));
            return true;
        }
    };
    // the pipeline's own run gives up sooner than we do; re-run to be sure
    let programs = [("lowered", out.axcut.as_ref()), ("inlined", out.native.as_ref())];
    for (how, program) in programs {
        let Some(program) = program else {
            failures.push(format!("{what}: nothing was {how}\n{src}"));
            continue;
        };
        match machine(program) {
            Ok(got) if got == spec => {}
            Ok(got) => failures.push(format!("{what}: the machine ({how}) gave {got:?}, the specification {spec:?}\n{src}")),
            Err(why) => failures.push(format!("{what}: the machine ({how}) failed: {why}\n{src}")),
        }
    }
    // and what the REPL showed
    let shown = match &out.evaluation {
        Some(Evaluation::Value(v)) => Answer::Shown(v.clone()),
        Some(Evaluation::Opaque) => Answer::Opaque,
        Some(Evaluation::OutOfFuel) => return true,
        other => {
            failures.push(format!("{what}: the pipeline gave {other:?}\n{src}"));
            return true;
        }
    };
    if shown != spec {
        failures.push(format!("{what}: the REPL showed {shown:?}, the specification {spec:?}\n{src}"));
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
