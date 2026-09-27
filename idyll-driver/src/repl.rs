//! The REPL.  What an entry *means* — checking it with the declarations made
//! so far in scope — is the compiler's business ([`Session`]); this module is
//! the line editing, history, commands and the tour around it.

use crate::tour::{self, Section, STEPS};
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
const PROMPT: &str = "> ";

const BANNER: &str = r"    ____    __      ____
   /  _/___/ /_  __/ / /
   / // __  / / / / / /
 _/ // /_/ / /_/ / / /
/___/\__,_/\__, /_/_/
          /____/
Welcome to the Idyll REPL!
Type :tour for a guided tour, :help for the commands, :quit to leave.";

/// The commands, for `:help`.
const COMMANDS: &[(&str, &str)] = &[
    (":tour", "a guided tour of the language, one step at a time"),
    (":tour <n> / <section>", "go to a step, or to where a section starts"),
    (":next / :back", "move through the tour (:n, :b)"),
    (":try", "put the tour's next example on the prompt, to edit and run"),
    (":tour off", "leave the tour; what you defined stays"),
    (":reset", "forget every definition made so far"),
    (":help", "this list (:h, :?)"),
    (":quit", "leave (:q, or Ctrl-C / Ctrl-D on an empty prompt)"),
];

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

/// What to do after a command.
enum Flow {
    Continue,
    Quit,
}

/// Styling for what the REPL itself prints, following the same colour
/// decision as the diagnostics.
struct Paint {
    on: bool,
}

impl Paint {
    fn wrap(&self, code: &str, s: &str) -> String {
        if self.on {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    fn heading(&self, s: &str) -> String {
        self.wrap("1;32", s)
    }
    fn command(&self, s: &str) -> String {
        self.wrap("36", s)
    }
    fn strong(&self, s: &str) -> String {
        self.wrap("1;36", s)
    }
    fn dim(&self, s: &str) -> String {
        self.wrap("2", s)
    }
}

struct Repl<'a> {
    opts: &'a Options,
    axcut: bool,
    paint: Paint,
    /// declarations stay in scope for later entries
    session: Session,
    /// Which step of the tour is showing, if any.  The tour is not a mode:
    /// the prompt between steps is the ordinary one, and this is only where
    /// `:next` and `:try` carry on from.
    tour: Option<usize>,
    /// how many of the current step's examples `:try` has handed over
    tried: usize,
    /// what `:try` left to go on the prompt, instead of an empty line
    pending: Option<String>,
}

impl Repl<'_> {
    fn command(&mut self, cmd: &str) -> Flow {
        let mut words = cmd.split_whitespace();
        let name = words.next().unwrap_or("");
        let arg: Vec<&str> = words.collect();
        match (name, arg.as_slice()) {
            (":quit" | ":q", _) => return Flow::Quit,
            (":reset", _) => {
                self.session.reset();
                println!("{}", self.paint.dim("(every definition is forgotten)"));
            }
            (":help" | ":h" | ":?", _) => self.help(),
            (":tour", []) => match self.tour {
                Some(at) => self.show_step(at),
                None => self.contents(),
            },
            (":tour", ["off" | "end" | "stop"]) => {
                self.tour = None;
                println!("{}", self.paint.dim("(the tour is over; everything you defined is still here)"));
            }
            (":tour", [what]) => match (Section::named(what), what.parse::<usize>()) {
                (Some(section), _) => match section.first() {
                    Some(at) => self.show_step(at),
                    None => println!("{}", self.paint.dim("(that section has no steps yet)")),
                },
                (_, Ok(n)) if (1..=STEPS.len()).contains(&n) => self.show_step(n - 1),
                _ => {
                    println!("{}", self.paint.dim(&format!("(there is no step or section called {what})")));
                    self.contents();
                }
            },
            (":next" | ":n", _) => match self.tour {
                Some(at) if at + 1 < STEPS.len() => self.show_step(at + 1),
                Some(_) => {
                    self.tour = None;
                    println!(
                        "{}",
                        self.paint.dim(
                            "(that was the last step. The tour grows as Idyll does; TUTORIAL.md has everything in detail.)"
                        )
                    );
                }
                None => self.show_step(0),
            },
            (":back" | ":b", _) => match self.tour {
                Some(at) if at > 0 => self.show_step(at - 1),
                _ => self.show_step(0),
            },
            (":try" | ":t", _) => match self.tour {
                Some(at) => {
                    let examples = STEPS[at].examples;
                    // round again rather than refusing: a step is worth
                    // running twice
                    if self.tried >= examples.len() {
                        self.tried = 0;
                    }
                    self.pending = Some(examples[self.tried].source.to_string());
                    self.tried += 1;
                }
                None => println!("{}", self.paint.dim("(`:tour` first, and `:try` will offer what to type)")),
            },
            _ => println!("{}", self.paint.dim(&format!("(unknown command {name}; :help lists them)"))),
        }
        Flow::Continue
    }

