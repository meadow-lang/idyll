//! `front/`'s CEK machine (`front/src/Cek.mw`), the reference interpreter
//! written again in Meadow, held to this one: every program of the corpus
//! that checks is run by both, on the Core each checker made of it, and
//! must come to the same answer in the same number of steps, printing the
//! same things on the way, or stop in the same way.
//!
//! The harness is the compiler's (`compiler/idyllic/tests/support/front.rs`):
//! it builds `front/` with the `meadow` on the path, and passes, saying
//! so, if there is none.

#[path = "../../compiler/idyllic/tests/support/tutorial_examples.rs"]
#[allow(dead_code)]
mod tutorial_examples;

#[path = "../../compiler/idyllic/tests/support/front.rs"]
#[allow(dead_code)]
mod harness;

use idyll_eval::EvalError;
use idyllic_world::World;
use std::process::Command;

/// The steps each machine is given, as `tests/spec.rs` gives them.
const FUEL: u64 = 5_000_000;

#[test]
fn the_runs_agree() {
    let source = idyllic::pipeline::PRELUDE;
    let prelude = harness::program(source).expect("the prelude parses");
    harness::agree_with(
        &["run", "--prelude", "_prelude.txt"],
        &[("_prelude.txt", source)],
        |src| ran(&prelude, src),
        |d| d.to_string(),
    );
}

/// The Cut `front/` makes of each program, run on Glade with `meadow cut`,
/// must do what this interpreter says the program does: print the same, and
/// answer the same (the Cut's entry writes its answer as the REPL shows
/// it), or exit with the same status. A program whose answer depends on the
/// clock or randomness is left out; and where Meadow itself fails (a panic,
/// a crash), that is Meadow's bug, not front/'s, and is counted apart.
#[test]
fn front_cut_runs_as_the_reference_says() {
    cut_runs_on("glade");
}

/// The same on Silo, which compiles each program natively first.
#[test]
fn front_cut_runs_natively_as_the_reference_says() {
    cut_runs_on("silo");
}

