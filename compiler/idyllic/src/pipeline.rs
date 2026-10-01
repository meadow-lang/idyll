//! The compiler pipeline: lex → layout → parse → rename → SCC → type check,
//! and then — for a program that checks — lower to AxCut → linearize → run.
//!
//! Every pass reports plain [`Diagnostic`]s, collected here in pass order.  A
//! parse failure does not short-circuit the report: whatever the lexer and
//! layout already found is still shown.

use idyllic_diagnostics::{self as diagnostics, Diagnostic, OutputStyle};
use idyllic_span::{LineIndex, SourceMap, Span, Spanned};
use idyllic_llvm as llvm;
use idyllic_world::World;
use {
    idyllic_seq as seq, idyllic_layout as layout, idyllic_lexer as lexer, idyllic_parser as parser,
    idyllic_rename as rename, idyllic_scc as scc, idyllic_typecheck as typecheck,
};

#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Dump the HIR after the SCC pass.
    pub debug: bool,
    pub style: OutputStyle,
    /// Whether a program talks to the real standard streams as it runs (the
    /// REPL, the compiler run on a file), or to a captured world whose output
    /// is kept in [`Output::printed`] and whose input is empty (tests).
    pub live: bool,
}

pub struct Output {
    /// Every diagnostic, in pass order.
    pub diagnostics: Vec<Diagnostic>,
    /// The type of the program's trailing expression, if it has one and
    /// checking got that far.
    pub body_type: Option<String>,
    /// Each top-level definition and its type, in source order.
    pub decls: Vec<(String, String)>,
    /// The HIR dump, when [`Options::debug`] is set.
    pub hir: Option<String>,
    /// The program as AxCut, if it checked and has an expression to run.
    pub axcut: Option<seq::Program>,
    /// The same, inlined: what the native compiler is given.  (What is run
    /// here, and shown, is the program as it was lowered.)
    pub native: Option<seq::Program>,
    /// What running it came to.
    pub evaluation: Option<Evaluation>,
    /// What running it wrote to the console, if its world was captured (see
    /// [`Options::live`]); a live one has written it already.
    pub printed: String,
    /// Whether what was run is `main ()`, for a program with a `main` and no
    /// expression of its own.  Its value, `()`, is not shown.
    pub ran_main: bool,
    /// What it did with memory, if it ran to the end.
    pub memory: Option<seq::machine::Memory>,
    /// How compiled code should print the answer.
    pub result_kind: llvm::ResultKind,
    /// The elaborated program, if checking got that far: what is lowered,
    /// and what the reference interpreter (`idyll-eval`) runs.
    pub checked: Option<typecheck::TypeCheckResult>,
    /// Every file the program was read from, its own first and the
    /// prelude's last, in the one address space their spans share.
    pub sources: SourceMap,
}

/// What a package is compiled against: every package it depends on,
/// directly or not, compiled, in an order where each follows those it
/// depends on; the ones it depends on directly, by the names it calls them;
/// and the first unique it may name its own things with, so that no two
/// packages of a build share one.
#[derive(Clone, Copy, Default)]
pub struct Deps<'a> {
    pub all: &'a [&'a CompiledPackage],
    pub direct: &'a [(&'a str, &'a CompiledPackage)],
    pub first_unique: usize,
}


pub use typecheck::packages::CompiledPackage;

/// A package to compile: its name, and its modules' files, the root module
/// first.
#[derive(Clone, Debug)]
pub struct PackageSource {
    pub name: String,
    pub modules: Vec<ModuleFile>,
}

/// One module of a package: where it is in the package (the root is the
/// empty path), what file it was read from, and what that says.
#[derive(Clone, Debug)]
pub struct ModuleFile {
    pub path: Vec<String>,
    pub file: String,
    pub text: String,
}

impl PackageSource {
    /// A package of one module, the text given.
    pub fn single(name: &str, file: &str, text: &str) -> PackageSource {
        let root = ModuleFile { path: Vec::new(), file: file.to_string(), text: text.to_string() };
        PackageSource { name: name.to_string(), modules: vec![root] }
    }
}

