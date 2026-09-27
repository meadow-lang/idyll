//! The REPL.  What an entry *means* — checking it with the declarations made
//! so far in scope — is the compiler's business ([`Session`]); this module is
//! the line editing, history and key handling around it.

use idyllic::pipeline::{Options, Session};
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{
    Cmd, Completer, ConditionalEventHandler, Config, Editor, Event, EventContext, EventHandler, Helper, Highlighter,
    Hinter, KeyEvent, RepeatCount,
};
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

pub fn run(opts: &Options, axcut: bool) -> ExitCode {
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
                if let Some(report) = entered.output.axcut_report().filter(|_| axcut) {
                    print!("{report}");
                }
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
