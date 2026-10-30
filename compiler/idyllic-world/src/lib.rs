//! The world, as a running program meets it: the effects in the prelude's
//! `Eff` that the runtime handles itself.  An operation of one of them that
//! no handler of the program's own takes is done here, and the program goes
//! on with what it gives back.
//!
//! The reference interpreter (`idyll-eval`) does the world's operations
//! through this crate, turning [`Answer`]s into values of its own. A
//! compiled program does them on Meadow's runtimes, as the operations of
//! Meadow's own each is declared to be.
//!
//! A [`World`] is *live*, reading and writing the real standard streams as
//! the program goes, or *captured*: its output kept to be looked at later,
//! its input empty, and its randomness seeded the same way every time, which
//! is what tests want.  Files, the environment and the clock are the real
//! ones either way.

use std::collections::VecDeque;
use std::io::{BufRead, Write};
use std::rc::Rc;

/// The world's operations, with how many arguments each takes: what native
/// code has a function in the runtime for, `idyll_op_NAME`.
pub const OPERATIONS: &[(&str, usize)] = &[
    ("putStr", 1),
    ("putErr", 1),
    ("readLine", 1),
    ("readToString", 1),
    ("writeString", 2),
    ("appendString", 2),
    ("removeFile", 1),
    ("createDir", 1),
    ("createDirAll", 1),
    ("removeDir", 1),
    ("removeDirAll", 1),
    ("rename", 2),
    ("copy", 2),
    ("listDir", 1),
    ("exists", 1),
    ("isFile", 1),
    ("isDir", 1),
    ("exit", 1),
    ("currentPid", 1),
    ("argv", 1),
    ("getEnv", 1),
    ("setEnv", 2),
    ("removeEnv", 1),
    ("isTerminal", 1),
    ("nextInt", 1),
    ("intBetween", 2),
    ("nextSeed", 1),
    ("now", 1),
    ("monotonic", 1),
    ("sleep", 1),
    ("fail", 1),
    ("abort", 1),
    ("logAt", 2),
];

/// What an operation is given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arg {
    Int(i64),
    Str(Rc<str>),
}

/// What an operation gives back: a value of one of the prelude's types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Unit,
    Int(i64),
    Bool(bool),
    Str(Rc<str>),
    /// A constructor of the prelude's, with its tag among its type's
    /// constructors: `Maybe`'s `Nothing` (0) and `Just` (1), `Result`'s
    /// `Err` (0) and `Ok` (1), `List`'s `Nil` (0) and `::` (1).
    Con(&'static str, usize, Vec<Answer>),
    /// An array (the prelude's `Array`).
    Array(Vec<Answer>),
}

impl Answer {
    fn nothing() -> Answer {
        Answer::Con("Nothing", 0, Vec::new())
    }

    fn just(a: Answer) -> Answer {
        Answer::Con("Just", 1, vec![a])
    }

    fn str(s: impl Into<Rc<str>>) -> Answer {
        Answer::Str(s.into())
    }

    /// `Ok a`, or `Err` with what went wrong, as text.
    fn result<T>(r: std::io::Result<T>, ok: impl FnOnce(T) -> Answer) -> Answer {
        match r {
            Ok(v) => Answer::Con("Ok", 1, vec![ok(v)]),
            Err(e) => Answer::Con("Err", 0, vec![Answer::str(e.to_string())]),
        }
    }
}

/// An operation that does not give back at all: the program ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    /// `exit n`
    Exit(i32),
    /// `fail message`, the `Test` effect's
    Failed(String),
}

/// The world's side of a running program.
pub struct World {
    live: bool,
    /// what a captured world was given to print, and to print as an error
    output: String,
    errors: String,
    /// what a captured world has to read, line by line
    input: VecDeque<String>,
    /// the state of the random generator
    seed: u64,
    /// the program's command-line arguments
    args: Vec<String>,
}

/// The seed a captured world's randomness starts from.
const TEST_SEED: u64 = 0x5EED_1D71;

impl World {
    /// The standard streams, as the program goes, and randomness seeded from
    /// the clock.
    pub fn live(args: Vec<String>) -> World {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        World { live: true, output: String::new(), errors: String::new(), input: VecDeque::new(), seed: nanos ^ TEST_SEED, args }
    }