/// Read a package from a directory: its name from `Idyll.toml`'s `[package]`
/// (or the directory's), and its modules from `src/`, where `Main.idl` or
/// `Lib.idl` is the root and `src/Math.idl` is the module `Math`.  Module
/// files, and directories under `src/`, are PascalCase, since each is a name
/// in a `use` path.
pub fn load_package(dir: &std::path::Path) -> Result<PackageSource, String> {
    let manifest = dir.join("Idyll.toml");
    let name = match std::fs::read_to_string(&manifest) {
        Ok(text) => manifest_name(&text).ok_or_else(|| format!("{} has no [package] name", manifest.display()))?,
        Err(_) => dir
            .canonicalize()
            .ok()
            .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
            .ok_or_else(|| format!("cannot tell what {} is called", dir.display()))?,
    };
    let src = dir.join("src");
    let mut files = Vec::new();
    collect_modules(&src, &mut Vec::new(), &mut files)?;
    let roots: Vec<usize> = files.iter().enumerate().filter(|(_, (p, _))| matches!(&p[..], [r] if r == "Main" || r == "Lib")).map(|(i, _)| i).collect();
    let root = match roots.as_slice() {
        [i] => *i,
        [] => return Err(format!("{} has no src/Main.idl or src/Lib.idl", dir.display())),
        _ => return Err(format!("{} has both src/Main.idl and src/Lib.idl; a package has one root", dir.display())),
    };
    let mut modules = Vec::new();
    for (i, (path, file)) in files.into_iter().enumerate() {
        let text = std::fs::read_to_string(&file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        let shown = file.strip_prefix(dir).unwrap_or(&file).to_string_lossy().replace('\\', "/");
        let path = if i == root { Vec::new() } else { path };
        modules.push(ModuleFile { path, file: shown, text });
    }
    modules.swap(0, root);
    Ok(PackageSource { name, modules })
}

/// `name = "..."` under `[package]`, from an `Idyll.toml`.  (The build system
/// will read the rest.)
fn manifest_name(text: &str) -> Option<String> {
    let mut in_package = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if let (true, Some(rest)) = (in_package, line.strip_prefix("name")) {
            let value = rest.trim_start().strip_prefix('=')?.trim();
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

/// The `.idl` files under `dir`, each with its module path.
fn collect_modules(
    dir: &std::path::Path,
    path: &mut Vec<String>,
    out: &mut Vec<(Vec<String>, std::path::PathBuf)>,
) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    entries.sort();
    let pascal = |name: &str, what: &str, file: &std::path::Path| {
        if name.starts_with(|c: char| c.is_ascii_uppercase()) {
            return Ok(());
        }
        let mut fixed = name.to_string();
        if let Some(first) = fixed.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        Err(format!("{} is a {what}, and so a name in a use path, which is PascalCase: rename it to {fixed}", file.display()))
    };
    for entry in entries {
        let stem = entry.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if entry.is_dir() {
            pascal(&stem, "module directory", &entry)?;
            path.push(stem);
            collect_modules(&entry, path, out)?;
            path.pop();
        } else if entry.extension().is_some_and(|e| e == "idl") {
            pascal(&stem, "module file", &entry)?;
            let mut module = path.clone();
            module.push(stem);
            out.push((module, entry));
        }
    }
    Ok(())
}

/// What every program starts with.
///
/// Lists are an ordinary data type; what is built in is only the spelling of
/// the constructor, an infix `::`, and that a sequence literal with nothing
/// to say otherwise is one.  Constructors live under their types, and the
/// prelude's `use`s are what put `Nil`, `::`, `Nothing`, `Just`, `Err` and
/// `Ok` in every program's scope bare.
///
/// `Eff` is the effects the runtime handles, which are what a program's top
/// level, and so its `main`, may have: Meadow's, less the ones Idyll has no
/// means for yet (`Mut`'s references and `Thread`'s threads).  An operation
/// of one of them that no handler of the program's own takes is done by the
/// runtime (see `idyllic_world`, and `rts/idyll_rt.c` for native code).
pub const PRELUDE: &str = "\
@seq(nil = Nil, cons = (::))
data List a = Nil | (::) a (List a)

data Maybe a = Nothing | Just a

data Result e a = Err e | Ok a

use List.*
use Maybe.*
use Result.*

effect Console where
  putStr : String -> ()
  putStrLn : String -> ()
  printInt : Int -> ()
  readLine : () -> Maybe String

effect Fs where
  readToString : String -> Result String String
  writeString : String -> String -> Result String ()
  appendString : String -> String -> Result String ()
  removeFile : String -> Result String ()
  createDir : String -> Result String ()
  createDirAll : String -> Result String ()
  removeDir : String -> Result String ()
  removeDirAll : String -> Result String ()
  rename : String -> String -> Result String ()
  copy : String -> String -> Result String Int
  readDir : String -> Result String (List String)
  exists : String -> Bool
  isFile : String -> Bool
  isDir : String -> Bool

effect Process where
  exit : Int -> ()
  currentPid : () -> Int
  args : () -> List String
  getEnv : String -> Maybe String
  setEnv : String -> String -> ()
  removeEnv : String -> ()
  isTerminal : Int -> Bool

effect Random where
  nextInt : () -> Int
  intBetween : Int -> Int -> Int
  nextSeed : () -> Int

effect Time where
  now : () -> Int
  monotonic : () -> Int
  sleep : Int -> ()

effect Test where
  fail : String -> ()

effect Eff = {Console, Fs, Process, Random, Time, Test, Div}
";

/// Lex, lay out and parse one file, added to `map`, so that its spans are
/// where it is there.
fn read(map: &mut SourceMap, name: &str, text: &str, diagnostics: &mut Vec<Diagnostic>) -> Option<idyllic_ast::Program> {
    let file = map.add(name, text);
    let base = map.file(file).base;
    let lexed = lexer::tokenize(text);
    diagnostics.extend(lexed.diagnostics.into_iter().map(|d| d.shifted(base)));
    let laid_out = layout::layout(&LineIndex::new(text), &lexed.tokens);
    diagnostics.extend(laid_out.errors.iter().map(|e| e.to_diagnostic().shifted(base)));
    let tokens: Vec<Spanned<idyllic_lexer::token::Token>> = laid_out
        .tokens
        .into_iter()
        .map(|t| Spanned::new(t.node, Span::new(t.span.start + base, t.span.end + base)))
        .collect();
    match parser::parse(base + text.len(), &tokens) {
        Ok(program) => Some(program),
        Err(errs) => {
            diagnostics.extend(errs);
            None
        }
    }
}

/// How many steps the AxCut machine gives a program before giving up
/// on it.  Nothing checks that a recursion ends; and one that does, but not
/// soon, is what the native compiler is for.
pub const FUEL: u64 = 1_000_000;

/// What running a program came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Evaluation {
    /// A value that can be shown: a number, a string, a box of one, ...
    Value(String),
    /// A value with nothing to show: a function, a type.
    Opaque,
    /// Still running after [`FUEL`] steps.
    OutOfFuel,
    /// Stopped by an error of the program's own: dividing by zero, a failed
    /// test.
    Stopped(String),
    /// Ended by the program itself, with `exit`, and this exit code.
    Exited(i32),
    /// The compiler produced a program it could not run.  That is a bug in
    /// the compiler, never in the program.
    Failed(String),
}

/// What [`evaluate`] comes to: the program as run, and as compiled; what
/// running it came to, did with memory, and printed.
struct Ran {
    program: seq::Program,
    native: Option<seq::Program>,
    evaluation: Evaluation,
    memory: Option<seq::machine::Memory>,
    printed: String,
}

/// Lower, hold the result to its grades, infer the substitutions, and run,
/// in `world`; with none, only lower.
fn evaluate(checked: &typecheck::TypeCheckResult, world: Option<World>) -> Option<Ran> {
    let failed = |program, why: String| Ran {
        program,
        native: None,
        evaluation: Evaluation::Failed(why),
        memory: None,
        printed: String::new(),
    };
    let lowered = match seq::lower::lower(checked)? {
        Ok(lowered) => lowered,
        Err(e) => return Some(failed(seq::Program::default(), e.to_string())),
    };
    let program = seq::linearize::linearize(&lowered);
    // a variable used more often than its grade allows: the type checker
    // and the lowering disagree, and one of them is wrong
    if let Some(violation) = seq::grades::check(&lowered).first() {
        return Some(failed(program, violation.to_string()));
    }
    // only now: inlining lets an argument be used as its parameter was,
    // which is not what the argument's own grade says
    let native = Some(seq::linearize::linearize(&seq::inline::inline(&lowered)));
    let Some(mut world) = world else {
        let evaluation = Evaluation::Opaque;
        return Some(Ran { program, native, evaluation, memory: None, printed: String::new() });
    };
    use seq::machine::MachineError;
    let (evaluation, memory) = match seq::machine::run_in(&program, FUEL, &mut world) {
        Ok(outcome) if outcome.memory.leaked + outcome.memory.miscounted > 0 => {
            let m = outcome.memory;
            let why = format!("{} blocks leaked and {} miscounted", m.leaked, m.miscounted);
            (Evaluation::Failed(why), Some(m))
        }
        Ok(outcome) => (outcome.answer.map_or(Evaluation::Opaque, Evaluation::Value), Some(outcome.memory)),
        Err(MachineError::OutOfFuel) => (Evaluation::OutOfFuel, None),
        Err(MachineError::Exited(code)) => (Evaluation::Exited(code), None),
        Err(e @ (MachineError::DivisionByZero | MachineError::TestFailed(_))) => (Evaluation::Stopped(e.to_string()), None),
        Err(e) => (Evaluation::Failed(e.to_string()), None),
    };
    let printed = world.into_output();
    Some(Ran { program, native, evaluation, memory, printed })
}

impl Output {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }

    /// The program as it was lowered to AxCut, and what running it did with
    /// memory, for `--axcut`; `None` if it never got as far as lowering.
    pub fn axcut_report(&self) -> Option<String> {
        let program = self.axcut.as_ref()?;
        let mut out = format!("{program}\n\n");
        if let Some(memory) = &self.memory {
            out.push_str(&format!("{memory}\n\n"));
        }
        Some(out)
    }

    /// Everything a user should see: rendered diagnostics, then what the
    /// program printed, then the result — the value and type of the trailing
    /// expression (`3 : Int`; just `: Int -> Int` when the value is nothing
    /// that can be shown), or failing one, the types of the definitions.  A
    /// run of `main` shows only what it printed.
    pub fn display(&self, name: &str, src: &str, style: OutputStyle) -> String {
        // (the program's own first file is the one called `name`)
        let _ = src;
        let mut sources = self.sources.clone();
        if !sources.files().is_empty() {
            sources.set_name(0, name);
        }
        let mut out = diagnostics::render_map(&self.diagnostics, &sources, style);
        if let Some(hir) = &self.hir {
            out.push_str(hir);
            out.push('\n');
        }
        out.push_str(&self.printed);
        match &self.body_type {
            Some(ty) => {
                if !self.ran_main {
                    if let Some(Evaluation::Value(v)) = &self.evaluation {
                        out.push_str(v);
                        out.push(' ');
                    }
                    out.push_str(": ");
                    out.push_str(ty);
                    out.push('\n');
                }
                match &self.evaluation {
                    Some(Evaluation::OutOfFuel) => {
                        out.push_str(&format!("(still running after {FUEL} steps; stopped)\n"));
                    }
                    Some(Evaluation::Stopped(why)) => out.push_str(&format!("(stopped: {why})\n")),
                    // (a program run for its `main` says nothing of it)
                    Some(Evaluation::Exited(code)) if !self.ran_main => {
                        out.push_str(&format!("(exited with {code})\n"));
                    }
                    Some(Evaluation::Failed(why)) => {
                        out.push_str(&format!("(could not be run — this is a bug in idyllic: {why})\n"));
                    }
                    _ => {}
                }
                // as before: no newline of its own at the end
                if out.ends_with('\n') {
                    out.pop();
                }
            }
            None => {
                let decls: Vec<String> = self
                    .decls
                    .iter()
                    .map(|(x, ty)| format!("{x} : {ty}"))
                    .collect();
                out.push_str(&decls.join("\n"));
            }
        }
        out
    }
}

