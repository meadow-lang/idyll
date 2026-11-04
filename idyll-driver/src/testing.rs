//! `idyll test`'s running: a package's tests, built into one program by
//! `Std.Test`'s runner, each run in a process of its own, as Meadow runs its
//! native tests. The program is asked for its tests' names (`names`), then
//! run once a test with that test's number for its argument, several at a
//! time. A test stopped by an error of its own (dividing by zero) fails
//! alone, and what a test printed is shown only if it failed.

use idyllic::meadow::{self, Outcome, Running};

/// How one test went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Told {
    pub name: String,
    pub passed: bool,
    /// what it printed
    pub printed: String,
    /// why it failed: its `fail`'s message, or the runtime's account of the
    /// error that stopped it
    pub said: String,
}

/// Run every test of `cut`, a program of tests, each alone, `threads` at a
/// time: how each went, in the order they were written, or why they could
/// not be run.
pub fn run_each(cut: &str, threads: usize) -> Result<Vec<Told>, String> {
    let listed = meadow::run(cut, &asked("names"));
    match listed.outcome {
        Outcome::Answered(_) => {}
        other => return Err(format!("the tests could not be listed: {}", account(&other, &listed.errors))),
    }
    let names: Vec<String> = listed.printed.lines().map(|l| l.trim_end().to_string()).filter(|l| !l.is_empty()).collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let slots: Vec<std::sync::Mutex<Option<Told>>> = names.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..threads.clamp(1, names.len().max(1)) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(name) = names.get(i) else { return };
                let ran = meadow::run(cut, &asked(&i.to_string()));
                let passed = matches!(ran.outcome, Outcome::Answered(_));
                let said = if passed { String::new() } else { account(&ran.outcome, &ran.errors) };
                let told = Told { name: name.clone(), passed, printed: ran.printed, said };
                *slots[i].lock().unwrap_or_else(|p| p.into_inner()) = Some(told);
            });
        }
    });
    Ok(slots.into_iter().map(|s| s.into_inner().unwrap_or_else(|p| p.into_inner()).expect("every test ran")).collect())
}

/// The program, asked for `what` by its first argument.
fn asked(what: &str) -> Running {
    Running { args: vec![what.to_string()], ..Running::default() }
}

/// Why a run did not finish: what the test said on standard error when it
/// failed itself, else what stopped it.
fn account(outcome: &Outcome, errors: &str) -> String {
    let said = errors.trim_end();
    match outcome {
        Outcome::Exited(_) if !said.is_empty() => said.to_string(),
        Outcome::Exited(code) => format!("the test exited with status {code}"),
        Outcome::Stopped(why) if said.is_empty() => why.clone(),
        Outcome::Stopped(why) => format!("{said}\n{why}"),
        Outcome::TimedOut(after) => format!("the test was still running after {after:?}"),
        Outcome::Failed(why) => why.clone(),
        Outcome::Answered(_) => String::new(),
    }
}

/// What `idyll test` prints of how the tests went, as `meadow test` says
/// it: a line a test, then each failure with what its test printed, then
/// the total.
pub fn report(told: &[Told]) -> String {
    let n = told.len();
    let mut out = format!("running {n} {}\n", if n == 1 { "test" } else { "tests" });
    for t in told {
        out.push_str(&format!("test {} ... {}\n", t.name, if t.passed { "ok" } else { "FAILED" }));
    }
    let failed: Vec<&Told> = told.iter().filter(|t| !t.passed).collect();
    if !failed.is_empty() {
        out.push_str("\nfailures:\n");
        for t in &failed {
            out.push_str(&format!("\n---- {} ----\n{}", t.name, t.printed));
            if !t.printed.is_empty() && !t.printed.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!("{}\n", t.said));
        }
    }
    let verdict = if failed.is_empty() { "ok" } else { "FAILED" };
    out.push_str(&format!("\ntest result: {verdict}. {} passed; {} failed\n", n - failed.len(), failed.len()));
    out
}