    /// Output kept, no input, no arguments, and the same random numbers
    /// every time.
    pub fn captured() -> World {
        World {
            live: false,
            output: String::new(),
            errors: String::new(),
            input: VecDeque::new(),
            seed: TEST_SEED,
            args: Vec::new(),
        }
    }

    /// A captured world with these lines to read.
    pub fn with_input(mut self, lines: &[&str]) -> World {
        self.input = lines.iter().map(|l| l.to_string()).collect();
        self
    }

    /// What a captured world printed; nothing, for a live one.
    pub fn output(&self) -> &str {
        &self.output
    }

    pub fn into_output(self) -> String {
        self.output
    }

    /// What a captured world printed, and printed as errors (standard
    /// error); nothing, for a live one.
    pub fn into_streams(self) -> (String, String) {
        (self.output, self.errors)
    }

    /// `Log`'s line at `level`, as Meadow's runtime writes it: a live world
    /// writes it to standard error with the time, if `MEADOW_LOG` lets it;
    /// a captured one keeps it without the time, at `info` and above, so
    /// that a run is the same every time.
    fn log(&mut self, level: i64, message: &str) {
        let threshold = if self.live { log_threshold() } else { 1 };
        if level < threshold {
            return;
        }
        let name = LOG_LEVELS[level.clamp(0, 3) as usize];
        let line = if self.live {
            format!("{} {name} {message}\n", rfc3339(std::time::SystemTime::now()))
        } else {
            format!("{name} {message}\n")
        };
        self.print_error(&line);
    }

    fn print_error(&mut self, s: &str) {
        if self.live {
            let _ = std::io::stdout().flush();
            let mut err = std::io::stderr().lock();
            let _ = err.write_all(s.as_bytes());
            let _ = err.flush();
        } else {
            self.errors.push_str(s);
        }
    }

    fn print(&mut self, s: &str) {
        if self.live {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(s.as_bytes());
            let _ = out.flush();
        } else {
            self.output.push_str(s);
        }
    }

