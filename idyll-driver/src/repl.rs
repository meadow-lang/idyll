//! The REPL.  What an entry *means* — checking it with the declarations made
//! so far in scope — is the compiler's business ([`Session`]); this module is
//! the line editing, history, commands and the tour around it.  Much of it
//! follows Meadow's REPL: Tab completion, the `Ctrl-F` finder, an entry that
//! submits itself once it is whole, `:t`, `:module`, `:time` and a choice of
//! machine.  Idyll adds proof search (`:prove`, and `?` in the finder) and
//! `:ir`, a pop-up of what a block of code is at each stage of the compiler.

use idyll::complete;
use idyll::find::{Decl, Index};
use idyll::finder::{self, Catalog};
use crate::tour::{self, Section, STEPS};
use idyll::viewer::{self, Tab, Viewer};
use idyllic::inspect;
use idyllic::pipeline::{self, Evaluation, Mode, Options, Session};
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::history::DefaultHistory;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{
    Cmd, ConditionalEventHandler, Config, Editor, Event, EventContext, EventHandler, Helper, Hinter, KeyCode, KeyEvent,
    Modifiers, RepeatCount,
};
use std::borrow::Cow;
use std::io::IsTerminal;
use std::process::ExitCode;
use std::sync::{Arc, RwLock};
use std::time::Instant;

const HISTORY_FILE: &str = ".repl_history";
const PROMPT: &str = "> ";

const LOGO: &str = r"    ____    __      ____
   /  _/___/ /_  __/ / /
   / // __  / / / / / /
 _/ // /_/ / /_/ / / /
/___/\__,_/\__, /_/_/
          /____/";

/// The commands, for the banner and `:help`.
const COMMANDS: &[(&str, &str)] = &[
    (":t <expr>", "type-check without running (:type)"),
    (":module", "list what is defined so far (:browse)"),
    (":find <query>", "search by name, or by a type like  Int -> String"),
    (":prove <type>", "look for a term of a type: proof search"),
    (":ir [stages] <code>", "what a block is at each stage of the compiler; :ir alone, the last entry"),
    (":machine / :eval / :native", "run on the AxCut machine (the default), the reference interpreter, or LLVM"),
    (":time [<expr>]", "time every entry from now on, or stop; or time just this one"),
    (":reset", "forget every definition made so far"),
    (":tour", "a guided tour of the language, one step at a time"),
    (":next / :back / :try", "move through it (:n, :b); :try puts its example on the prompt"),
    (":tour <n> / <section> / off", "go to a step, or where a section starts; or leave the tour"),
    (":help", "this list (:h, :?)"),
    (":quit", "leave (:q, or Ctrl-C / Ctrl-D on an empty prompt)"),
];

/// Every command's name, for completion.
const COMMAND_NAMES: &[&str] = &[
    ":t", ":type", ":module", ":browse", ":find", ":prove", ":ir", ":machine", ":eval", ":native", ":time", ":reset",
    ":tour", ":next", ":back", ":try", ":help", ":quit",
];

/// The rustyline helper: an entry that knows when it is whole, completion,
/// and the prompt in colour.
#[derive(Helper, Hinter, Default)]
struct ReplHelper {
    names: complete::Names,
    colour: bool,
}

impl Highlighter for ReplHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(&'s self, prompt: &'p str, default: bool) -> Cow<'b, str> {
        if default && self.colour {
            Cow::Owned(format!("\x1b[1;32m{prompt}\x1b[0m"))
        } else {
            Cow::Borrowed(prompt)
        }
    }
}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(&self, line: &str, pos: usize, _: &rustyline::Context<'_>) -> rustyline::Result<(usize, Vec<Pair>)> {
        let start = complete::word_start(line, pos);
        let candidates = self
            .names
            .candidates(&line[..pos], &line[start..pos])
            .into_iter()
            .map(|c| Pair { display: c.clone(), replacement: c })
            .collect();
        Ok((start, candidates))
    }
}

impl Validator for ReplHelper {
    fn validate(&self, ctx: &mut ValidationContext) -> rustyline::Result<ValidationResult> {
        Ok(if needs_continuation(ctx.input()) { ValidationResult::Incomplete } else { ValidationResult::Valid(None) })
    }
}

