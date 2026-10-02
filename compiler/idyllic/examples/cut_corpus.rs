//! Write every program of the test corpora that runs, as the Cut it is
//! lowered to, with what running it gives: the corpus Meadow's `meadow-cut`
//! reference interpreter, and then Glade and Silo, check themselves against
//! (the shared back end's steps 3 and 4).
//!
//! ```sh
//! cargo run -p idyllic --example cut_corpus -- <directory>
//! ```
//!
//! Each program is two files: `NAME.cut`, the Cut and nothing else, as
//! `docs/CUT.md` has it (it has no comments); and `NAME.expect`, the Idyll
//! source (`-- |` lines), what running it printed (`prints:`, a line each),
//! what its answer is written as (`answer:`, which the Cut's `answer str`
//! prints), and how it ended (`exit:`); and `nondeterministic` if it uses
//! random numbers or the clock, when only how it ended can be checked. The
//! answers are the AxCut machine's,
//! which `idyll-eval/tests/spec.rs` holds to the reference interpreter's.
//! Programs with handlers of their own go in `handlers/`, those whose only
//! effects are the world's in `world/`, and the rest in `pure/`; one that
//! does not lower yet is listed, with why, in `not-lowered.txt`.

use idyllic::pipeline::{self, Evaluation, Options};
use idyllic_diagnostics::OutputStyle;
use std::path::PathBuf;

const CORPORA: [(&str, &str); 3] = [
    ("handwritten", include_str!("../tests/corpus/handwritten.txt")),
    ("exhaustive", include_str!("../tests/corpus/exhaustive.txt")),
    ("tutorial", include_str!("../tests/corpus/tutorial.txt")),
];

#[path = "../tests/support/tutorial_examples.rs"]
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
    // and every entry of the tutorial's sessions that runs, as the program
    // the session has made by then
    let markdown = include_str!("../../../TUTORIAL.md").replace("\r\n", "\n");
    let mut session = pipeline::Session::default();
    let mut block = 0;
    for ex in tutorial_examples::examples(&markdown) {
        if ex.block != block {
            session.reset();
            block = ex.block;
        }
        let entered = session.enter(&ex.source, &opts);
        if entered.output.has_errors() || entered.output.evaluation.is_none() {
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
    let out = pipeline::run(src, opts);
    if out.has_errors() {
        return;
    }
    let (answer, exit) = match &out.evaluation {
        Some(Evaluation::Value(v)) if !out.ran_main => (Some(v.clone()), 0),
        Some(Evaluation::Value(_) | Evaluation::Opaque) => (None, 0),
        Some(Evaluation::Exited(code)) => (None, *code),
        // (a program that runs out of fuel, or stops, pins nothing down)
        _ => return,
    };
    let compiled = pipeline::compile(src, opts);
    let Some(mut checked) = compiled.checked else { return };
    let cut = match idyllic_cut::lower::lower(&mut checked, "Main") {
        Ok(program) => program.to_string(),
        Err(why) => {
            written.not_lowered.push_str(&format!("{name}: {}\n", why.join("; ")));
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
    for line in out.printed.lines() {
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
