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

use idyllic::diagnostics::{OutputStyle, Severity};
use idyllic::pipeline::{Evaluation, Options, Session};

#[derive(Debug, Default)]
struct Example {
    line: usize,
    /// which fenced block (session) it belongs to
    block: usize,
    source: String,
    ty: Option<String>,
    /// what the entry evaluates to, where the tutorial shows it
    value: Option<String>,
    /// `name : type`, for an entry of declarations
    decls: Vec<String>,
    /// (headline with severity, primary label if shown)
    diags: Vec<(String, Option<String>)>,
}

/// `value : type`, as the REPL reports an expression that comes to plain
/// data: a number, a string, `True`, `()`, or a box of one.
fn value_report(line: &str) -> Option<(&str, &str)> {
    // a number, a string, a box, `()`, or a constructor (`True`, `Just 3`,
    // `1 :: Nil` starts with a digit).  No line of *source* in the tutorial
    // starts with a capital and goes on to a ` : `.
    let starts_a_value = line.starts_with(|c: char| c.is_ascii_digit() || c.is_ascii_uppercase() || c == '"' || c == '[' || c == '(');
    // a string may contain " : " itself; its report ends at the last quote
    let at = if line.starts_with('"') { line.rfind("\" : ")? + 1 } else { line.find(" : ")? };
    starts_a_value.then(|| (&line[..at], &line[at + 3..]))
}

/// Whether an entry is declarations (so that what follows reports them)
/// rather than an expression (so that it reports a value): `Nothing : ..` is
/// the one or the other depending on what was asked.
fn declares(source: &str) -> bool {
    let first = source.lines().next().unwrap_or("");
    let mut words = first.split_whitespace();
    match (words.next(), words.next()) {
        (Some("data"), _) => true,
        (Some("let" | "if" | "case"), _) => false,
        (Some(name), second) => {
            name.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
                && (second == Some(":") || first.contains(" = "))
        }
        _ => false,
    }
}

/// `name : type`, as the REPL reports a definition, a data type or a
/// constructor (which may be `::`).
fn is_definition_report(line: &str) -> bool {
    let Some((name, _)) = line.split_once(" : ") else { return false };
    let mut chars = name.chars();
    name == "::"
        || chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '\'')
}

fn examples(markdown: &str) -> Vec<Example> {
    let mut out: Vec<Example> = Vec::new();
    let mut in_block = false;
    let mut block = 0;
    // whether we are still reading the current example's source lines
    let mut in_source = false;
    // whether this block has shown a prompt yet; until it does, its lines
    // are prose and say nothing about the previous block's example
    let mut has_example = false;
    let mut margin = 0;
    for (i, line) in markdown.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            in_block = !in_block;
            margin = 0;
            block += usize::from(in_block);
            in_source = false;
            has_example = false;
            continue;
        }
        if !in_block {
            continue;
        }
        // a block inside a list item is indented; its first prompt marks the
        // margin, which is not part of the source
        if !has_example && line.trim_start().starts_with("> ") {
            margin = line.len() - line.trim_start().len();
        }
        let line = match line.get(..margin) {
            Some(pad) if pad.trim().is_empty() => &line[margin..],
            _ => line,
        };
        if let Some(first) = line.strip_prefix("> ") {
            out.push(Example { line: i + 1, block, source: first.to_string(), ..Example::default() });
            in_source = true;
            has_example = true;
            continue;
        }
        let Some(ex) = out.last_mut().filter(|_| has_example) else { continue };
        if line.trim().is_empty() {
            in_source = false;
        } else if let Some(ty) = line.strip_prefix(": ") {
            ex.ty = Some(ty.to_string());
            in_source = false;
        } else if let Some((value, ty)) = value_report(line).filter(|_| !declares(&ex.source)) {
            ex.value = Some(value.to_string());
            ex.ty = Some(ty.to_string());
            in_source = false;
        } else if let Some(h) = line.strip_prefix("Error: ") {
            ex.diags.push((format!("error: {h}"), None));
            in_source = false;
        } else if let Some(h) = line.strip_prefix("Warning: ") {
            ex.diags.push((format!("warning: {h}"), None));
            in_source = false;
        } else if let Some((_, label)) = line.split_once('╰') {
            let label = label.trim_start_matches(['─', '╸']).trim();
            if let Some(last) = ex.diags.last_mut() {
                last.1 = Some(label.to_string());
            }
        } else if in_source && !line.contains('│') {
            ex.source.push('\n');
            ex.source.push_str(line);
        } else if ex.diags.is_empty() && is_definition_report(line) {
            ex.decls.push(line.to_string());
        }
    }
    out
}

#[test]
fn every_tutorial_example_behaves_as_shown() {
    let markdown = include_str!("../../TUTORIAL.md").replace("\r\n", "\n");
    let examples = examples(&markdown);
    assert!(examples.len() > 85, "only found {} examples; has the format changed?", examples.len());

    let opts = Options { debug: false, style: OutputStyle::PLAIN };
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
