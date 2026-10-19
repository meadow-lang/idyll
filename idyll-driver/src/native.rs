//! **Native code**: a linked program, built by Silo, Meadow's native
//! runtime, into an executable. The program's Cut is handed to `meadow cut
//! --runtime silo --no-run`, in a directory beside the executable that
//! Silo keeps its objects in, so that a build after a small change reuses
//! most of them. An executable is known by the fingerprint of the package
//! it was made from, written beside it: a build of an unchanged package
//! keeps the one it has.

use idyllic::pipeline::Output;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Why there is no executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// There is no `meadow` to build it with: Glade can still run it, if
    /// `meadow` is found then.
    Unavailable(String),
    /// Building it failed.
    Failed(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Unavailable(why) | Failure::Failed(why) => f.write_str(why),
        }
    }
}

/// The Cut of a linked program.
pub fn cut(linked: &Output) -> Result<&str, Failure> {
    linked.cut.as_deref().ok_or_else(|| {
        let why = if linked.not_lowered.is_empty() { "the program only declares".to_string() } else { linked.not_lowered.join("; ") };
        Failure::Failed(format!("there is nothing to build: {why}"))
    })
}

/// Whether `exe` was made from the package with `fingerprint`.
pub fn fresh(exe: &Path, fingerprint: u64) -> bool {
    exe.is_file() && std::fs::read_to_string(stamp(exe)).is_ok_and(|s| s.trim() == format!("{fingerprint:016x}"))
}

fn stamp(exe: &Path) -> PathBuf {
    exe.with_extension("fingerprint")
}

/// Build `cut` into the executable `exe`, and note that it was made from
/// the package with `fingerprint`.
pub fn compile(cut: &str, exe: &Path, fingerprint: Option<u64>) -> Result<(), Failure> {
    let dir = exe.parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let stem = exe.file_stem().map_or("program".into(), |s| s.to_string_lossy().into_owned());
    let _ = std::fs::remove_file(stamp(exe));
    // Silo's own directory, kept between builds
    let work = dir.join("silo");
    let made = idyllic::meadow::build(cut, idyllic::meadow::Runtime::Silo, &work, &stem).map_err(|why| {
        if why.contains("needs `meadow`") { Failure::Unavailable(why) } else { Failure::Failed(why) }
    })?;
    std::fs::copy(&made, exe).map_err(|e| Failure::Failed(format!("could not write {}: {e}", exe.display())))?;
    if let Some(print) = fingerprint {
        let _ = std::fs::write(stamp(exe), format!("{print:016x}\n"));
    }
    Ok(())
}

/// Run the executable `exe` with `args`, answering its exit code.
pub fn run(exe: &Path, args: &[String]) -> Result<i32, String> {
    let status = Command::new(exe).args(args).status().map_err(|e| format!("could not run {}: {e}", exe.display()))?;
    Ok(status.code().unwrap_or(1))
}