/// Run the whole pipeline over one source text.  Never fails.  A program
/// with a `main` and no expression of its own runs `main ()`.
pub fn run(src: &str, opts: &Options) -> Output {
    run_with(src, opts, true, Stage::Run(world(opts, Vec::new())))
}

/// The same, for a program given command-line arguments: what `args ()`
/// gives it.
pub fn run_program(src: &str, opts: &Options, args: Vec<String>) -> Output {
    run_with(src, opts, true, Stage::Run(world(opts, args)))
}

/// Everything short of running: what the native compiler wants.  (Running
/// a program to compile it would do what the program does, to the world.)
pub fn compile(src: &str, opts: &Options) -> Output {
    run_with(src, opts, true, Stage::Lower)
}

/// A package, run (with these arguments for its `main`).
pub fn run_package(package: &PackageSource, opts: &Options, args: Vec<String>) -> Output {
    build(package, Deps::default(), opts, true, Stage::Run(world(opts, args)))
}

/// A package, everything short of running it.
pub fn compile_package(package: &PackageSource, opts: &Options) -> Output {
    build(package, Deps::default(), opts, true, Stage::Lower)
}

/// A package compiled against what it depends on, and run (with these
/// arguments for its `main`): it is linked with them first.
pub fn run_package_with(package: &PackageSource, deps: Deps<'_>, opts: &Options, args: Vec<String>) -> Output {
    build(package, deps, opts, true, Stage::Run(world(opts, args)))
}