/// Whether the entry being typed should keep taking lines rather than be
/// submitted.
///
/// - A command is submitted at once, but for `:ir` with code under it,
///   which is code like any other.
/// - A trailing `\` asks for another line.
/// - An entry of one line is submitted as soon as it parses, so that `1 + 2`
///   needs one Enter -- unless it is a signature, which is only half a
///   declaration, or it ends in something that wants more (`where`, `=`,
///   `->`, an open bracket).
/// - An entry of several lines is submitted by a blank line.  Idyll's
///   definitions may have several equations, each of which parses alone, so
///   there is no telling sooner that one is finished.
pub fn needs_continuation(input: &str) -> bool {
    let trimmed = input.trim_start();
    let multiline = input.contains('\n');
    if trimmed.starts_with(':') && !(trimmed.starts_with(":ir") && multiline) {
        return false;
    }
    if input.trim_end_matches([' ', '\t']).ends_with('\\') {
        return true;
    }
    // "blank" has to mean whitespace: auto-indent puts spaces on the line
    // before Enter is pressed
    if multiline {
        return input.rsplit('\n').next().is_some_and(|l| !l.trim().is_empty());
    }
    let code = strip_strings_and_comments(input);
    if opens_a_signature(&code) || bracket_depth(&code) > 0 || dangles(&code) {
        return true;
    }
    !parses_ok(input)
}

/// Whether a line is a signature with nothing defined yet: `f : T`, or
/// `(++) : T`.
fn opens_a_signature(line: &str) -> bool {
    let line = line.trim_start();
    let name_end = if line.starts_with('(') {
        line.find(')').map(|i| i + 1)
    } else {
        line.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '\''))
    };
    let Some(end) = name_end else { return false };
    let name = &line[..end];
    let first = name.chars().next();
    let named = first.is_some_and(|c| c.is_lowercase() || c == '_' || c == '(');
    named && line[end..].trim_start().starts_with(':') && !line.contains('=')
}

/// Whether the line ends in something that wants more after it.
fn dangles(code: &str) -> bool {
    matches!(
        code.split_whitespace().last().unwrap_or(""),
        "->" | "=" | "where" | "of" | "with" | "let" | "in" | "if" | "then" | "else" | "\\" | "|" | "," | "::" | "++"
    )
}

/// Whether `input` lexes, lays out and parses with no errors.
fn parses_ok(input: &str) -> bool {
    let lexed = idyllic_lexer::tokenize(input);
    if !lexed.diagnostics.is_empty() {
        return false;
    }
    let laid_out = idyllic_layout::layout(&idyllic_span::LineIndex::new(input), &lexed.tokens);
    laid_out.errors.is_empty() && idyllic_parser::parse(input.len(), &laid_out.tokens).is_ok()
}

/// The code with string literals emptied and `--` comments gone, for
/// counting brackets and finding the last word.
fn strip_strings_and_comments(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push('"');
                while let Some(d) = chars.next() {
                    if d == '\\' {
                        chars.next();
                    } else if d == '"' {
                        break;
                    }
                }
                out.push('"');
            }
            '-' if chars.peek() == Some(&'-') => {
                for d in chars.by_ref() {
                    if d == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            c => out.push(c),
        }
    }
    out
}

fn bracket_depth(code: &str) -> i32 {
    code.chars().fold(0, |depth, c| match c {
        '(' | '[' | '{' => depth + 1,
        ')' | ']' | '}' => depth - 1,
        _ => depth,
    })
}

/// Turn `foo \<newline> bar` into `foo <newline> bar`: the marker goes, the
/// newline stays, so that spans stay where they were.
fn join_continuations(input: &str) -> String {
    input.split('\n').map(|l| l.trim_end().strip_suffix('\\').unwrap_or(l)).collect::<Vec<_>>().join("\n")
}

