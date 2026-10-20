//! Running a program. Idyll has no runtime of its own: a program is lowered
//! to Cut (`idyllic-cut`, `docs/CUT.md` in the Meadow repository), and
//! Meadow runs that, through `meadow cut`: on Glade, its bytecode, which is
//! what the REPL and a debug build run on; or on Silo, as native code, which
//! is what a release build makes.
//!
//! Meadow writes a program's answer right after what the program printed,
//! so the Cut that is run puts [`ANSWER_MARK`] in front of its answer (see
//! `idyllic_cut::lower::lower_marked`), and what comes after the last mark
//! is the answer.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// What a program's answer is written after.
pub const ANSWER_MARK: char = '\u{1e}';

/// Which of Meadow's runtimes runs a program.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Runtime {
    /// its bytecode, at once: the REPL's, and a debug build's
    #[default]
    Glade,
    /// native code, built first: a release build's
    Silo,
}

impl Runtime {
    pub fn name(self) -> &'static str {
        match self {
            Runtime::Glade => "glade",
            Runtime::Silo => "silo",
        }
    }
}

/// How a program is run.
#[derive(Clone, Debug, Default)]
pub struct Running {
    pub runtime: Runtime,
    /// what its `argv` answers
    pub args: Vec<String>,
    /// talking to the real standard streams, as the REPL's and `idyll run`'s
    /// do; otherwise with no input, and what it writes kept (tests)
    pub live: bool,
    /// how long it may run before it is stopped; `None`, as long as it likes
    pub timeout: Option<Duration>,
}

/// What running a program came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It finished, with this answer written out, if it has one to show.
    Answered(Option<String>),
    /// It ended itself, `exit n`.
    Exited(i32),
    /// An error of its own stopped it (dividing by zero, an index outside
    /// an array), as the runtime said.
    Stopped(String),
    /// It was still running when its time was up.
    TimedOut(Duration),
    /// Meadow could not run it: not a fault of the program's.
    Failed(String),
}

/// A program run: what it came to, and what it wrote to standard output and
/// standard error, if that was kept.
#[derive(Clone, Debug)]
pub struct Ran {
    pub outcome: Outcome,
    pub printed: String,
    pub errors: String,
}

/// The `meadow` to run: `IDYLL_MEADOW`, or the one on the path.
pub fn meadow() -> PathBuf {
    std::env::var_os("IDYLL_MEADOW").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("meadow"))
}

/// A directory of its own for one program.
fn scratch() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("idyll-cut-{}-{n}", std::process::id()))
}

const NO_MEADOW: &str = "Idyll runs programs on Meadow's runtimes, so it needs `meadow` (install it with meadowup, or name one with IDYLL_MEADOW)";

/// Whether a line Meadow writes to standard error is its own account of a
/// build (Silo's `Lowering ..`, `Linking ..`), not the program's.
fn build_line(line: &str) -> bool {
    let t = line.trim_start();
    line.starts_with(' ')
        && ["Lowering ", "Linking ", "Reusing ", "Compiling ", "Finished ", "Building "].iter().any(|w| t.starts_with(w))
}

