//! Shared helpers for the build-system tests.
//!
//! A package shows a value by ending with an expression, which a build runs
//! as its result -- the REPL's answer by another name. A program's `main` is
//! `() -> () ! Eff`, and is compared by what it printed.

#![allow(dead_code)]

use idyll::pipeline::{BuildOutput, Reported};
use idyllic::pipeline::{self, Evaluation, Options, Output};
use idyllic_diagnostics::OutputStyle;
use std::path::{Component, Path, PathBuf};

/// Quiet and captured: what a program prints is kept in `Output::printed`.
pub const OPTS: Options = Options { debug: false, style: OutputStyle::PLAIN, live: false };

/// Everything a build said: why it could not start, then each package's
/// diagnostics, each with what its first label says.
pub fn said(out: &BuildOutput) -> Vec<String> {
    out.problem.iter().map(|p| p.msg.clone()).chain(out.reported.iter().flat_map(messages)).collect()
}

/// A package's diagnostics, as [`said`] shows them.
pub fn messages(r: &Reported) -> Vec<String> {
    r.diagnostics
        .iter()
        .map(|d| match d.labels.first() {
            Some(l) => format!("{}: {}", d.message, l.message),
            None => d.message.clone(),
        })
        .collect()
}

/// Run a built program on the machine.
pub fn run(out: &BuildOutput) -> Output {
    assert!(out.errors() == 0, "{:?}", said(out));
    assert!(out.runnable(), "nothing to run");
    pipeline::run_compiled(&out.packages(), &OPTS, Vec::new())
}

/// The value of a built program's trailing expression.
pub fn value(out: &BuildOutput) -> String {
    let ran = run(out);
    match ran.evaluation {
        Some(Evaluation::Value(v)) => v,
        other => panic!("no value: {other:?}; {:?}", ran.diagnostics),
    }
}

/// What a built program printed.
pub fn printed(out: &BuildOutput) -> String {
    let ran = run(out);
    assert!(ran.diagnostics.is_empty(), "{:?}", ran.diagnostics);
    ran.printed
}

/// Run `f` on a thread with the stack the `idyll` command gives the compiler,
/// which walks a program's structure recursively.
pub fn on_big_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        let worker = std::thread::Builder::new().stack_size(256 << 20).spawn_scoped(s, f).expect("a thread");
        worker.join().unwrap_or_else(|e| std::panic::resume_unwind(e))
    })
}

/// Write `text` to `path`, making the directories it needs.
pub fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

// --- paths, spelled exactly ------------------------------------------------------

/// Does `path` name what is on disk **exactly**, letter for letter?
///
/// macOS and Windows find `layers` where the directory is `Layers`; Linux
/// does not. A test that opens a directory through a path one letter off
/// therefore passes on the machine it was written on and fails everywhere
/// else -- so the tests ask this first, and fail the same way on every
/// platform. Each component is looked for in its parent's listing, where a
/// name is what it is whatever the filesystem would accept.
pub fn exact_case(path: &Path) -> Result<(), String> {
    let mut at =
        if path.is_absolute() { PathBuf::new() } else { std::env::current_dir().map_err(|e| e.to_string())? };
    for c in path.components() {
        match c {
            Component::Normal(name) => {
                let listed: Vec<std::ffi::OsString> = std::fs::read_dir(&at)
                    .map_err(|e| format!("{}: {e}", at.display()))?
                    .filter_map(|e| e.ok().map(|e| e.file_name()))
                    .collect();
                if !listed.iter().any(|n| n == name) {
                    let near = listed
                        .iter()
                        .find(|n| n.to_string_lossy().to_lowercase() == name.to_string_lossy().to_lowercase());
                    return Err(match near {
                        Some(n) => format!(
                            "`{}` is spelled `{}` on disk, in {}: this passes on a \
                             case-insensitive filesystem and fails on Linux",
                            name.to_string_lossy(),
                            n.to_string_lossy(),
                            at.display()
                        ),
                        None => format!("no `{}` in {}", name.to_string_lossy(), at.display()),
                    });
                }
                at.push(name);
            }
            Component::ParentDir => {
                at.pop();
            }
            Component::CurDir => {}
            other => at.push(other.as_os_str()),
        }
    }
    Ok(())
}

/// The packages Meadow's tests keep as fixtures, written out afresh under
/// `dir` -- rather than checked in, since building one writes its `target`
/// beside it.
pub fn fixtures(dir: &Path) {
    for (path, text) in [
        (
            "App/Idyll.toml",
            "[package]\nname = \"App\"\nversion = \"0.1.0\"\n\n[dependencies]\nUtil = { path = \"../Util\" }\n",
        ),
        (
            "App/src/Main.idl",
            "-- `App` depends on `Util`. A dependency's names are in scope only\n\
             -- where a `use` brings them.\n\n\
             use Util (double, scale, sum)\n\n\
             nums : List Int\nnums = #[1, 2, 3, 4, 5]\n\n\
             sum (scale 3 (Util.map double nums))\n",
        ),
        ("Util/Idyll.toml", "[package]\nname = \"Util\"\nversion = \"0.2.0\"\n"),
        (
            "Util/src/Lib.idl",
            "-- `Util`. Only `@pub` names are visible to dependents; a plain\n\
             -- `@pub(pkg)` would reach this package's other modules and stop there.\n\n\
             @pub double : Int -> Int\ndouble x = x * 2\n\n\
             @pub map : (f : [] (Int -> Int)) -> List Int -> List Int\nmap f xs = case xs of\n  Nil -> Nil\n  y :: ys -> f y :: map f ys\n\n\
             @pub scale : (factor : [] Int) -> List Int -> List Int\nscale factor xs = map (\\x -> x * factor) xs\n\n\
             @pub sum : List Int -> Int\nsum xs = case xs of\n  Nil -> 0\n  y :: ys -> y + sum ys\n\n\
             -- private helper: not exported, so `App` cannot see it\n\
             secret : Int -> Int\nsecret x = x + 1\n",
        ),
        ("Layers/Idyll.toml", "[package]\nname = \"Layers\"\nversion = \"0.1.0\"\n"),
        (
            "Layers/src/Alpha.idl",
            "use Layers.Zeta (halve, wrap)\n\n\
             -- Alphabetically first, but depends on `Zeta` -- which is what makes\n\
             -- this fixture interesting. Modules are discovered in filename order,\n\
             -- so without a dependency sort `Alpha` is resolved, checked and run\n\
             -- before the module it needs.\n\n\
             twentyOne : Int\ntwentyOne = halve 42\n\n\
             describe : Int -> Int\ndescribe n = wrap (halve n)\n",
        ),
        (
            "Layers/src/Main.idl",
            "-- Each module is its own namespace, so `Alpha`'s bindings arrive by\n\
             -- `use` -- written here with the package name in front.\n\
             use Layers.Alpha (describe, twentyOne)\n\n\
             describe 20 + twentyOne\n",
        ),
        (
            "Layers/src/Zeta.idl",
            "-- Alphabetically last, depended on by `Alpha`.\n\nhalve : Int -> Int\nhalve n = n / 2\n\nwrap : Int -> Int\nwrap n = n + 1\n",
        ),
    ] {
        write(&dir.join(path), text);
    }
}

/// The fixture `name` under `dir`, which has to be spelled as it is on disk.
pub fn fixture(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    if let Err(e) = exact_case(&path) {
        panic!("{e}");
    }
    path
}