/// The whitespace a continuation line opens with: the line above's own
/// indentation, and two more after something that opens a block or a body
/// (`where`, `of`, `=`, ...).  In the source's columns, which are what
/// layout reads: continuation lines show two columns left of where they sit
/// in the source, having no prompt.  (Meadow pads them by the prompt's width
/// and takes it off again; Idyll's layout would be broken by that when a
/// line is pasted, or read from a pipe, without the padding.)
fn auto_indent(before: &str) -> String {
    let own = before.rsplit('\n').next().unwrap_or("");
    let indent = own.len() - own.trim_start().len();
    let opens = matches!(
        strip_strings_and_comments(own).split_whitespace().last().unwrap_or(""),
        "where" | "of" | "with" | "=" | "->" | "let" | "in" | "then" | "else"
    );
    " ".repeat(indent + if opens { 2 } else { 0 })
}

/// Opens a continuation line already indented.  Bound to Enter, where it
/// only fires while the entry is unfinished (a whole one still falls through
/// to the validator and is submitted), and to the keys that always break
/// the line: Alt-Enter, Shift-Enter, Ctrl-J.
struct AutoIndent {
    always: bool,
}

impl ConditionalEventHandler for AutoIndent {
    fn handle(&self, _: &Event, _: RepeatCount, _: bool, ctx: &EventContext<'_>) -> Option<Cmd> {
        if !self.always && !needs_continuation(ctx.line()) {
            return None;
        }
        // a blank line ends a multi-line entry, with no indent to hold it
        // open
        if !self.always && ctx.line().contains('\n') && ctx.line().rsplit('\n').next().is_some_and(|l| l.trim().is_empty())
        {
            return None;
        }
        let before = &ctx.line()[..ctx.pos()];
        Some(Cmd::Insert(1, format!("\n{}", auto_indent(before))))
    }
}

/// `Ctrl-F`: the finder, over the prompt.  What is chosen goes on the
/// prompt at the cursor.
struct FindKey {
    catalog: Arc<RwLock<Catalog>>,
}

impl ConditionalEventHandler for FindKey {
    fn handle(&self, _: &Event, _: RepeatCount, _: bool, _: &EventContext<'_>) -> Option<Cmd> {
        let catalog = self.catalog.read().ok()?;
        match finder::pick(&catalog) {
            Ok(Some(text)) => Some(Cmd::Insert(1, text)),
            // closed without choosing, or the terminal would not have it:
            // the line goes back as it was
            _ => Some(Cmd::Repaint),
        }
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
fn escape_keys() -> Vec<rustyline::KeyEvent> {
    let mut keys = vec![rustyline::KeyEvent::ctrl('C'), rustyline::KeyEvent::ctrl('\\')];
    if cfg!(windows) {
        keys.push(rustyline::KeyEvent::ctrl('Z'));
    }
    keys
}

/// What to do after a command.
pub enum Flow {
    Continue,
    Quit,
}

/// Which machine runs an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// the AxCut machine, which checks memory as it goes
    Machine,
    /// the reference interpreter, `idyll-eval`: the specification
    Eval,
    /// compiled by LLVM and run as an executable (needs clang)
    Native,
}

impl std::fmt::Display for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Engine::Machine => "AxCut machine",
            Engine::Eval => "reference interpreter",
            Engine::Native => "native compiler",
        })
    }
}

/// Styling for what the REPL itself prints, following the same colour
/// decision as the diagnostics.
struct Paint {
    on: bool,
}

