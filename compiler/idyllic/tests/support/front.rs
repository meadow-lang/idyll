//! The differential harness: `front/`, the compiler written in Meadow, run
//! on every program of the corpus, its dumps compared with what this
//! compiler makes of the same programs, written the same way. Shared by the
//! compiler's pass-by-pass tests (`tests/front.rs`) and the reference
//! interpreter's (`idyll-eval/tests/front.rs`), each of which declares the
//! `tutorial_examples` module this reads the tutorial with.

use idyllic_span::LineIndex;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use super::tutorial_examples;

/// The repository: the nearest directory up from the test's package with
/// the tutorial in it.
pub fn repo() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize().expect("the package");
    let found = here.ancestors().find(|d| d.join("TUTORIAL.md").is_file()).expect("the repository").to_path_buf();
    // not Windows' verbatim form (`\\?\C:\..`), in which a `/` written onto
    // the path is no separator, as front/ writes them
    match found.to_string_lossy().strip_prefix(r"\\?\") {
        Some(plain) => PathBuf::from(plain),
        None => found,
    }
}

pub fn meadow() -> PathBuf {
    std::env::var_os("IDYLL_FRONT_MEADOW").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("meadow"))
}

/// `front/`, built natively, once: its executable, or why there is none.
pub fn front() -> Result<PathBuf, String> {
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

/// Every program of the corpora, with a name for it; the prelude; and every
/// `.idl` file of the repository (the standard library, the examples, the
/// benchmarks).
pub fn corpus() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, text) in [
        ("handwritten", include_str!("../corpus/handwritten.txt")),
        ("exhaustive", include_str!("../corpus/exhaustive.txt")),
        ("tutorial", include_str!("../corpus/tutorial.txt")),
    ] {
        for (i, src) in text.replace("\r\n", "\n").split("\n%%%\n").enumerate() {
            out.push((format!("{name}-{i:04}"), src.to_string()));
        }
    }
    out.push(("prelude".to_string(), idyllic::pipeline::PRELUDE.to_string()));
    // every entry of the tutorial's sessions, as the program the session has
    // made by then
    let markdown = include_str!("../../../../TUTORIAL.md").replace("\r\n", "\n");
    let opts = idyllic::pipeline::Options { debug: false, style: idyllic_diagnostics::OutputStyle::PLAIN, live: false };
    let mut session = idyllic::pipeline::Session::default();
    let mut block = 0;
    for ex in tutorial_examples::examples(&markdown) {
        if ex.block != block {
            session.reset();
            block = ex.block;
        }
        let entered = session.enter_in(&ex.source, &opts, idyllic::pipeline::Mode::Lower);
        out.push((format!("tutorial-md-{:04}", ex.line), entered.source));
    }
    let repo = repo();
    let mut files = Vec::new();
    for dir in ["lib", "examples", "bench"] {
        idl_files(&repo.join(dir), &mut files);
    }
    files.sort();
    for f in files {
        let name = f.strip_prefix(&repo).unwrap_or(&f).to_string_lossy().replace(['/', '\\'], "-");
        let text = std::fs::read_to_string(&f).expect("a source file").replace("\r\n", "\n");
        out.push((name.trim_end_matches(".idl").to_string(), text));
    }
    out
}

pub fn idl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "target" && n != "build") {
                idl_files(&path, out);
            }
        } else if path.extension().is_some_and(|x| x == "idl") {
            out.push(path);
        }
    }
}