    fn help(&self) {
        println!();
        for (cmd, what) in COMMANDS {
            println!("  {}  {what}", self.paint.command(&format!("{cmd:<22}")));
        }
        println!();
    }

    /// What the tour has to offer: its sections, and where each begins.
    /// Shown before anything has started, because a tour whose shape you
    /// cannot see is one you have to take in order.
    fn contents(&self) {
        let p = &self.paint;
        println!();
        println!("  {}", p.heading("A tour of Idyll"));
        println!();
        for section in Section::ALL {
            let steps: Vec<(usize, &tour::Step)> =
                STEPS.iter().enumerate().filter(|(_, s)| s.section == section).collect();
            let Some((first, _)) = steps.first() else { continue };
            println!(
                "  {} {} steps, from {}",
                p.strong(&format!("{:<22}", section.title())),
                steps.len(),
                p.command(&format!(":tour {}", first + 1))
            );
            for (i, step) in &steps {
                println!("    {:>3}. {}", i + 1, step.title);
            }
            println!();
        }
        println!(
            "  {} starts at the beginning, and a section can be started by name:",
            p.command(":next")
        );
        println!("  {} or {}.", p.command(":tour grades"), p.command(":tour dependent"));
        println!("  The examples define names, so take it again after a {}.", p.command(":reset"));
        println!();
    }

    /// Show step `at`, and remember that it is where we are.  What follows
    /// is the ordinary prompt: a step is something to read and then try,
    /// not a question to answer.
    fn show_step(&mut self, at: usize) {
        let p = &self.paint;
        let step = &STEPS[at];
        println!();
        println!("  {}  {}", p.heading(&tour::heading(at)), p.dim(&format!("({})", step.section.title())));
        println!();
        for line in step.text {
            if line.is_empty() {
                println!();
            } else {
                println!("  {line}");
            }
        }
        println!();
        for (i, ex) in step.examples.iter().enumerate() {
            let number = format!("{}.", i + 1);
            let mut lines = ex.source.lines();
            let first = lines.next().unwrap_or("");
            let rest: Vec<&str> = lines.collect();
            let note = match ex.rejected_with {
                Some(_) => format!("   {}", p.dim("(an error, on purpose)")),
                None => String::new(),
            };
            if rest.is_empty() {
                println!("  {} {first}{note}", p.strong(&number));
            } else {
                println!("  {} {first}", p.strong(&number));
                let last = rest.len() - 1;
                for (j, line) in rest.iter().enumerate() {
                    let tail = if j == last { note.as_str() } else { "" };
                    println!("  {} {line}{tail}", " ".repeat(number.len()));
                }
            }
        }
        println!();
        println!(
            "  {} puts the next of those on the prompt (an empty line runs it),",
            p.command(":try")
        );
        println!("  {} moves on, {} goes back.", p.command(":next"), p.command(":back"));
        println!();
        self.tour = Some(at);
        self.tried = 0;
    }

    fn enter(&mut self, entry: &str) {
        let entered = self.session.enter(entry, self.opts);
        if let Some(report) = entered.output.axcut_report().filter(|_| self.axcut) {
            print!("{report}");
        }
        println!("{}", entered.output.display("<interactive>", &entered.source, self.opts.style));
    }
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

    let mut repl = Repl {
        opts,
        axcut,
        paint: Paint { on: opts.style.color },
        session: Session::default(),
        tour: None,
        tried: 0,
        pending: None,
    };

    loop {
        // `:try` leaves an example on the prompt instead of running it, so
        // that it can be read and changed first
        let read = match repl.pending.take() {
            Some(text) => rl.readline_with_initial(PROMPT, (&text, "")),
            None => rl.readline(PROMPT),
        };
        match read {
            Ok(raw) => {
                let entry = raw.trim_end();
                if entry.trim().is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(entry);
                let _ = rl.append_history(HISTORY_FILE);
                if is_command(entry) {
                    match repl.command(entry.trim()) {
                        Flow::Continue => continue,
                        Flow::Quit => break,
                    }
                }
                repl.enter(entry);
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