fn cut_runs_on(runtime: &str) {
    let source = idyllic::pipeline::PRELUDE;
    let prelude = harness::program(source).expect("the prelude parses");
    let programs = harness::corpus();
    let cuts = match harness::dumps(&["cut", "--prelude", "_prelude.txt"], &[("_prelude.txt", source)], &programs) {
        Ok(d) => d,
        Err(why) if why.starts_with("no meadow") => {
            eprintln!("{why}: front/ was not checked");
            return;
        }
        Err(why) => panic!("{why}"),
    };
    let dir = std::env::temp_dir().join(format!("idyll-front-{runtime}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory for the Cut");
    // each program to run, with what it should print, exit with, and answer
    let mut jobs: Vec<(String, std::path::PathBuf, String, i32)> = Vec::new();
    for ((name, src), cut) in programs.iter().zip(&cuts) {
        if cut.starts_with('(') || ["perform idyll:Prelude/Random.", "perform idyll:Prelude/Time."].iter().any(|op| cut.contains(op)) {
            continue;
        }
        let Some((printed, answer, exit)) = expected(&prelude, src) else { continue };
        let file = dir.join(format!("{name}.cut"));
        std::fs::write(&file, cut).expect("the Cut can be written");
        jobs.push((name.clone(), file, format!("{printed}{}", answer.unwrap_or_default()), exit));
    }
    // a few at a time: each is a process of its own
    let results: Vec<(String, Result<(), String>, bool)> = std::thread::scope(|scope| {
        let chunks: Vec<_> = jobs.chunks(jobs.len().div_ceil(8).max(1)).collect();
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|(name, file, want, exit)| {
                            let mut cmd = Command::new(harness::meadow());
                            cmd.arg("cut");
                            if runtime != "glade" {
                                cmd.args(["--runtime", runtime]);
                            }
                            let Ok(out) = cmd.arg(file).output() else {
                                return (name.clone(), Err("meadow did not run".to_string()), false);
                            };
                            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                            let code = out.status.code().unwrap_or(-1);
                            // Meadow itself panicking, or crashing (Windows'
                            // access violation, a Unix signal)
                            let crashed = stderr.contains("panicked at") || code == -1073741819 || out.status.code().is_none() || code == 139;
                            if crashed && code != *exit {
                                return (name.clone(), Ok(()), true);
                            }
                            let got = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
                            // Silo says what it is building first
                            let same = if runtime == "glade" { got == *want } else { got.ends_with(want.as_str()) };
                            if same && code == *exit {
                                (name.clone(), Ok(()), false)
                            } else {
                                let why = format!("printed {got:?} and exited {code}; the reference: {want:?}, exit {exit}; {}", stderr.lines().next().unwrap_or(""));
                                (name.clone(), Err(why), false)
                            }
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("a runner")).collect()
    });
    let _ = std::fs::remove_dir_all(&dir);
    let crashed: Vec<&String> = results.iter().filter(|(_, _, c)| *c).map(|(n, _, _)| n).collect();
    let wrong: Vec<String> = results.iter().filter_map(|(n, r, _)| r.as_ref().err().map(|why| format!("{n}: {why}"))).collect();
    eprintln!(
        "{runtime}: {} programs run, {} as the reference says, {} where Meadow crashed ({})",
        results.len(),
        results.len() - crashed.len() - wrong.len(),
        crashed.len(),
        crashed.iter().take(12).map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
    );
    assert!(wrong.is_empty(), "{} programs run otherwise than the reference says:\n{}", wrong.len(), wrong.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
}

/// What a program does, as this interpreter runs it: what it prints, its
/// answer as the REPL shows it if it has one to show, and its exit status.
/// `None` if it does not check, has nothing to run, or stops otherwise than
/// with an answer or an exit.
fn expected(prelude: &idyllic_ast::Program, src: &str) -> Option<(String, Option<String>, i32)> {
    let p = harness::program(src)?;
    let result = idyllic_rename::rename_module(&p, prelude);
    if !result.errors.is_empty() {
        return None;
    }
    let module = idyllic_scc::scc_module(result.module);
    let checked = idyllic_typecheck::check_module(&module, result.spans.len(), false, &[]);
    if !checked.errors.is_empty() {
        return None;
    }
    let mut world = World::captured();
    let outcome = idyll_eval::evaluate_in(&checked, FUEL, &mut world)?;
    let (printed, _) = world.into_streams();
    match outcome {
        Ok(o) => Some((printed, o.answer, 0)),
        Err(EvalError::Exited(code)) => Some((printed, None, code)),
        Err(_) => None,
    }
}

/// A program run here, after the prelude, as `front/src/Main.mw` writes its
/// run: the answer as the REPL shows it (`-` for a function or a type) and
/// the steps it took, or how it stopped; then what it printed, and what it
/// wrote as errors.
fn ran(prelude: &idyllic_ast::Program, src: &str) -> String {
    let Some(p) = harness::program(src) else { return "(error)".to_string() };
    let result = idyllic_rename::rename_module(&p, prelude);
    if !result.errors.is_empty() {
        return "(error)".to_string();
    }
    let module = idyllic_scc::scc_module(result.module);
    let checked = idyllic_typecheck::check_module(&module, result.spans.len(), false, &[]);
    if !checked.errors.is_empty() {
        return "(error)".to_string();
    }
    let mut world = World::captured();
    let Some(outcome) = idyll_eval::evaluate_in(&checked, FUEL, &mut world) else {
        return "(no expression)".to_string();
    };
    let (printed, errors) = world.into_streams();
    let mut lines = match outcome {
        Ok(o) => vec![format!("answer: {}", o.answer.as_deref().unwrap_or("-")), format!("steps: {}", o.steps)],
        // what made it stuck is a compiler bug either way, said differently
        Err(EvalError::Stuck(_)) => vec!["stopped: stuck".to_string()],
        Err(e) => vec![format!("stopped: {e}")],
    };
    if !printed.is_empty() {
        lines.push(format!("printed: {printed:?}"));
    }
    if !errors.is_empty() {
        lines.push(format!("errors: {errors:?}"));
    }
    lines.join("\n")
}