/// A package compiled against what it depends on, and linked with them if
/// it is a program, but not run: what a build keeps, and what native code
/// is made from.
pub fn compile_package_with(package: &PackageSource, deps: Deps<'_>, opts: &Options) -> Output {
    build(package, deps, opts, true, Stage::Lower)
}

fn world(opts: &Options, args: Vec<String>) -> World {
    if opts.live {
        World::live(args)
    } else {
        World::captured()
    }
}

/// A package checked against what it depends on, and no more: neither
/// linked nor lowered.  What a build compiles each package of its graph
/// with, keeping `checked.compiled`; `entry` says whether a `main` of its
/// own is to be run.
pub fn check_package_with(package: &PackageSource, deps: Deps<'_>, opts: &Options, entry: bool) -> Output {
    build(package, deps, opts, entry, Stage::Check)
}

/// A program linked from compiled packages alone, and run (with these
/// arguments for its `main`): every package it uses, in an order where each
/// follows those it depends on, the program's own last.  What a build runs
/// when nothing needed compiling.
pub fn run_compiled(packages: &[&CompiledPackage], opts: &Options, args: Vec<String>) -> Output {
    linked(packages, opts, Stage::Run(world(opts, args)))
}

/// The same, everything short of running it: what native code is made from.
pub fn link_compiled(packages: &[&CompiledPackage], opts: &Options) -> Output {
    linked(packages, opts, Stage::Lower)
}

