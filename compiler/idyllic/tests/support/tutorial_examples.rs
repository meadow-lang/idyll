//! The REPL examples of `TUTORIAL.md`, read out of the Markdown.  Shared by
//! the tutorial's own test (`tests/tutorial.rs`), which checks each against
//! what the tutorial shows, and by the reference interpreter's specification
//! tests (`idyll-eval/tests/spec.rs`), which run every one.  See
//! `tests/tutorial.rs` for the format.

#[derive(Debug, Default)]
pub struct Example {
    pub line: usize,
    /// which fenced block (session) it belongs to
    pub block: usize,
    pub source: String,
    pub ty: Option<String>,
    /// what the entry evaluates to, where the tutorial shows it
    pub value: Option<String>,
    /// `name : type`, for an entry of declarations
    pub decls: Vec<String>,
    /// what running it printed, line by line
    pub printed: Vec<String>,
    /// (headline with severity, primary label if shown)
    pub diags: Vec<(String, Option<String>)>,
}

/// `value : type`, as the REPL reports an expression that comes to plain
/// data: a number, a string, `True`, `()`, or a box of one.
fn value_report(line: &str) -> Option<(&str, &str)> {
    // a number, a string, a character, a box, `()`, an array (`#[1; 2]`),
    // or a constructor (`True`, `Just 3`, `1 :: Nil` starts with a digit).
    // No line of *source* in the tutorial starts with a capital, or an
    // array, and goes on to a ` : `.
    let starts_a_value = line
        .starts_with(|c: char| c.is_ascii_digit() || c.is_ascii_uppercase() || c == '"' || c == '\'' || c == '[' || c == '(')
        || line.starts_with("#[") && line.contains(';');
    // a string may contain " : " itself; its report ends at the last quote
    let at = if line.starts_with('"') { line.rfind("\" : ")? + 1 } else { line.find(" : ")? };
    starts_a_value.then(|| (&line[..at], &line[at + 3..]))
}

/// Whether an entry is declarations (so that what follows reports them)
/// rather than an expression (so that it reports a value): `Nothing : ..` is
/// the one or the other depending on what was asked.
fn declares(source: &str) -> bool {
    // (`@linear` is the module's, and says nothing about what follows it)
    let first = source.lines().find(|l| l.trim() != "@linear").unwrap_or("");
    let mut words = first.split_whitespace();
    match (words.next(), words.next()) {
        (Some("data" | "effect"), _) => true,
        // an attribute, before a declaration
        (Some(attribute), _) if attribute.starts_with('@') => true,
        (Some("let" | "if" | "case"), _) => false,
        (Some(name), second) => {
            name.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
                && (second == Some(":") || equation(first))
        }
        _ => false,
    }
}

/// Whether a line has an equation's `=`: one outside any brackets, as a
/// record's `{ x = 1 }` is not.
fn equation(line: &str) -> bool {
    let mut depth = 0i32;
    let bytes = line.as_bytes();
    for (i, c) in line.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '=' if depth == 0 => {
                let before = i > 0 && bytes[i - 1] == b' ';
                let after = bytes.get(i + 1) == Some(&b' ');
                if before && after {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// `name : type`, as the REPL reports a definition, a data type or a
/// constructor (which is reported under its type, `Nat.Z`, and may be an
/// operator, `List.::`).
fn is_definition_report(line: &str) -> bool {
    let Some((name, _)) = line.split_once(" : ") else { return false };
    let (qualifier, last) = name.rsplit_once('.').unwrap_or(("", name));
    let word = |w: &str| {
        let mut chars = w.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '\'')
    };
    (qualifier.is_empty() || qualifier.split('.').all(word)) && (word(last) || last == "::")
}

pub fn examples(markdown: &str) -> Vec<Example> {
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
        } else if !in_source && ex.diags.is_empty() && ex.decls.is_empty() && ex.ty.is_none() {
            // after the blank line that ends the entry, and before its
            // value: what it printed
            ex.printed.push(line.to_string());
        }
    }
    out
}