/// Run a program of Cut.
pub fn run(cut: &str, how: &Running) -> Ran {
    let failed = |why: String| Ran { outcome: Outcome::Failed(why), printed: String::new(), errors: String::new() };
    let dir = scratch();
    let file = dir.join("program.cut");
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&file, cut)) {
        return failed(format!("could not write the program's Cut: {e}"));
    }
    let mut cmd = Command::new(meadow());
    cmd.arg("cut");
    if how.runtime == Runtime::Silo {
        cmd.args(["--runtime", "silo"]);
    }
    cmd.arg(&file);
    if !how.args.is_empty() {
        cmd.arg("--").args(&how.args);
    }
    cmd.current_dir(&dir);
    cmd.stdin(if how.live { Stdio::inherit() } else { Stdio::null() });
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            return failed(format!("{NO_MEADOW} ({e})"));
        }
    };
    // what it prints goes on as it comes, live, up to the answer's mark
    let mut stdout = child.stdout.take().expect("piped");
    let live = how.live;
    let out_reader = std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buf = [0u8; 4096];
        let mut answering = false;
        let mark = ANSWER_MARK.to_string();
        loop {
            let n = match stdout.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let chunk = &buf[..n];
            if live && !answering {
                let start = kept.len();
                kept.extend_from_slice(chunk);
                let text = &kept[start..];
                let shown = match text.windows(mark.len()).position(|w| w == mark.as_bytes()) {
                    Some(at) => {
                        answering = true;
                        &text[..at]
                    }
                    None => text,
                };
                let mut o = std::io::stdout();
                let _ = o.write_all(shown);
                let _ = o.flush();
            } else {
                kept.extend_from_slice(chunk);
            }
        }
        String::from_utf8_lossy(&kept).into_owned()
    });
    let mut stderr = child.stderr.take().expect("piped");
    let err_reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        if how.timeout.is_some_and(|t| started.elapsed() >= t) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let out = out_reader.join().unwrap_or_default();
    let err = err_reader.join().unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    // what it wrote, and its answer after the last mark
    let (printed, answer) = match out.rfind(ANSWER_MARK) {
        Some(at) => (out[..at].to_string(), Some(out[at + ANSWER_MARK.len_utf8()..].to_string())),
        None => (out, None),
    };
    // standard error without Meadow's account of a build
    let mut lines: Vec<&str> = err.lines().filter(|l| !build_line(l)).collect();
    let outcome = match status {
        None if how.timeout.is_some_and(|t| started.elapsed() >= t) => Outcome::TimedOut(how.timeout.expect("checked")),
        None => Outcome::Failed("meadow did not finish".into()),
        Some(s) if s.success() => Outcome::Answered(answer),
        Some(s) => {
            let code = s.code().unwrap_or(-1);
            // the runtime's own account of an error is its last line
            match lines.last().and_then(|l| l.strip_prefix("error: ")) {
                // (Meadow's, about the Cut, names the file)
                Some(why) if why.contains("program.cut") => Outcome::Failed(why.to_string()),
                Some(why) if code == 1 => {
                    let why = why.to_string();
                    lines.pop();
                    Outcome::Stopped(why)
                }
                _ => Outcome::Exited(code),
            }
        }
    };
    let mut errors = lines.join("\n");
    if !errors.is_empty() {
        errors.push('\n');
    }
    if live {
        eprint!("{errors}");
        return Ran { outcome, printed: String::new(), errors: String::new() };
    }
    Ran { outcome, printed, errors }
}

/// Build a program of Cut for `runtime` without running it, in `dir`, as
/// `name`: the executable Silo makes, or Glade's bytecode image. What a
/// front end's `build` asks for.
pub fn build(cut: &str, runtime: Runtime, dir: &Path, name: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!("{name}.cut"));
    std::fs::write(&file, cut).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(meadow());
    cmd.arg("cut");
    if runtime == Runtime::Silo {
        cmd.args(["--runtime", "silo"]);
    }
    let out = cmd.arg("--no-run").arg(&file).current_dir(dir).output().map_err(|e| format!("{NO_MEADOW} ({e})"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let why: Vec<&str> = err.lines().filter(|l| !build_line(l)).collect();
        return Err(why.join("\n"));
    }
    let made = match runtime {
        Runtime::Silo => dir.join("target/release/native/silo").join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
        Runtime::Glade => dir.join(format!("{name}.cut")),
    };
    Ok(made)
}

/// What `meadow cut --emit what` prints of a program of Cut: a form of it
/// to read, which nothing here parses (`docs/CUT.md`: the printed form is
/// not a contract).
fn emitted(cut: &str, what: &str) -> Result<String, String> {
    let dir = scratch();
    let file = dir.join("program.cut");
    std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&file, cut)).map_err(|e| e.to_string())?;
    let out = Command::new(meadow())
        .args(["cut", "--emit", what])
        .arg(&file)
        .current_dir(&dir)
        .output()
        .map_err(|e| format!("{NO_MEADOW} ({e})"));
    let _ = std::fs::remove_dir_all(&dir);
    let out = out?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err.lines().filter(|l| !build_line(l)).collect::<Vec<_>>().join("\n"))
    }
}

/// The AxCut Meadow lowers a program of Cut to: what Glade's bytecode and
/// Silo's native code are made from, each definition under its name.
pub fn axcut(cut: &str) -> Result<String, String> {
    emitted(cut, "axcut")
}

/// The LLVM IR a Silo build of a program of Cut compiles, for this machine.
pub fn llvm(cut: &str) -> Result<String, String> {
    emitted(cut, "llvm")
}
