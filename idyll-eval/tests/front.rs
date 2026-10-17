//! `front/`'s CEK machine (`front/src/Cek.mw`), the reference interpreter
//! written again in Meadow, held to this one: every program of the corpus
//! that checks is run by both, on the Core each checker made of it, and
//! must come to the same answer in the same number of steps, printing the
//! same things on the way, or stop in the same way.
//!
//! The harness is the compiler's (`compiler/idyllic/tests/support/front.rs`):
//! it builds `front/` with the `meadow` on the path, and passes, saying
//! so, if there is none.

#[path = "../../compiler/idyllic/tests/support/tutorial_examples.rs"]
#[allow(dead_code)]
mod tutorial_examples;

#[path = "../../compiler/idyllic/tests/support/front.rs"]
#[allow(dead_code)]
mod harness;

use idyll_eval::EvalError;
use idyllic_world::World;

/// The steps each machine is given, as `tests/spec.rs` gives them.
const FUEL: u64 = 5_000_000;

#[test]
fn the_runs_agree() {
    let source = idyllic::pipeline::PRELUDE;
    let prelude = harness::program(source).expect("the prelude parses");
    harness::agree_with(
        &["run", "--prelude", "_prelude.txt"],
        &[("_prelude.txt", source)],
        |src| ran(&prelude, src),
        |d| d.to_string(),
    );
}

/// A program run here, after the prelude, as `front/src/Main.mw` writes its
/// run: the answer as the REPL shows it (`-` for a function or a type) and
/// the steps it took, or how it stopped; then what it printed, and what it
/// wrote as errors.
fn ran(prelude: &idyllic_ast::Program, src: &str) -> String {
    let Some(p) = harness::program(src) else { return "(error)".to_string() };
    let result = idyllic_rename::rename_module(&p, prelude);
    if !result.errors.is_empty() {
        return "(error)".to_string();
    }
    let module = idyllic_scc::scc_module(result.module);
    let checked = idyllic_typecheck::check_module(&module, result.spans.len(), false, &[]);
    if !checked.errors.is_empty() {
        return "(error)".to_string();
    }
    let mut world = World::captured();
    let Some(outcome) = idyll_eval::evaluate_in(&checked, FUEL, &mut world) else {
        return "(no expression)".to_string();
    };
    let (printed, errors) = world.into_streams();
    let mut lines = match outcome {
        Ok(o) => vec![format!("answer: {}", o.answer.as_deref().unwrap_or("-")), format!("steps: {}", o.steps)],
        // what made it stuck is a compiler bug either way, said differently
        Err(EvalError::Stuck(_)) => vec!["stopped: stuck".to_string()],
        Err(e) => vec![format!("stopped: {e}")],
    };
    if !printed.is_empty() {
        lines.push(format!("printed: {printed:?}"));
    }
    if !errors.is_empty() {
        lines.push(format!("errors: {errors:?}"));
    }
    lines.join("\n")
}