    fn read_line(&mut self) -> Option<String> {
        if !self.live {
            return self.input.pop_front();
        }
        let mut line = String::new();
        match std::io::stdin().lock().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => {
                let trimmed = line.trim_end_matches(['\n', '\r']).len();
                line.truncate(trimmed);
                Some(line)
            }
        }
    }

    /// SplitMix64, as Meadow's `Random` has it.
    fn next_random(&mut self) -> u64 {
        self.seed = self.seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Do the operation `op` with `args`, if it is one of the world's.
    /// `None` if it is not (or is given the wrong arguments), which is a
    /// compiler bug: the type checker lets no other operation get here.
    pub fn perform(&mut self, op: &str, args: &[Arg]) -> Option<Result<Answer, Stop>> {
        use Arg::{Int, Str};
        let unit = |()| Answer::Unit;
        let answer = match (op, args) {
            // Console
            ("putStr", [Str(s)]) => {
                self.print(s);
                Answer::Unit
            }
            ("putErr", [Str(s)]) => {
                self.print_error(s);
                Answer::Unit
            }
            ("readLine", [_]) => self.read_line().map_or_else(Answer::nothing, |l| Answer::just(Answer::str(l))),
            // Log
            ("logAt", [Int(level), Str(message)]) => {
                self.log(*level, message);
                Answer::Unit
            }
            // Time
            ("now", [_]) => {
                let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
                Answer::Int(since.map_or(0, |d| d.as_millis() as i64))
            }
            ("monotonic", [_]) => Answer::Int(monotonic_nanos() as i64),
            ("sleep", [Int(ms)]) => {
                if let Ok(ms) = u64::try_from(*ms) {
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                }
                Answer::Unit
            }
            // Random
            ("nextInt", [_]) | ("nextSeed", [_]) => Answer::Int(self.next_random() as i64),
            ("intBetween", [Int(lo), Int(hi)]) => {
                // as the native runtime does it, in unsigned arithmetic
                if hi <= lo {
                    Answer::Int(*lo)
                } else {
                    let span = hi.wrapping_sub(*lo) as u64;
                    Answer::Int((*lo as u64).wrapping_add(self.next_random() % span) as i64)
                }
            }
            // Process
            ("exit", [Int(code)]) => {
                if self.live {
                    let _ = std::io::stdout().flush();
                }
                return Some(Err(Stop::Exit(i32::try_from(*code).unwrap_or(1))));
            }
            ("currentPid", [_]) => Answer::Int(i64::from(std::process::id())),
            ("argv", [_]) => Answer::Array(self.args.iter().map(|a| Answer::str(a.as_str())).collect()),
            ("getEnv", [Str(name)]) => {
                std::env::var(&**name).map_or_else(|_| Answer::nothing(), |v| Answer::just(Answer::str(v)))
            }
            ("setEnv", [Str(name), Str(value)]) => {
                if !name.is_empty() && !name.contains(['=', '\0']) && !value.contains('\0') {
                    std::env::set_var(&**name, &**value);
                }
                Answer::Unit
            }
            ("removeEnv", [Str(name)]) => {
                if !name.is_empty() && !name.contains(['=', '\0']) {
                    std::env::remove_var(&**name);
                }
                Answer::Unit
            }
            ("isTerminal", [Int(fd)]) => {
                use std::io::IsTerminal;
                let fd = *fd;
                Answer::Bool(self.live && match fd {
                    0 => std::io::stdin().is_terminal(),
                    1 => std::io::stdout().is_terminal(),
                    2 => std::io::stderr().is_terminal(),
                    _ => false,
                })
            }
            // Fs
            ("readToString", [Str(path)]) => Answer::result(std::fs::read_to_string(&**path), Answer::str),
            ("writeString", [Str(path), Str(text)]) => Answer::result(std::fs::write(&**path, text.as_bytes()), unit),
            ("appendString", [Str(path), Str(text)]) => {
                let appended = std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&**path)
                    .and_then(|mut f| f.write_all(text.as_bytes()));
                Answer::result(appended, unit)
            }
            ("removeFile", [Str(path)]) => Answer::result(std::fs::remove_file(&**path), unit),
            ("createDir", [Str(path)]) => Answer::result(std::fs::create_dir(&**path), unit),
            ("createDirAll", [Str(path)]) => Answer::result(std::fs::create_dir_all(&**path), unit),
            ("removeDir", [Str(path)]) => Answer::result(std::fs::remove_dir(&**path), unit),
            ("removeDirAll", [Str(path)]) => Answer::result(std::fs::remove_dir_all(&**path), unit),
            ("rename", [Str(from), Str(to)]) => Answer::result(std::fs::rename(&**from, &**to), unit),
            // how many bytes were copied, as the runtime's Fs.copy answers
            ("copy", [Str(from), Str(to)]) => Answer::result(std::fs::copy(&**from, &**to), |n| Answer::Int(n as i64)),
            ("listDir", [Str(path)]) => {
                let names = std::fs::read_dir(&**path).and_then(|entries| {
                    let mut names = entries
                        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
                        .collect::<std::io::Result<Vec<String>>>()?;
                    names.sort();
                    Ok(names)
                });
                Answer::result(names, |names| Answer::Array(names.into_iter().map(Answer::str).collect()))
            }
            ("exists", [Str(path)]) => Answer::Bool(std::path::Path::new(&**path).exists()),
            ("isFile", [Str(path)]) => Answer::Bool(std::path::Path::new(&**path).is_file()),
            ("isDir", [Str(path)]) => Answer::Bool(std::path::Path::new(&**path).is_dir()),
            // Test
            ("fail", [Str(message)]) => return Some(Err(Stop::Failed(message.to_string()))),
            // what the prelude's `Abort` does unhandled: the message, as an
            // error, and exit status 1 (on the shared back end, a handler of
            // the front end's over `putErr` and `exit`)
            ("abort", [Str(message)]) => {
                self.print_error(&format!("{message}\n"));
                return Some(Err(Stop::Exit(1)));
            }
            _ => return None,
        };
        Some(Ok(answer))
    }
}

/// Nanoseconds since some fixed point in this process's life; only the
/// differences mean anything.
fn monotonic_nanos() -> u128 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_nanos()
}

/// `Log`'s level names, by rank, padded to one width so messages line up.
const LOG_LEVELS: [&str; 4] = ["DEBUG", "INFO ", "WARN ", "ERROR"];