fn linked(packages: &[&CompiledPackage], opts: &Options, stage: Stage) -> Output {
    let (root, deps) = packages.split_last().expect("a program has a package of its own");
    // the prelude, as every package has it: its definitions are the
    // outermost of the program, and its tables are everyone's
    let mut out = build(&PackageSource::single("Main", "<link>", ""), Deps::default(), opts, false, Stage::Check);
    let Some(mut checked) = out.checked.take() else { return out };
    checked.compiled = Some((*root).clone());
    typecheck::packages::link(&mut checked, deps);
    finish(checked, out.diagnostics, out.sources, Vec::new(), None, stage)
}

/// How far to take a program.
enum Stage {
    /// check it
    Check,
    /// and lower it, for the machine and for native code
    Lower,
    /// and run it, in this world
    Run(World),
}

fn run_with(src: &str, opts: &Options, entry: bool, stage: Stage) -> Output {
    build(&PackageSource::single("Main", "<input>", src), Deps::default(), opts, entry, stage)
}

fn build(package: &PackageSource, deps: Deps<'_>, opts: &Options, entry: bool, stage: Stage) -> Output {
    let mut diagnostics = Vec::new();
    let mut sources = SourceMap::default();
    let programs: Vec<Option<idyllic_ast::Program>> =
        package.modules.iter().map(|m| read(&mut sources, &m.file, &m.text, &mut diagnostics)).collect();
    let programs: Option<Vec<idyllic_ast::Program>> = programs.into_iter().collect();
    let asts = match programs {
        Some(asts) => asts,
        None => {
            return Output {
                sources,
                diagnostics,
                body_type: None,
                decls: Vec::new(),
                hir: None,
                axcut: None,
                native: None,
                evaluation: None,
                printed: String::new(),
                ran_main: false,
                memory: None,
                result_kind: llvm::ResultKind::Other,
                checked: None,
            };
        }
    };

    // name resolution -> HIR, then SCC / retopologize, then type check
    let prelude = read(&mut sources, "<prelude>", PRELUDE, &mut diagnostics).expect("the prelude parses");
    let modules: Vec<rename::ModuleSource<'_>> = package
        .modules
        .iter()
        .zip(&asts)
        .map(|(m, program)| rename::ModuleSource { path: m.path.clone(), program })
        .collect();
    let direct: Vec<(&str, &idyllic_hir::PackageNames)> = deps.direct.iter().map(|(n, p)| (*n, &p.names)).collect();
    let first_unique = deps.first_unique.max(idyllic_builtins::FIRST_USER_UNIQUE);
    let renamed = rename::rename_package(&package.name, &modules, &prelude, &direct, first_unique);
    diagnostics.extend(
        renamed
            .errors
            .iter()
            .map(rename::RenameError::to_diagnostic),
    );
    let module = scc::scc_module(renamed.module);
    let mut checked = typecheck::check_module(&module, renamed.spans.len(), entry, deps.all);
    diagnostics.extend(
        checked
            .errors
            .iter()
            .map(|e| e.to_diagnostic(&renamed.spans)),
    );
    let decls = {
        // in the order they were written
        let mut decls: Vec<_> = checked.decl_types.iter().collect();
        decls.sort_by_key(|(id, ..)| renamed.spans.get(*id).map(|s| s.start));
        decls.into_iter().map(|(_, x, ty)| (x.clone(), ty.to_string())).collect()
    };
    let hir = opts.debug.then(|| format!("{module:#?}"));

    // a program that uses other packages is put together with them
    let checks = !diagnostics.iter().any(Diagnostic::is_error);
    if !matches!(stage, Stage::Check) && !deps.all.is_empty() && checks && checked.program.is_some() {
        typecheck::packages::link(&mut checked, deps.all);
    }
    finish(checked, diagnostics, sources, decls, hir, stage)
}

