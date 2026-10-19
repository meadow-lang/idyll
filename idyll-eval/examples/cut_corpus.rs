//! Write every program of the test corpora that runs, as the Cut it is
//! lowered to, with what running it gives: the corpus Meadow's `meadow-cut`
//! reference interpreter, and Glade and Silo, check themselves against (the
//! shared back end's steps 3 and 4).
//!
//! ```sh
//! cargo run --release --example cut_corpus -- <directory>
//! ```
//!
//! Each program is two files: `NAME.cut`, the Cut and nothing else, as
//! `docs/CUT.md` has it (it has no comments); and `NAME.expect`, the Idyll
//! source (`-- |` lines), what running it printed (`prints:`, a line each),
//! what its answer is written as (`answer:`, which the Cut's `answer str`
//! prints), and how it ended (`exit:`); and `nondeterministic` if it uses
//! random numbers or the clock, when only how it ended can be checked. What
//! is expected is what the reference interpreter, Idyll's specification,
//! computes: it lives here, beside the interpreter, so that no runtime is
//! checked against itself. Programs with handlers of their own go in
//! `handlers/`, those whose only effects are the world's in `world/`, and
//! the rest in `pure/`; one that does not lower yet is listed, with why, in
//! `not-lowered.txt`.

use idyll_eval::EvalError;
use idyllic::pipeline::{self, Mode, Options};
use idyllic_diagnostics::OutputStyle;
use idyllic_world::World;
use std::path::PathBuf;

/// Steps the interpreter is given, as `tests/spec.rs` gives them.
const FUEL: u64 = 5_000_000;

const CORPORA: [(&str, &str); 3] = [
    ("handwritten", include_str!("../../compiler/idyllic/tests/corpus/handwritten.txt")),
    ("exhaustive", include_str!("../../compiler/idyllic/tests/corpus/exhaustive.txt")),
    ("tutorial", include_str!("../../compiler/idyllic/tests/corpus/tutorial.txt")),
];

#[path = "../../compiler/idyllic/tests/support/tutorial_examples.rs"]
#[allow(dead_code)]
mod tutorial_examples;

/// What has been written, and what could not be.
#[derive(Default)]
struct Written {
    counts: [usize; 3],
    not_lowered: String,
}

fn main() {
    let Some(dir) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: cut_corpus <directory>");
        std::process::exit(2);
    };
    let opts = Options { debug: false, style: OutputStyle::PLAIN, live: false };
    let mut written = Written::default();
    for (corpus, text) in CORPORA {
        for (i, src) in text.replace("\r\n", "\n").split("\n%%%\n").enumerate() {
            write(&dir, &format!("{corpus}-{i:04}"), src, &opts, &mut written);
        }
    }
    // and every entry of the tutorial's sessions that checks, as the program
    // the session has made by then
    let markdown = include_str!("../../TUTORIAL.md").replace("\r\n", "\n");
    let mut session = pipeline::Session::default();
    let mut block = 0;
    for ex in tutorial_examples::examples(&markdown) {
        if ex.block != block {
            session.reset();
            block = ex.block;
        }
        let entered = session.enter_in(&ex.source, &opts, Mode::Lower);
        if entered.output.has_errors() {
            continue;
        }
        write(&dir, &format!("tutorial-md-{:04}", ex.line), &entered.source, &opts, &mut written);
    }
    std::fs::write(dir.join("not-lowered.txt"), &written.not_lowered).expect("the file can be written");
    let [pure, world, handlers] = written.counts;
    println!(
        "{pure} pure programs, {world} with the world's effects, {handlers} with handlers, in {}; {} not lowered yet",
        dir.display(),
        written.not_lowered.lines().count()
    );
}

/// One program, if it runs to an answer: its Cut, and what to expect.
fn write(dir: &std::path::Path, name: &str, src: &str, opts: &Options, written: &mut Written) {
    let out = pipeline::compile(src, opts);
    if out.has_errors() {
        return;
    }
    let Some(checked) = &out.checked else { return };
    let mut world = World::captured();
    let result = idyll_eval::evaluate_in(checked, FUEL, &mut world);
    let printed = world.into_output();
    let (answer, exit) = match result {
        Some(Ok(o)) => (o.answer.filter(|_| !out.ran_main), 0),
        Some(Err(EvalError::Exited(code))) => (None, code),
        // (a program that runs out of steps, or stops, pins nothing down;
        // one that only declares has nothing to run)
        _ => return,
    };
    let cut = match &out.cut {
        Some(cut) => cut.clone(),
        None => {
            written.not_lowered.push_str(&format!("{name}: {}\n", out.not_lowered.join("; ")));
            return;
        }
    };
    let kind = match (cut.contains("handle {"), cut.contains("perform ")) {
        (true, _) => 2,
        (false, true) => 1,
        (false, false) => 0,
    };
    let mut expect = String::new();
    for line in src.trim_end().lines() {
        expect.push_str(&format!("-- | {line}\n"));
    }
    for line in printed.lines() {
        expect.push_str(&format!("prints: {line}\n"));
    }
    if let Some(answer) = answer {
        expect.push_str(&format!("answer: {answer}\n"));
    }
    expect.push_str(&format!("exit: {exit}\n"));
    // what a random number or the clock gave cannot be expected again: only
    // how it ended can
    if ["Random.", "Time.now", "Time.monotonic"].iter().any(|op| cut.contains(&format!("perform idyll:Prelude/{op}"))) {
        expect.push_str("nondeterministic\n");
    }
    let at = dir.join(["pure", "world", "handlers"][kind]);
    std::fs::create_dir_all(&at).expect("the directory can be made");
    std::fs::write(at.join(format!("{name}.cut")), cut).expect("the file can be written");
    std::fs::write(at.join(format!("{name}.expect")), expect).expect("the file can be written");
    written.counts[kind] += 1;
}