/// What `front/` writes for each program, run as `Idyll <args> <files>`:
/// its programs written to files of a directory of the test's own (with
/// `extra` files beside them, which `args` may name), and front/ run on them
/// once, its output cut at each `== <file>` line.
pub fn dumps(args: &[&str], extra: &[(&str, &str)], programs: &[(String, String)]) -> Result<Vec<String>, String> {
    let exe = front()?;
    let dir = std::env::temp_dir().join(format!("idyll-front-{}-{}", args[0], std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (name, text) in extra {
        std::fs::write(dir.join(name), text).map_err(|e| e.to_string())?;
    }
    let files: Vec<String> = programs
        .iter()
        .map(|(name, src)| {
            let f = format!("{name}.idl");
            std::fs::write(dir.join(&f), src).map(|()| f)
        })
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    // run where the files are, a few hundred at a time: a command line has a
    // length it may not pass
    let mut text = String::new();
    for batch in files.chunks(200) {
        let out = Command::new(&exe).args(args).args(batch).current_dir(&dir).output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(format!("front/ failed:\n{}", String::from_utf8_lossy(&out.stderr)));
        }
        text.push_str(&String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"));
    }
    let _ = std::fs::remove_dir_all(&dir);
    // A program's own text can have `== ` in it, so a dump starts only at the
    // line that names the next program's file.
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

/// Where two dumps first differ, for a report: the line, for a dump of
/// several, or the token.
pub fn first_difference(ours: &str, theirs: &str) -> String {
    if ours.contains('\n') || theirs.contains('\n') {
        let (a, b): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), theirs.lines().collect());
        let at = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
        let line = |v: &[&str]| v.get(at).copied().unwrap_or("(nothing)").to_string();
        return format!("at line {at}:\n    here    {}\n    front/  {}", line(&a), line(&b));
    }
    let (a, b): (Vec<&str>, Vec<&str>) = (ours.split(' ').collect(), theirs.split(' ').collect());
    let at = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let around = |v: &[&str]| v[at.saturating_sub(4)..(at + 4).min(v.len())].join(" ");
    format!("at token {at}: here `{}`, front/ `{}`", around(&a), around(&b))
}

/// Compare `front/`'s dump of each program with this compiler's.
pub fn agree(pass: &str, ours: impl Fn(&str) -> String) {
    agree_with(&[pass], &[], ours, |d| d.to_string());
}

/// The same, with `front/` run as `Idyll <args>`, `extra` files beside the
/// programs, and both dumps put through `canon` before they are compared.
pub fn agree_with(args: &[&str], extra: &[(&str, &str)], ours: impl Fn(&str) -> String, canon: impl Fn(&str) -> String) {
    let pass = args[0];
    let programs = corpus();
    let theirs = match dumps(args, extra, &programs) {
        Ok(d) => d,
        Err(why) if why.starts_with("no meadow") => {
            eprintln!("{why}: front/ was not checked");
            return;
        }
        Err(why) => panic!("{why}"),
    };
    // a program that uses what a slice of front/ does not handle yet is
    // counted, not compared
    let unsupported = theirs.iter().filter(|d| d.starts_with("(unsupported")).count();
    let differ: Vec<String> = programs
        .iter()
        .zip(&theirs)
        .filter(|(_, theirs)| !theirs.starts_with("(unsupported"))
        .filter_map(|((name, src), theirs)| {
            let (ours, theirs) = (canon(&ours(src)), canon(theirs));
            // `IDYLL_FRONT_SHOW=handwritten-0003` shows both dumps of a
            // program (several, separated by commas)
            if std::env::var("IDYLL_FRONT_SHOW").is_ok_and(|v| v.split(',').any(|n| n == name)) {
                eprintln!("--- {name}, {pass}, here:\n{ours}\n--- front/:\n{theirs}");
            }
            (ours != theirs).then(|| format!("{name}: {}", first_difference(&ours, &theirs)))
        })
        .collect();
    let errors = theirs.iter().filter(|d| *d == "(error)").count();
    let idle = theirs.iter().filter(|d| *d == "(no expression)").count();
    let idle = if idle > 0 { format!(", {idle} with nothing to run") } else { String::new() };
    let unlowered = theirs.iter().filter(|d| d.starts_with("(not lowered")).count();
    let idle = if unlowered > 0 { format!("{idle}, {unlowered} not lowered by either") } else { idle };
    eprintln!("{pass}: {} programs, {errors} of them an error to front/, {unsupported} not handled by it yet{idle}", programs.len());
    assert!(
        differ.is_empty(),
        "{} of {} programs differ:\n{}",
        differ.len(),
        programs.len(),
        differ.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// `src` parsed, if the lexer, the layout and the parser have nothing to say.
pub fn program(src: &str) -> Option<idyllic_ast::Program> {
    let lexed = idyllic_lexer::tokenize(src);
    let laid = idyllic_layout::layout(&LineIndex::new(src), &lexed.tokens);
    if !lexed.diagnostics.is_empty() || !laid.errors.is_empty() {
        return None;
    }
    idyllic_parser::parse(src.len(), &laid.tokens).ok()
}

/// Each `@123` numbered by where its number first appears: `@0`, `@1`, ...
pub fn renumbered(dump: &str) -> String {
    renumbered_by(dump, '@')
}

/// The same, for the numbers after `mark`.
pub fn renumbered_by(dump: &str, mark: char) -> String {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut out = String::with_capacity(dump.len());
    let mut chars = dump.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == mark {
            let mut digits = String::new();
            while let Some(d) = chars.peek().copied().filter(char::is_ascii_digit) {
                digits.push(d);
                chars.next();
            }
            if !digits.is_empty() {
                let n = seen.len();
                out.push_str(&seen.entry(digits).or_insert(n).to_string());
            }
        }
    }
    out
}

