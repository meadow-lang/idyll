//! `TUTORIAL.md` is executable: every REPL example in it is run, and must
//! produce exactly the types and the diagnostics the tutorial shows.
//!
//! An example is a `> ` line inside a fenced block, plus the lines that
//! continue it (shown as the REPL shows them: the prompt is only on the first
//! line).  A multi-line entry ends at a blank line, as it does in the REPL.
//! What follows, up to the next example, is its expected output:
//!
//! ```text
//! : <type>                    the type of the entry's expression
//! <value> : <type>            and its value, when that is something to show
//! <name> : <type>             or of each definition it makes
//! <line>                      what it printed, after a blank line that ends
//!                             even a one-line entry, before its value
//! Error: <headline>           a diagnostic, in order; none may be left out
//! Warning: <headline>
//!    │ ╰── <label>            optionally, that diagnostic's primary label
//! ```
//!
//! Other lines of a rendered diagnostic (the quoted source, `Help:`) are
//! ignored, as the tutorial trims them.  Blocks with no `> ` line are prose.
//!
//! Each fenced block is one REPL session: a declaration made in it stays in
//! scope until the block ends.

use idyllic_diagnostics::{OutputStyle, Severity};
use idyllic::pipeline::{Evaluation, Options, Session};

#[path = "support/tutorial_examples.rs"]
mod tutorial_examples;

use tutorial_examples::examples;

#[test]
fn every_tutorial_example_behaves_as_shown() {
    let markdown = include_str!("../../../TUTORIAL.md").replace("\r\n", "\n");
    let examples = examples(&markdown);
    assert!(examples.len() > 85, "only found {} examples; has the format changed?", examples.len());

    let opts = Options { debug: false, style: OutputStyle::PLAIN, live: false };
    let mut failures = Vec::new();
    let mut session = Session::default();
    let mut block = 0;
    for ex in &examples {
        if ex.block != block {
            session.reset();
            block = ex.block;
        }
        let out = session.enter(&ex.source, &opts).output;
        let got: Vec<(String, String)> = out
            .diagnostics
            .iter()
            .map(|d| {
                let kind = if d.severity == Severity::Error { "error" } else { "warning" };
                let label = d.labels.first().map_or(String::new(), |l| l.message.clone());
                (format!("{kind}: {}", d.message), label)
            })
            .collect();

        let mut problems = Vec::new();
        let headlines: Vec<&String> = got.iter().map(|(h, _)| h).collect();
        let expected: Vec<&String> = ex.diags.iter().map(|(h, _)| h).collect();
        if headlines != expected {
            problems.push(format!("diagnostics: tutorial shows {expected:?}, got {headlines:?}"));
        } else {
            for ((_, want), (_, have)) in ex.diags.iter().zip(&got) {
                if let Some(want) = want.as_ref().filter(|w| *w != have) {
                    problems.push(format!("label: tutorial shows {want:?}, got {have:?}"));
                }
            }
        }
        if let Some(want) = &ex.ty {
            let have = out.body_type.as_deref().unwrap_or("NONE");
            if want != have {
                problems.push(format!("type: tutorial shows {want:?}, got {have:?}"));
            }
        }
        let have = match &out.evaluation {
            Some(Evaluation::Value(v)) => Some(v.clone()),
            Some(Evaluation::Failed(why)) => {
                problems.push(format!("evaluation failed: {why}"));
                None
            }
            _ => None,
        };
        if have != ex.value {
            problems.push(format!("value: tutorial shows {:?}, got {have:?}", ex.value));
        }
        let printed: Vec<&str> = out.printed.lines().collect();
        if printed != ex.printed {
            problems.push(format!("printed: tutorial shows {:?}, got {printed:?}", ex.printed));
        }
        // an entry with no expression reports its definitions instead
        if out.body_type.is_none() && !out.has_errors() || !ex.decls.is_empty() {
            let have: Vec<String> = out.decls.iter().map(|(x, ty)| format!("{x} : {ty}")).collect();
            if have != ex.decls {
                problems.push(format!("definitions: tutorial shows {:?}, got {have:?}", ex.decls));
            }
        }
        if !problems.is_empty() {
            failures.push(format!("TUTORIAL.md:{}\n{}\n  {}", ex.line, ex.source, problems.join("\n  ")));
        }
    }
    assert!(failures.is_empty(), "{} tutorial examples are wrong:\n\n{}", failures.len(), failures.join("\n\n"));
}
