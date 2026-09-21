use clap::Parser;
use idyllic_diagnostics::OutputStyle;
use idyllic::pipeline::{self, Options, Session};
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{
    Cmd, Completer, ConditionalEventHandler, Config, Editor, Event, EventContext, EventHandler, Helper, Highlighter,
    Hinter, KeyEvent, RepeatCount,
};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HISTORY_FILE: &str = ".repl_history";

const BANNER: &str = r"    ____    __      ____
   /  _/___/ /_  __/ / /
   / // __  / / / / / /
 _/ // /_/ / /_/ / / /
/___/\__,_/\__, /_/_/
          /____/
Welcome to the Idyll REPL!
Type :quit or :q to exit (or press Ctrl-C), :reset to forget your definitions.";

/// The Idyll compiler.  With no file, starts the REPL.
#[derive(Parser, Debug)]
#[command(name = "idyllic", version, about)]
struct Cli {
    /// Source file to check.  Omit to start the REPL.
    file: Option<PathBuf>,

    /// Dump the HIR (after renaming and the SCC pass).
    #[arg(long)]
    debug: bool,

    /// Never colour output (the NO_COLOR environment variable also works).
    #[arg(long)]
    no_color: bool,

    /// Draw diagnostics with ASCII instead of box-drawing characters.
    #[arg(long)]
    ascii: bool,

    /// Print the AxCut each program is lowered to, and what running it did
    /// with memory, before its result.  When compiling: the AxCut that is
    /// compiled, which has been inlined.
    #[arg(long)]
    axcut: bool,

    /// Write the program as LLVM IR to this file.
    #[arg(long, value_name = "FILE")]
    emit_llvm: Option<PathBuf>,

    /// Compile the program to a native executable (needs clang and lld).
    #[arg(short, long, value_name = "EXE")]
    output: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let mut style = OutputStyle::detect();
    style.color &= !cli.no_color;
    style.unicode &= !cli.ascii;
    let opts = Options { debug: cli.debug, style };

    match &cli.file {
        Some(path) if cli.emit_llvm.is_some() || cli.output.is_some() => compile_file(path, &opts, &cli),
        Some(path) => check_file(path, &opts, cli.axcut),
        None => repl(&opts, cli.axcut),
    }
}