/// The rest of the pipeline, for a program checked (and linked).
fn finish(
    checked: typecheck::TypeCheckResult,
    diagnostics: Vec<Diagnostic>,
    sources: SourceMap,
    decls: Vec<(String, String)>,
    hir: Option<String>,
    stage: Stage,
) -> Output {
    let ran_main = checked.ran_main;
    // only a program that checks is run: the machine takes the types' word
    // for it that every call has a function and every unboxing a box
    let ran = match stage {
        _ if diagnostics.iter().any(Diagnostic::is_error) => None,
        Stage::Check => None,
        Stage::Lower => evaluate(&checked, None),
        Stage::Run(world) => evaluate(&checked, Some(world)),
    };
    let (axcut, native, evaluation, memory, printed) = match ran {
        Some(r) => (Some(r.program), r.native, Some(r.evaluation), r.memory, r.printed),
        None => (None, None, None, None, String::new()),
    };

    use idyllic_builtins::Builtin;
    use idyllic_typecheck::core::Core;
    let result_kind = match checked.body_type.as_deref() {
        _ if ran_main => llvm::ResultKind::Main,
        Some(Core::LitTy(Builtin::Int)) => llvm::ResultKind::Int,
        Some(Core::LitTy(Builtin::Bool)) => llvm::ResultKind::Bool,
        Some(Core::LitTy(Builtin::Unit)) => llvm::ResultKind::Unit,
        Some(Core::LitTy(Builtin::Str)) => llvm::ResultKind::Str,
        _ => llvm::ResultKind::Other,
    };

    Output {
        diagnostics,
        result_kind,
        axcut,
        native,
        evaluation,
        printed,
        ran_main,
        memory,
        body_type: checked.body_type.as_ref().map(|ty| ty.to_string()),
        decls,
        hir,
        checked: Some(checked),
        sources,
    }
}

/// An interactive session: the declarations entered so far stay in scope for
/// later entries.
///
/// They are kept as source, and checked again in front of each new entry.
/// That is slower than keeping the checker's state, but it cannot get out of
/// step with it, and a REPL's worth of declarations checks in no time.
#[derive(Default)]
pub struct Session {
    source: String,
    /// how many definitions `source` makes
    seen: usize,
}

/// The result of one entry.  Spans in `output` index `source`, which is the
/// session's declarations followed by the entry.
pub struct Entered {
    pub output: Output,
    pub source: String,
}

impl Session {
    /// Run one entry.  Only what is new is reported: the earlier entries'
    /// diagnostics and definitions have been shown already.  An entry made
    /// of declarations alone, with no errors, extends the session.
    pub fn enter(&mut self, entry: &str, opts: &Options) -> Entered {
        let source = format!("{}{}\n", self.source, entry.trim_end());
        // defining `main` is not running it: that is `main ()`
        let mut output = run_with(&source, opts, false, Stage::Run(world(opts, Vec::new())));
        let earlier = self.source.len();
        output
            .diagnostics
            .retain(|d| d.labels.is_empty() || d.labels.iter().any(|l| l.span.start >= earlier));
        let defined = output.decls.len();
        output.decls.drain(..self.seen.min(defined));
        // (an entry of declarations is kept even if it defines nothing new:
        // `use Nat.*` is one)
        if output.body_type.is_none() && !output.has_errors() {
            self.seen = defined;
            self.source = source.clone();
        }
        Entered { output, source }
    }

    /// Forget every declaration.
    pub fn reset(&mut self) {
        *self = Session::default();
    }
}
