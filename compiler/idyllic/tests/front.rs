//! The compiler written in Meadow, `front/`, checked against this one, pass
//! by pass, as MeadowBoot is checked against the Rust Meadow compiler.
//!
//! Each pass of `front/` writes what it made as text, and the same pass here
//! is written the same way; a program whose two texts differ is a difference
//! between the compilers, shown at its first token. The inputs are every
//! program of the compiler's corpora (`tests/corpus/`).
//!
//! `front/` is built once a run, natively (`meadow build --release`), with the
//! `meadow` on the path; with none, the tests say so and pass, so that a
//! machine without Meadow can still run the rest. `IDYLL_FRONT_MEADOW` names
//! another `meadow` to build it with.

use idyllic_lexer::token::Token;
use idyllic_span::LineIndex;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("the repository")
}

fn meadow() -> PathBuf {
    std::env::var_os("IDYLL_FRONT_MEADOW").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("meadow"))
}

/// `front/`, built natively, once: its executable, or why there is none.
fn front() -> Result<PathBuf, String> {
    static BUILT: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let dir = repo().join("front");
            let out = Command::new(meadow())
                .arg("build")
                .arg("--release")
                .arg(&dir)
                .output()
                .map_err(|e| format!("no meadow to build front/ with ({e})"))?;
            if !out.status.success() {
                return Err(format!("front/ did not build:\n{}", String::from_utf8_lossy(&out.stderr)));
            }
            Ok(dir.join("target/release/native/silo").join(format!("Idyll{}", std::env::consts::EXE_SUFFIX)))
        })
        .clone()
}

/// Every program of the corpora, with a name for it.
fn corpus() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, text) in [
        ("handwritten", include_str!("corpus/handwritten.txt")),
        ("tutorial", include_str!("corpus/tutorial.txt")),
    ] {
        for (i, src) in text.replace("\r\n", "\n").split("\n%%%\n").enumerate() {
            out.push((format!("{name}-{i:04}"), src.to_string()));
        }
    }
    out
}

/// What `front/`'s `pass` writes for each program: its programs written to
/// files of a directory of the test's own, and `Idyll <pass> <files>` run on
/// them once, its output cut at each `== <file>` line.
fn dumps(pass: &str, programs: &[(String, String)]) -> Result<Vec<String>, String> {
    let exe = front()?;
    let dir = std::env::temp_dir().join(format!("idyll-front-{pass}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let files: Vec<PathBuf> = programs
        .iter()
        .map(|(name, src)| {
            let f = dir.join(format!("{name}.idl"));
            std::fs::write(&f, src).map(|()| f)
        })
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let out = Command::new(&exe).arg(pass).args(&files).output().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&dir);
    if !out.status.success() {
        return Err(format!("front/ failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    // A program's own text can have `== ` in it, so a dump starts only at the
    // line that names the next program's file.
    let text = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    let mut parts: Vec<String> = Vec::new();
    for line in text.lines() {
        let next = programs.get(parts.len()).map(|(name, _)| format!("{name}.idl"));
        if line.starts_with("== ") && next.is_some_and(|f| line.ends_with(&f)) {
            parts.push(String::new());
        } else if let Some(part) = parts.last_mut() {
            if !part.is_empty() {
                part.push('\n');
            }
            part.push_str(line);
        }
    }
    if parts.len() != programs.len() {
        return Err(format!("front/ wrote {} dumps for {} programs", parts.len(), programs.len()));
    }
    Ok(parts)
}

/// The tokens this compiler's lexer and layout make of `src`, as `front/`
/// writes them: each as the text it covers, the virtual ones as «{» «;» «}»,
/// a space between each two.
fn tokens(src: &str) -> String {
    let lexed = idyllic_lexer::tokenize(src);
    let laid = idyllic_layout::layout(&LineIndex::new(src), &lexed.tokens);
    laid.tokens
        .iter()
        .map(|t| match t.node {
            Token::VLBrace => "«{»".to_string(),
            Token::VSemi => "«;»".to_string(),
            Token::VRBrace => "«}»".to_string(),
            _ => src[t.span.start..t.span.end].to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Where two dumps first differ, for a report.
fn first_difference(ours: &str, theirs: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (ours.split(' ').collect(), theirs.split(' ').collect());
    let at = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let around = |v: &[&str]| v[at.saturating_sub(4)..(at + 4).min(v.len())].join(" ");
    format!("at token {at}: here `{}`, front/ `{}`", around(&a), around(&b))
}

/// Compare `front/`'s dump of each program with this compiler's.
fn agree(pass: &str, ours: impl Fn(&str) -> String) {
    let programs = corpus();
    let theirs = match dumps(pass, &programs) {
        Ok(d) => d,
        Err(why) if why.starts_with("no meadow") => {
            eprintln!("{why}: front/ was not checked");
            return;
        }
        Err(why) => panic!("{why}"),
    };
    let differ: Vec<String> = programs
        .iter()
        .zip(&theirs)
        .filter_map(|((name, src), theirs)| {
            let ours = ours(src);
            (ours != *theirs).then(|| format!("{name}: {}", first_difference(&ours, theirs)))
        })
        .collect();
    assert!(
        differ.is_empty(),
        "{} of {} programs differ:\n{}",
        differ.len(),
        programs.len(),
        differ.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn the_tokens_and_their_layout_agree() {
    agree("tokens", tokens);
}