/// Check a program and compile it: to LLVM IR, and with `-o`, on through
/// clang to an executable.  The runtime is compiled along with it, with
/// link-time optimisation, so that allocation is inlined into the program.
fn compile_file(path: &Path, opts: &Options, cli: &Cli) -> ExitCode {
    let src = match std::fs::read_to_string(path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let out = pipeline::run(&src, opts);
    let name = path.display().to_string();
    eprint!("{}", idyllic_diagnostics::render(&out.diagnostics, &name, &src, opts.style));
    let fail = |why: String| {
        eprintln!("error: {why}");
        ExitCode::FAILURE
    };
    if out.has_errors() {
        return ExitCode::FAILURE;
    }
    let Some(program) = &out.native else { return fail("there is no expression to compile: the program only declares".into()) };
    if cli.axcut {
        println!("{program}
");
    }
    let ir = match idyllic_llvm::emit(program, out.result_kind) {
        Ok(ir) => ir,
        Err(why) => return fail(why),
    };
    if let Some(ll) = &cli.emit_llvm {
        if let Err(e) = std::fs::write(ll, &ir) {
            return fail(format!("cannot write {}: {e}", ll.display()));
        }
    }
    let Some(exe) = &cli.output else { return ExitCode::SUCCESS };
    let dir = std::env::temp_dir().join(format!("idyllic-{}", std::process::id()));
    let (ll, rt) = (dir.join("program.ll"), dir.join("idyll_rt.c"));
    let written = std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(&ll, &ir))
        .and_then(|()| std::fs::write(&rt, idyllic_llvm::RUNTIME));
    if let Err(e) = written {
        return fail(format!("cannot write to {}: {e}", dir.display()));
    }
    let status = std::process::Command::new("clang")
        .args(idyllic_llvm::clang_arguments())
        .arg(&ll)
        .arg(&rt)
        .arg("-o")
        .arg(exe)
        .status();
    let _ = std::fs::remove_dir_all(&dir);
    match status {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => fail(format!("clang failed ({s})")),
        Err(e) => fail(format!("cannot run clang: {e}")),
    }
}

/// The lowered program and what running it did with memory, when `--axcut`
/// asked to see them.
fn print_axcut(out: &pipeline::Output, wanted: bool) {
    if let Some(program) = out.axcut.as_ref().filter(|_| wanted) {
        println!("{program}\n");
        if let Some(memory) = &out.memory {
            println!("{memory}\n");
        }
    }
}

fn check_file(path: &Path, opts: &Options, axcut: bool) -> ExitCode {
    let src = match std::fs::read_to_string(path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let out = pipeline::run(&src, opts);
    print_axcut(&out, axcut);
    println!("{}", out.display(&path.display().to_string(), &src, opts.style));
    if out.has_errors() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

// ---------------------------------------------------------------------------
// REPL
// ---------------------------------------------------------------------------

/// Layout-sensitive programs span several lines, so an entry is one
/// multi-line buffer rather than one line: Enter inserts a newline until the
/// entry is submitted by a blank line.  That keeps the whole entry editable
/// and makes history recall entries whole.
#[derive(Helper, Completer, Hinter, Highlighter)]
struct ReplHelper;

impl Validator for ReplHelper {
    fn validate(&self, ctx: &mut ValidationContext) -> rustyline::Result<ValidationResult> {
        let input = ctx.input();
        let last_line = input.rsplit('\n').next().unwrap_or("");
        // An empty buffer and REPL commands are submitted right away;
        // anything else is submitted by a blank last line.
        let complete =
            input.trim().is_empty() || is_command(input) || last_line.trim().is_empty();
        Ok(if complete { ValidationResult::Valid(None) } else { ValidationResult::Incomplete })
    }
}

/// The way out: abandon the entry being typed, or, when there is none, leave.
/// Bound to Ctrl-C and its relatives, so that whichever "get me out of here"
/// key your hands know does what you meant — out of a half-typed multi-line
/// entry first, then out of the REPL.
struct Escape;

impl ConditionalEventHandler for Escape {
    fn handle(&self, _: &Event, _: RepeatCount, _: bool, ctx: &EventContext) -> Option<Cmd> {
        // `EndOfFile` only ends the session on an empty line, so ask for it
        // only then
        Some(if ctx.line().is_empty() { Cmd::EndOfFile } else { Cmd::Interrupt })
    }
}

/// Ctrl-C everywhere, Ctrl-\ (the terminal's "quit"), and on Windows Ctrl-Z,
/// which is end-of-input there.  (On Unix Ctrl-Z suspends, and is left
/// alone.)  Ctrl-D already leaves from an empty prompt.  Escape itself is not
/// bound: it is the prefix of every Alt- key.
fn escape_keys() -> Vec<KeyEvent> {
    let mut keys = vec![KeyEvent::ctrl('C'), KeyEvent::ctrl('\\')];
    if cfg!(windows) {
        keys.push(KeyEvent::ctrl('Z'));
    }
    keys
}

/// A single line starting with ':'.
fn is_command(input: &str) -> bool {
    let input = input.trim();
    input.starts_with(':') && !input.contains('\n')
}

fn repl(opts: &Options, axcut: bool) -> ExitCode {
    let editor = Config::builder()
        .max_history_size(1000)
        .map(|b| b.build())
        .and_then(Editor::<ReplHelper, DefaultHistory>::with_config);
    let mut rl = match editor {
        Ok(rl) => rl,
        Err(e) => {
            eprintln!("error: cannot start the line editor: {e}");
            return ExitCode::FAILURE;
        }
    };
    rl.set_helper(Some(ReplHelper));
    for key in escape_keys() {
        rl.bind_sequence(key, EventHandler::Conditional(Box::new(Escape)));
    }
    // a missing history file is the normal first-run case
    let _ = rl.load_history(HISTORY_FILE);
    println!("{BANNER}");

    // declarations stay in scope for later entries
    let mut session = Session::default();

    loop {
        match rl.readline("> ") {
            Ok(raw) => {
                let entry = raw.trim_end();
                if entry.trim().is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(entry);
                let _ = rl.append_history(HISTORY_FILE);
                match entry.trim() {
                    ":quit" | ":q" => break,
                    ":reset" => {
                        session.reset();
                        continue;
                    }
                    _ => {}
                }
                let entered = session.enter(entry, opts);
                print_axcut(&entered.output, axcut);
                println!("{}", entered.output.display("<interactive>", &entered.source, opts.style));
            }
            // an escape key with an entry half typed discards it; with
            // nothing typed it arrives as Eof, like Ctrl-D, and leaves
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("readline error: {e}");
                break;
            }
        }
    }
    ExitCode::SUCCESS
}