impl Paint {
    fn wrap(&self, code: &str, s: &str) -> String {
        if self.on { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
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

pub struct Repl {
    opts: Options,
    axcut: bool,
    paint: Paint,
    /// declarations stay in scope for later entries
    session: Session,
    engine: Engine,
    /// report how long every entry took: `:time` turns it on and off
    timing: bool,
    /// the last entry that was code, for `:ir` with nothing after it
    /// (and the session's declarations as they were before it, which may
    /// now include the entry's own)
    last: Option<(String, String)>,
    /// what the finder and completion know of, refreshed after every entry
    catalog: Arc<RwLock<Catalog>>,
    names: complete::Names,
    /// Which step of the tour is showing, if any.  The tour is not a mode:
    /// the prompt between steps is the ordinary one, and this is only where
    /// `:next` and `:try` carry on from.
    tour: Option<usize>,
    /// how many of the current step's examples `:try` has handed over
    tried: usize,
    /// what `:try` left to go on the prompt, instead of an empty line
    pending: Option<String>,
    /// whether a pop-up can be shown: stdout is a terminal
    tui: bool,
}

impl Repl {
    pub fn new(opts: &Options, axcut: bool) -> Repl {
        let mut repl = Repl {
            opts: *opts,
            axcut,
            paint: Paint { on: opts.style.color },
            session: Session::default(),
            engine: Engine::Machine,
            timing: false,
            last: None,
            catalog: Arc::new(RwLock::new(Catalog::default())),
            names: complete::Names::default(),
            tour: None,
            tried: 0,
            pending: None,
            tui: std::io::stdout().is_terminal(),
        };
        repl.refresh();
        repl
    }

    /// Bring what the finder and completion know of up to date with the
    /// session.
    fn refresh(&mut self) {
        let source = self.session.source().to_string();
        let checked = pipeline::check(&source, &Options { live: false, ..self.opts });
        let decls = Decl::all(&checked.declarations, &|ty| idyll::find::bare(&source, ty));
        self.names = complete::Names::new(&decls, COMMAND_NAMES);
        if let Ok(mut catalog) = self.catalog.write() {
            *catalog = Catalog { index: Index::new(decls), earlier: source, opts: Some(self.opts) };
        }
    }

    /// Handle one line or block typed at the prompt: a command, or code.
    pub fn handle(&mut self, entry: &str) -> Flow {
        let entry = join_continuations(entry);
        let trimmed = entry.trim();
        if trimmed.starts_with(':') {
            return self.command(trimmed);
        }
        self.last = Some((self.session.source().to_string(), trimmed.to_string()));
        self.enter(trimmed, false, self.timing);
        self.refresh();
        Flow::Continue
    }

    fn command(&mut self, cmd: &str) -> Flow {
        let (name, rest) = match cmd.find(char::is_whitespace) {
            Some(i) => (&cmd[..i], cmd[i..].trim()),
            None => (cmd, ""),
        };
        let words: Vec<&str> = rest.split_whitespace().collect();
        match (name, words.as_slice()) {
            (":quit" | ":q", _) => return Flow::Quit,
            (":reset", _) => {
                self.session.reset();
                self.refresh();
                println!("{}", self.paint.dim("(every definition is forgotten)"));
            }
            (":help" | ":h" | ":?", _) => self.help(),
            (":t" | ":type", []) => println!("{}", self.paint.dim("(:t takes an expression to check)")),
            (":t" | ":type", _) => self.enter(rest, true, false),
            (":module" | ":browse", _) => self.module(),
            (":machine" | ":eval" | ":native", _) => {
                self.engine = match name {
                    ":eval" => Engine::Eval,
                    ":native" => Engine::Native,
                    _ => Engine::Machine,
                };
                println!("{}", self.paint.dim(&format!("(running on the {})", self.engine)));
            }
            (":time", []) => {
                self.timing = !self.timing;
                let now = if self.timing { "on" } else { "off" };
                println!("{}", self.paint.dim(&format!("(timing every entry: {now})")));
            }
            (":time", _) => {
                self.last = Some((self.session.source().to_string(), rest.to_string()));
                self.enter(rest, false, true);
                self.refresh();
            }
            (":find", _) => self.find(rest),
            (":prove", _) => self.prove(rest),
            (":ir", _) => self.ir(rest),
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
            (":try", _) => match self.tour {
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

    /// Check an entry, and run it unless `type_only`, on the machine chosen.
    fn enter(&mut self, entry: &str, type_only: bool, timed: bool) {
        let started = Instant::now();
        let mut entered = if type_only {
            self.session.enter_in(entry, &self.opts, Mode::Check)
        } else {
            match self.engine {
                Engine::Machine => self.session.enter(entry, &self.opts),
                Engine::Eval => {
                    let mut entered = self.session.enter_in(entry, &self.opts, Mode::Lower);
                    self.evaluate(&mut entered.output);
                    entered
                }
                Engine::Native => {
                    let mut entered = self.session.enter_in(entry, &self.opts, Mode::Lower);
                    if let Err(why) = self.native(&mut entered.output) {
                        println!("{}", self.paint.dim(&format!("(not run natively: {why}; it runs on the machine)")));
                        entered = self.session.enter(entry, &self.opts);
                    }
                    entered
                }
            }
        };
        let took = started.elapsed();
        if let Some(report) = entered.output.axcut_report().filter(|_| self.axcut && !type_only) {
            print!("{report}");
        }
        if type_only {
            entered.output.printed.clear();
        }
        println!("{}", entered.output.display("<interactive>", &entered.source, self.opts.style));
        if timed {
            println!("{}", self.paint.dim(&format!("({:.1} ms on the {})", took.as_secs_f64() * 1000.0, self.engine)));
        }
    }

    /// Run a lowered entry on the reference interpreter.
    fn evaluate(&self, output: &mut pipeline::Output) {
        let Some(checked) = output.checked.as_ref().filter(|c| c.program.is_some()) else { return };
        if output.has_errors() {
            return;
        }
        let mut world = pipeline::world(&self.opts, Vec::new());
        let result = idyll_eval::evaluate_in(checked, pipeline::FUEL, &mut world);
        output.evaluation = result.map(|r| match r {
            Ok(outcome) => outcome.answer.map_or(Evaluation::Opaque, Evaluation::Value),
            Err(idyll_eval::EvalError::OutOfFuel) => Evaluation::OutOfFuel,
            Err(idyll_eval::EvalError::Exited(code)) => Evaluation::Exited(code),
            Err(idyll_eval::EvalError::TestFailed(why)) => Evaluation::Stopped(format!("test failed: {why}")),
            Err(idyll_eval::EvalError::Stuck(why)) => Evaluation::Failed(format!("the reference interpreter is stuck: {why}")),
        });
        if !self.opts.live {
            let (printed, errors) = world.into_streams();
            output.printed = printed;
            output.errors = errors;
        }
    }

    /// Compile a lowered entry with LLVM and run it.  `Err` if it cannot be
    /// compiled, and the machine should run it instead.
    fn native(&self, output: &mut pipeline::Output) -> Result<(), String> {
        if output.has_errors() || output.body_type.is_none() {
            return Ok(());
        }
        let program = output.native.as_ref().ok_or("there is nothing to compile")?;
        let ir = idyllic_llvm::emit(program, output.result_kind)?;
        let dir = std::env::temp_dir().join(format!("idyll-repl-{}", std::process::id()));
        let exe = dir.join(if cfg!(windows) { "entry.exe" } else { "entry" });
        idyll::native::compile(&ir, &exe, None).map_err(|e| e.to_string())?;
        let ran = std::process::Command::new(&exe)
            .stdin(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .output()
            .map_err(|e| format!("could not run it: {e}"))?;
        let stdout = String::from_utf8_lossy(&ran.stdout).replace("\r\n", "\n");
        let code = ran.status.code().unwrap_or(1);
        if output.ran_main || code != 0 {
            output.printed = stdout;
            output.evaluation = Some(if code == 0 { Evaluation::Opaque } else { Evaluation::Exited(code) });
            return Ok(());
        }
        // the program prints what it prints, and then its answer
        let mut lines: Vec<&str> = stdout.trim_end_matches('\n').split('\n').collect();
        let answer = lines.pop().unwrap_or("").to_string();
        output.printed = lines.iter().map(|l| format!("{l}\n")).collect();
        output.evaluation = Some(match output.result_kind {
            idyllic_llvm::ResultKind::Other => Evaluation::Opaque,
            _ => Evaluation::Value(answer),
        });
        Ok(())
    }

    fn help(&self) {
        println!();
        for (cmd, what) in COMMANDS {
            println!("  {}  {what}", self.paint.command(&format!("{cmd:<28}")));
        }
        println!();
        println!("  {} completes names and commands.", self.paint.strong("Tab"));
        println!(
            "  {} finds a declaration, by name or by a type like {}; {} and a type looks for a term of it.",
            self.paint.strong("Ctrl-F"),
            self.paint.command("Int -> String"),
            self.paint.command("?")
        );
        println!(
            "  {}: {} / {} start a new line; a signature, or a line ending in {} or {}, keeps reading.",
            self.paint.strong("Several lines"),
            self.paint.command("Alt-Enter"),
            self.paint.command("Ctrl-J"),
            self.paint.command("where"),
            self.paint.command("=")
        );
        println!("  A blank line ends an entry of several lines.");
        println!();
    }

    /// `:module`: every definition made so far, with its type.
    fn module(&self) {
        let checked = pipeline::check(self.session.source(), &Options { live: false, ..self.opts });
        if checked.decls.is_empty() {
            println!("{}", self.paint.dim("(nothing defined yet)"));
            return;
        }
        for (name, ty) in &checked.decls {
            println!("{name} : {ty}");
        }
    }

    /// `:find`: the best answers to a query, by name or by type.
    fn find(&self, query: &str) {
        let Ok(catalog) = self.catalog.read() else { return };
        if query.trim_start().starts_with('?') {
            drop(catalog);
            return self.prove(query.trim_start().trim_start_matches('?'));
        }
        let hits = catalog.index.search(query, 12);
        if hits.is_empty() {
            println!("{}", self.paint.dim("(nothing matches)"));
        }
        for hit in hits {
            let d = &catalog.index.decls[hit.decl];
            let from = match d.module.as_str() {
                "" => String::new(),
                "Prelude" => "  (prelude)".to_string(),
                package => format!("  ({package})"),
            };
            println!("{} {}{}", self.paint.dim(&format!("{:<6}", d.kind.label())), d.headline(), self.paint.dim(&from));
        }
    }

    /// `:prove`: terms of a type, found and checked.
    fn prove(&self, goal: &str) {
        let Ok(catalog) = self.catalog.read() else { return };
        match idyll::prove::prove(&catalog.earlier, goal, &catalog.index.decls, &self.opts, 8) {
            Ok(found) if found.is_empty() => println!(
                "{}",
                self.paint.dim("(no term found: a goal that needs a `case`, or more than three deep, is beyond the search)")
            ),
            Ok(found) => {
                for term in found {
                    println!("{term}");
                }
            }
            Err(why) => println!("{}", why.trim_end()),
        }
    }

    /// `:ir`: a block of code at each stage of the compiler.
    fn ir(&mut self, rest: &str) {
        let stage_names: Vec<&str> = inspect::STAGES.iter().map(|(n, _)| *n).collect();
        // `:ir core,cut <code>` asks for those stages only
        let (wanted, code): (Vec<&str>, &str) = match rest.split_once(char::is_whitespace) {
            Some((first, code)) if first.split(',').all(|s| stage_names.contains(&s)) => {
                (first.split(',').collect(), code.trim())
            }
            None if !rest.is_empty() && rest.split(',').all(|s| stage_names.contains(&s)) => (rest.split(',').collect(), ""),
            _ => (Vec::new(), rest),
        };
        let (earlier, code) = if code.is_empty() {
            match &self.last {
                Some(last) => last.clone(),
                None => {
                    println!("{}", self.paint.dim("(:ir takes a block of code, or shows the last entry: there is none yet)"));
                    println!();
                    for (name, what) in inspect::STAGES {
                        println!("  {}  {what}", self.paint.command(&format!("{name:<8}")));
                    }
                    println!();
                    return;
                }
            }
        } else {
            (self.session.source().to_string(), code.to_string())
        };
        let inspection = inspect::inspect(&earlier, &code, &Options { live: false, ..self.opts }, &wanted);
        let about = |name: &str| inspect::STAGES.iter().find(|(n, _)| *n == name).map_or("", |(_, a)| a).to_string();
        let tabs: Vec<Tab> = inspection
            .stages
            .into_iter()
            .map(|(name, text)| Tab {
                name: name.to_string(),
                about: about(name),
                text: text.unwrap_or_else(|why| format!("({why})")),
            })
            .collect();
        let title = code.lines().next().unwrap_or("").to_string();
        if self.tui && !tabs.is_empty() {
            if let Err(e) = viewer::show(Viewer::new(&title, tabs)) {
                println!("{}", self.paint.dim(&format!("(the viewer could not open: {e})")));
            }
        } else {
            for tab in tabs {
                println!("{}", self.paint.heading(&format!("── {} ── {}", tab.name, tab.about)));
                println!("{}", tab.text);
                println!();
            }
        }
        if let Some(errors) = inspection.errors {
            println!("{}", errors.trim_end());
        }
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
        println!("  {} starts at the beginning, and a section can be started by name:", p.command(":next"));
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
        println!("  {} puts the next of those on the prompt (an empty line runs it),", p.command(":try"));
        println!("  {} moves on, {} goes back.", p.command(":next"), p.command(":back"));
        println!();
        self.tour = Some(at);
        self.tried = 0;
    }
}

fn print_banner(p: &Paint) {
    println!("{}", p.heading(LOGO));
    println!();
    println!("  Welcome to the Idyll REPL!");
    println!();
    for (cmd, what) in COMMANDS {
        println!("  {}  {what}", p.command(&format!("{cmd:<28}")));
    }
    println!();
    println!("  {} completes names; {} finds a declaration, by name or by type.", p.strong("Tab"), p.strong("Ctrl-F"));
    println!();
}

pub fn run(opts: &Options, axcut: bool) -> ExitCode {
    let editor = Config::builder()
        .max_history_size(1000)
        .map(|b| b.completion_type(rustyline::CompletionType::List).build())
        .and_then(Editor::<ReplHelper, DefaultHistory>::with_config);
    let mut rl = match editor {
        Ok(rl) => rl,
        Err(e) => {
            eprintln!("error: cannot start the line editor: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut repl = Repl::new(opts, axcut);
    rl.set_helper(Some(ReplHelper { names: repl.names.clone(), colour: opts.style.color }));
    for key in escape_keys() {
        rl.bind_sequence(key, EventHandler::Conditional(Box::new(Escape)));
    }
    rl.bind_sequence(
        rustyline::KeyEvent::ctrl('F'),
        EventHandler::Conditional(Box::new(FindKey { catalog: repl.catalog.clone() })),
    );
    // keys that always break the line, on terminals that report them
    for key in [
        KeyEvent(KeyCode::Enter, Modifiers::ALT),
        KeyEvent(KeyCode::Enter, Modifiers::SHIFT),
        rustyline::KeyEvent::ctrl('J'),
    ] {
        rl.bind_sequence(key, EventHandler::Conditional(Box::new(AutoIndent { always: true })));
    }
    // plain Enter continues an unfinished entry, indented, and otherwise
    // falls through to the validator, which submits it
    rl.bind_sequence(
        KeyEvent(KeyCode::Enter, Modifiers::NONE),
        EventHandler::Conditional(Box::new(AutoIndent { always: false })),
    );
    // a missing history file is the normal first-run case
    let _ = rl.load_history(HISTORY_FILE);
    print_banner(&repl.paint);

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
                if let Flow::Quit = repl.handle(entry) {
                    break;
                }
                // what was defined is completable, and findable, now
                if let Some(h) = rl.helper_mut() {
                    h.names = repl.names.clone();
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_line_is_submitted_and_half_of_one_is_not() {
        assert!(!needs_continuation("1 + 2"));
        assert!(!needs_continuation("double n = n * 2"));
        assert!(!needs_continuation(":t 1"));
        // a signature, which is half a declaration
        assert!(needs_continuation("double : Int -> Int"));
        assert!(needs_continuation("(++) : String -> String -> String"));
        // something that wants more
        assert!(needs_continuation("effect Ask where"));
        assert!(needs_continuation("f x ="));
        assert!(needs_continuation("(1 +"));
        assert!(needs_continuation("1 + \\"));
        // a string with a bracket in it is not open
        assert!(!needs_continuation("\"(\""));
    }

    #[test]
    fn several_lines_end_at_a_blank_one() {
        assert!(needs_continuation("double : Int -> Int\ndouble n = n * 2"));
        assert!(!needs_continuation("double : Int -> Int\ndouble n = n * 2\n"));
        assert!(!needs_continuation("double : Int -> Int\ndouble n = n * 2\n   "));
        // `:ir` with code under it is code
        assert!(needs_continuation(":ir\ndouble n = n * 2"));
        assert!(!needs_continuation(":ir\ndouble n = n * 2\n"));
    }

    #[test]
    fn a_continuation_is_indented_as_its_block_wants() {
        assert_eq!(auto_indent("effect Ask where"), "  ");
        assert_eq!(auto_indent("double : Int -> Int"), "");
        assert_eq!(auto_indent("effect Ask where\n  ask : () -> Int"), "  ");
        assert_eq!(auto_indent("f x =\n  let y = x\n      z = y in"), "        ");
    }
}