/// The least level a live world writes, as `MEADOW_LOG` names it (`debug`,
/// `info`, `warn`, `error` or `off`), `info` otherwise; read once.
fn log_threshold() -> i64 {
    static AT: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *AT.get_or_init(|| match std::env::var("MEADOW_LOG").ok().map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("debug" | "trace" | "all") => 0,
        Some("warn" | "warning") => 2,
        Some("error") => 3,
        Some("off" | "none") => 4,
        _ => 1,
    })
}

/// `when` in UTC as RFC 3339, to the millisecond, as Meadow's runtime
/// writes it: `2026-10-17T19:12:04.512Z`.
fn rfc3339(when: std::time::SystemTime) -> String {
    let since = when.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, since.subsec_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_captured_world_keeps_what_is_printed_and_reads_what_it_is_given() {
        let mut w = World::captured().with_input(&["first"]);
        assert_eq!(w.perform("putStr", &[Arg::Str("hi\n".into())]), Some(Ok(Answer::Unit)));
        assert_eq!(w.perform("readLine", &[Arg::Int(0)]), Some(Ok(Answer::just(Answer::str("first")))));
        assert_eq!(w.perform("readLine", &[Arg::Int(0)]), Some(Ok(Answer::nothing())));
        assert_eq!(w.output(), "hi\n");
    }

    #[test]
    fn a_captured_worlds_randomness_is_the_same_every_time() {
        let draw = || {
            let mut w = World::captured();
            (0..5).map(|_| w.perform("intBetween", &[Arg::Int(10), Arg::Int(20)])).collect::<Vec<_>>()
        };
        let first = draw();
        assert_eq!(first, draw());
        for a in first {
            let Some(Ok(Answer::Int(n))) = a else { panic!("{a:?}") };
            assert!((10..20).contains(&n));
        }
    }

    #[test]
    fn files_can_be_written_read_and_listed() {
        let dir = std::env::temp_dir().join(format!("idyllic-world-{}", std::process::id()));
        let d = |p: &str| Arg::Str(dir.join(p).to_string_lossy().into_owned().into());
        let mut w = World::captured();
        let ok = |a| Some(Ok(Answer::Con("Ok", 1, vec![a])));
        assert_eq!(w.perform("createDirAll", &[d("")]), ok(Answer::Unit));
        assert_eq!(w.perform("writeString", &[d("a.txt"), Arg::Str("one".into())]), ok(Answer::Unit));
        assert_eq!(w.perform("appendString", &[d("a.txt"), Arg::Str(" two".into())]), ok(Answer::Unit));
        assert_eq!(w.perform("readToString", &[d("a.txt")]), ok(Answer::str("one two")));
        assert_eq!(w.perform("listDir", &[d("")]), ok(Answer::Array(vec![Answer::str("a.txt")])));
        // a copy answers how many bytes it copied, as the runtime's
        // `Fs.copy` does
        assert_eq!(w.perform("copy", &[d("a.txt"), d("b.txt")]), ok(Answer::Int(7)));
        assert_eq!(w.perform("readToString", &[d("b.txt")]), ok(Answer::str("one two")));
        assert!(matches!(w.perform("copy", &[d("missing"), d("c.txt")]), Some(Ok(Answer::Con("Err", 0, _)))));
        assert_eq!(w.perform("isFile", &[d("a.txt")]), Some(Ok(Answer::Bool(true))));
        assert!(matches!(w.perform("readToString", &[d("missing")]), Some(Ok(Answer::Con("Err", 0, _)))));
        assert_eq!(w.perform("removeDirAll", &[d("")]), ok(Answer::Unit));
        assert_eq!(w.perform("exists", &[d("")]), Some(Ok(Answer::Bool(false))));
    }

    #[test]
    fn exit_and_fail_stop_the_program() {
        let mut w = World::captured();
        assert_eq!(w.perform("exit", &[Arg::Int(3)]), Some(Err(Stop::Exit(3))));
        assert_eq!(w.perform("fail", &[Arg::Str("no".into())]), Some(Err(Stop::Failed("no".into()))));
        assert_eq!(w.perform("nothing", &[]), None);
    }
}
