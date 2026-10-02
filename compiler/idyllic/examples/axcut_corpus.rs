//! Write every program of the test corpora that runs, as the AxCut it is
//! lowered to, with what running it gives: a corpus for other back ends to
//! check themselves against (Meadow's AxCut machine, Glade and Silo, once
//! they take Idyll's programs).
//!
//! ```sh
//! cargo run -p idyllic --example axcut_corpus -- <directory>
//! ```
//!
//! Programs with handlers of their own go in `handlers/`; programs whose
//! only effects are the world's (the runtime's operations, Meadow's
//! `Extern::Native`) in `world/`; and the rest in `pure/`. Each file
//! is the Idyll source and the expected answer as `--` comments, then the
//! AxCut. It is Idyll's own dialect of AxCut (`idyllic_seq`), not yet a
//! shared text format.

use idyllic::pipeline::{self, Evaluation, Options};
use idyllic_diagnostics::OutputStyle;
use std::path::PathBuf;

const CORPORA: [(&str, &str); 3] = [
    ("handwritten", include_str!("../tests/corpus/handwritten.txt")),
    ("exhaustive", include_str!("../tests/corpus/exhaustive.txt")),
    ("tutorial", include_str!("../tests/corpus/tutorial.txt")),
];

fn main() {
    let Some(dir) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: axcut_corpus <directory>");
        std::process::exit(2);
    };
    let opts = Options { debug: false, style: OutputStyle::PLAIN, live: false };
    let mut counts = [0usize; 3];
    for (corpus, text) in CORPORA {
        for (i, src) in text.replace("\r\n", "\n").split("\n%%%\n").enumerate() {
            let out = pipeline::run(src, &opts);
            if out.has_errors() {
                continue;
            }
            let (Some(program), Some(evaluation)) = (&out.axcut, &out.evaluation) else { continue };
            let expect = match evaluation {
                Evaluation::Value(v) => format!("value {v}"),
                Evaluation::Opaque if out.ran_main => "value ()".to_string(),
                Evaluation::Opaque => "nothing to show".to_string(),
                Evaluation::Exited(code) => format!("exit {code}"),
                // (a program that runs out of fuel, or stops, pins nothing down)
                _ => continue,
            };
            let text = program.to_string();
            let kind = match (text.contains("handle "), text.contains("perform ")) {
                (true, _) => 2,
                (false, true) => 1,
                (false, false) => 0,
            };
            let mut file = String::new();
            for line in src.trim_end().lines() {
                file.push_str(&format!("-- | {line}\n"));
            }
            file.push_str(&format!("-- expect: {expect}\n"));
            for line in out.printed.lines() {
                file.push_str(&format!("-- prints: {line}\n"));
            }
            file.push('\n');
            file.push_str(&text);
            file.push('\n');
            let path = dir.join(["pure", "world", "handlers"][kind]).join(format!("{corpus}-{i:04}.axcut"));
            std::fs::create_dir_all(path.parent().expect("a directory")).expect("the directory can be made");
            std::fs::write(&path, file).expect("the file can be written");
            counts[kind] += 1;
        }
    }
    let [pure, world, handlers] = counts;
    println!("{pure} pure programs, {world} with the world's effects, {handlers} with handlers, in {}", dir.display());
}
