//! **Native code**: a linked program, compiled by LLVM into an executable.
//!
//! The program's AxCut (inlined, see `idyllic_seq::inline`) is emitted as
//! LLVM IR, and clang compiles it together with the runtime, with link-time
//! optimisation, so that allocation is inlined into the program. An
//! executable is known by the fingerprint of the package it was made from,
//! written beside it: a build of an unchanged package keeps the one it has.

use idyllic::pipeline::Output;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Why there is no executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The program uses something native code cannot do yet (a handler,
    /// say), or there is no clang to compile it with: the machine can still
    /// run it.
    Unavailable(String),
    /// Compiling it failed.
    Failed(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Unavailable(why) | Failure::Failed(why) => f.write_str(why),
        }
    }
}

/// The LLVM IR of a linked program.
pub fn ir(linked: &Output) -> Result<String, Failure> {
    let program = linked
        .native
        .as_ref()
        .ok_or_else(|| Failure::Failed("there is no expression to compile: the program only declares".into()))?;
    idyllic_llvm::emit(program, linked.result_kind).map_err(Failure::Unavailable)
}

/// Whether `exe` was made from the package with `fingerprint`.
pub fn fresh(exe: &Path, fingerprint: u64) -> bool {
    exe.is_file() && std::fs::read_to_string(stamp(exe)).is_ok_and(|s| s.trim() == format!("{fingerprint:016x}"))
}

fn stamp(exe: &Path) -> PathBuf {
    exe.with_extension("fingerprint")
}

/// Compile `ir` into the executable `exe`, and note that it was made from
/// the package with `fingerprint`.
pub fn compile(ir: &str, exe: &Path, fingerprint: Option<u64>) -> Result<(), Failure> {
    let dir = exe.parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    std::fs::create_dir_all(&dir).map_err(|e| Failure::Failed(format!("could not create {}: {e}", dir.display())))?;
    let stem = exe.file_stem().map_or("program".into(), |s| s.to_string_lossy().into_owned());
    let (ll, rt) = (dir.join(format!("{stem}.ll")), dir.join("idyll_rt.c"));
    std::fs::write(&ll, ir)
        .and_then(|()| std::fs::write(&rt, idyllic_llvm::RUNTIME))
        .map_err(|e| Failure::Failed(format!("could not write to {}: {e}", dir.display())))?;
    let _ = std::fs::remove_file(stamp(exe));
    let status = Command::new("clang").args(idyllic_llvm::clang_arguments()).arg(&ll).arg(&rt).arg("-o").arg(exe).status();
    let _ = std::fs::remove_file(&rt);
    match status {
        Ok(s) if s.success() => {
            if let Some(print) = fingerprint {
                let _ = std::fs::write(stamp(exe), format!("{print:016x}\n"));
            }
            Ok(())
        }
        Ok(s) => Err(Failure::Failed(format!("clang failed ({s})"))),
        Err(e) => Err(Failure::Unavailable(format!("cannot run clang: {e}"))),
    }
}

/// Run the executable `exe` with `args`, answering its exit code.
pub fn run(exe: &Path, args: &[String]) -> Result<i32, String> {
    let status = Command::new(exe).args(args).status().map_err(|e| format!("could not run {}: {e}", exe.display()))?;
    Ok(status.code().unwrap_or(1))
}
