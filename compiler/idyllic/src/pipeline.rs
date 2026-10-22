//! The compiler pipeline: lex → layout → parse → rename → SCC → type check,
//! and then — for a program that checks — lower to Cut, and run it on
//! Meadow's runtimes ([`crate::meadow`]).
//!
//! Every pass reports plain [`Diagnostic`]s, collected here in pass order.  A
//! parse failure does not short-circuit the report: whatever the lexer and
//! layout already found is still shown.

use idyllic_diagnostics::{self as diagnostics, Diagnostic, OutputStyle};
use idyllic_span::{LineIndex, SourceMap, Span, Spanned};
use crate::meadow::{self, Outcome, Running};
use {
    idyllic_layout as layout, idyllic_lexer as lexer, idyllic_parser as parser,
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
    /// The program as Cut, which Meadow's runtimes run, if it checked and
    /// was lowered.
    pub cut: Option<String>,
    /// What lowering it said, if it could not be lowered.
    pub not_lowered: Vec<String>,
    /// What running it came to.
    pub evaluation: Option<Evaluation>,
    /// What running it wrote to the console, if its world was captured (see
    /// [`Options::live`]); a live one has written it already.
    pub printed: String,
    /// What it wrote as errors (standard error), likewise.
    pub errors: String,
    /// Whether what was run is `main ()`, for a program with a `main` and no
    /// expression of its own.  Its value, `()`, is not shown.
    pub ran_main: bool,
    /// The elaborated program, if checking got that far: what is lowered,
    /// and what the reference interpreter (`idyll-eval`) runs.
    pub checked: Option<typecheck::TypeCheckResult>,
    /// Every file the program was read from, its own first and the
    /// prelude's last, in the one address space their spans share.
    pub sources: SourceMap,
    /// Every name the program and the prelude declare, if checking got that
    /// far: what the REPL lists and searches.
    pub declarations: Vec<Declaration>,
}

/// A name a program can use, with what a person looking for it wants to
/// know.
#[derive(Clone, Debug)]
pub struct Declaration {
    /// as a program writes it: `double`, `Nat.Z`, `Math.double`
    pub name: String,
    pub kind: typecheck::DeclKind,
    /// its type (a data type's or an effect's kind), and that as written
    pub ty: std::rc::Rc<typecheck::core::Core>,
    pub shown: String,
    pub prelude: bool,
    /// where it is declared, in the program's address space
    pub span: Span,
    /// the file, and the one-based line and column there
    pub file: String,
    pub line: usize,
    pub column: usize,
    /// the `--` comment on the lines just above it, if there is one
    pub doc: Option<String>,
    /// the package it comes from, if a dependency (`Std`) declares it
    pub package: Option<String>,
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
    /// its manifest's version, which the symbols a back end is given carry
    /// (`idyll:Util@0.1.0/double`); none for a lone file, the REPL or `Std`
    pub version: Option<String>,
    /// whether every module of it is linear, as if each said `@linear`:
    /// `idyllic --linear`
    pub linear: bool,
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
        PackageSource { name: name.to_string(), version: None, linear: false, modules: vec![root] }
    }
}

/// Read a package from a directory: its name from `Idyll.toml`'s `[package]`
/// (or the directory's), and its modules from `src/`, where `Main.idl` or
/// `Lib.idl` is the root and `src/Math.idl` is the module `Math`.  Module
/// files, and directories under `src/`, are PascalCase, since each is a name
/// in a `use` path.
pub fn load_package(dir: &std::path::Path) -> Result<PackageSource, String> {
    let manifest = dir.join("Idyll.toml");
    let version = std::fs::read_to_string(&manifest).ok().and_then(|text| manifest_field(&text, "version"));
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
    Ok(PackageSource { name, version, linear: false, modules })
}

/// `name = "..."` under `[package]`, from an `Idyll.toml`.  (The build system
/// will read the rest.)
fn manifest_name(text: &str) -> Option<String> {
    manifest_field(text, "name")
}

/// A `[package]` field of a manifest, as a string: `name`, `version`.
fn manifest_field(text: &str, field: &str) -> Option<String> {
    let mut in_package = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if let (true, Some(rest)) = (in_package, line.strip_prefix(field)) {
            let Some(value) = rest.trim_start().strip_prefix('=') else { continue };
            return Some(value.trim().trim_matches('"').to_string());
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

/// What every program starts with: `prelude.idl`, beside this file, which
/// front/ (the compiler in Meadow) embeds too.
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
/// of one of them that no handler of the program's own takes is done by
/// Meadow's runtime, as the operation of its own it is declared `native` to
/// be (`docs/CUT.md` in Meadow).
pub const PRELUDE: &str = include_str!("prelude.idl");

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
    match parser::parse_file(base, base + text.len(), &tokens) {
        Ok(program) => Some(program),
        Err(errs) => {
            diagnostics.extend(errs);
            None
        }
    }
}

/// How long a program run with its output kept (a test's), or a REPL
/// entry, may run before it is stopped: `IDYLL_TIMEOUT` seconds, or 20.
/// Nothing checks that a recursion ends. A program run live (`idyll run`)
/// has as long as it likes.
pub fn timeout() -> std::time::Duration {
    let secs = std::env::var("IDYLL_TIMEOUT").ok().and_then(|s| s.parse().ok()).unwrap_or(20);
    std::time::Duration::from_secs(secs)
}

/// What running a program came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Evaluation {
    /// A value that can be shown: a number, a string, a box of one, ...
    Value(String),
    /// A value with nothing to show: a function, a type.
    Opaque,
    /// Still running when its time was up.
    TimedOut(std::time::Duration),
    /// Stopped by an error of the program's own: dividing by zero, a failed
    /// test.
    Stopped(String),
    /// Ended by the program itself, with `exit`, and this exit code.
    Exited(i32),
    /// The program could not be run: the compiler made Cut Meadow would not
    /// take, or there is no `meadow` to run it. Never the program's fault.
    Failed(String),
}

/// What [`evaluate`] comes to: the program as Cut, what running it came to,
/// and what it printed.
struct Ran {
    cut: Option<String>,
    not_lowered: Vec<String>,
    evaluation: Option<Evaluation>,
    printed: String,
    errors: String,
}

/// Lower to Cut, and run it as `running` says; with nothing to say, only
/// lower.
fn evaluate(checked: &mut typecheck::TypeCheckResult, running: Option<&Running>) -> Ran {
    let lowered = match running {
        // (run, the answer is marked, to be told from what was printed)
        Some(_) => idyllic_cut::lower::lower_marked(checked, "Main", &meadow::ANSWER_MARK.to_string()),
        None => idyllic_cut::lower::lower(checked, "Main"),
    };
    let cut = match lowered {
        Ok(program) => program.to_string(),
        Err(why) => {
            let evaluation = running.map(|_| Evaluation::Failed(format!("not lowered to Cut: {}", why.join("; "))));
            return Ran { cut: None, not_lowered: why, evaluation, printed: String::new(), errors: String::new() };
        }
    };
    let Some(running) = running else {
        return Ran { cut: Some(cut), not_lowered: Vec::new(), evaluation: None, printed: String::new(), errors: String::new() };
    };
    let ran = meadow::run(&cut, running);
    let evaluation = match ran.outcome {
        Outcome::Answered(Some(answer)) => Evaluation::Value(answer),
        Outcome::Answered(None) => Evaluation::Opaque,
        Outcome::Exited(code) => Evaluation::Exited(code),
        Outcome::Stopped(why) => Evaluation::Stopped(why),
        Outcome::TimedOut(after) => Evaluation::TimedOut(after),
        Outcome::Failed(why) => Evaluation::Failed(why),
    };
    Ran { cut: Some(cut), not_lowered: Vec::new(), evaluation: Some(evaluation), printed: ran.printed, errors: ran.errors }
}

impl Output {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }

    /// The program as it was lowered to Cut, for `--cut`; `None` if it
    /// never got as far as lowering.
    pub fn cut_report(&self) -> Option<String> {
        self.cut.as_ref().map(|cut| format!("{cut}\n"))
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
                    Some(Evaluation::TimedOut(after)) => {
                        out.push_str(&format!("(still running after {} seconds; stopped)\n", after.as_secs()));
                    }
                    Some(Evaluation::Stopped(why)) => out.push_str(&format!("(stopped: {why})\n")),
                    // (a program run for its `main` says nothing of it)
                    Some(Evaluation::Exited(code)) if !self.ran_main => {
                        out.push_str(&format!("(exited with {code})\n"));
                    }
                    Some(Evaluation::Failed(why)) => {
                        out.push_str(&format!("(could not be run: {why})\n"));
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
    run_with(src, opts, true, Stage::Run(running(opts, Vec::new())))
}

/// The same, stopped if it is still running after `timeout`, rather than
/// [`timeout()`]'s.
pub fn run_within(src: &str, opts: &Options, timeout: std::time::Duration) -> Output {
    run_with(src, opts, true, Stage::Run(Running { timeout: Some(timeout), ..running(opts, Vec::new()) }))
}

/// The same, for a program given command-line arguments: what `args ()`
/// gives it.
pub fn run_program(src: &str, opts: &Options, args: Vec<String>) -> Output {
    run_with(src, opts, true, Stage::Run(running(opts, args)))
}

/// Everything short of running: what the native compiler wants.  (Running
/// a program to compile it would do what the program does, to the world.)
pub fn compile(src: &str, opts: &Options) -> Output {
    run_with(src, opts, true, Stage::Lower)
}

/// A package, run (with these arguments for its `main`).
pub fn run_package(package: &PackageSource, opts: &Options, args: Vec<String>) -> Output {
    build(package, Deps::default(), opts, true, Stage::Run(running(opts, args)))
}

/// The same, on `runtime`: Silo, for native code.
pub fn run_package_on(package: &PackageSource, opts: &Options, args: Vec<String>, runtime: meadow::Runtime) -> Output {
    let running = Running { runtime, ..running(opts, args) };
    build(package, Deps::default(), opts, true, Stage::Run(running))
}

/// A package, everything short of running it.
pub fn compile_package(package: &PackageSource, opts: &Options) -> Output {
    build(package, Deps::default(), opts, true, Stage::Lower)
}

/// A package compiled against what it depends on, and run (with these
/// arguments for its `main`): it is linked with them first.
pub fn run_package_with(package: &PackageSource, deps: Deps<'_>, opts: &Options, args: Vec<String>) -> Output {
    build(package, deps, opts, true, Stage::Run(running(opts, args)))
}

/// A package compiled against what it depends on, and linked with them if
/// it is a program, but not run: what a build keeps, and what native code
/// is made from.
pub fn compile_package_with(package: &PackageSource, deps: Deps<'_>, opts: &Options) -> Output {
    build(package, deps, opts, true, Stage::Lower)
}

/// Only check a program: no lowering, no running.  (With no `main` run,
/// either.)
pub fn check(src: &str, opts: &Options) -> Output {
    run_with(src, opts, false, Stage::Check)
}

/// How a program is run: on Glade, with these arguments for it, talking to
/// the real streams or kept (see [`Options::live`]), and, kept, for as long
/// as [`timeout`] allows.
pub fn running(opts: &Options, args: Vec<String>) -> Running {
    let timeout = (!opts.live).then(timeout);
    Running { runtime: meadow::Runtime::Glade, args, live: opts.live, timeout }
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
    linked(packages, opts, Stage::Run(running(opts, args)))
}

/// The same, everything short of running it: what native code is made from.
pub fn link_compiled(packages: &[&CompiledPackage], opts: &Options) -> Output {
    linked(packages, opts, Stage::Lower)
}

fn linked(packages: &[&CompiledPackage], opts: &Options, stage: Stage) -> Output {
    let (root, deps) = packages.split_last().expect("a program has a package of its own");
    // Std, which every package has, first, unless it is among them already
    let std_package = crate::stdlib::compiled();
    let mut with_std: Vec<&CompiledPackage> = Vec::new();
    if !deps.iter().any(|p| p.name == crate::stdlib::NAME) {
        with_std.push(&std_package);
    }
    with_std.extend(deps.iter().copied());
    let deps = &with_std[..];
    // the prelude, as every package has it: its definitions are the
    // outermost of the program, and its tables are everyone's
    let mut out = build(&PackageSource::single("Main", "<link>", ""), Deps::default(), opts, false, Stage::Check);
    let Some(mut checked) = out.checked.take() else { return out };
    checked.compiled = Some((*root).clone());
    typecheck::packages::link(&mut checked, deps);
    finish(checked, out.diagnostics, out.sources, Vec::new(), None, stage)
}

/// How far to take a program.
pub(crate) enum Stage {
    /// check it
    Check,
    /// and lower it, to Cut
    Lower,
    /// and run it, as this says
    Run(Running),
}

fn run_with(src: &str, opts: &Options, entry: bool, stage: Stage) -> Output {
    build(&PackageSource::single("Main", "<input>", src), Deps::default(), opts, entry, stage)
}

fn build(package: &PackageSource, deps: Deps<'_>, opts: &Options, entry: bool, stage: Stage) -> Output {
    with_std(&package.name, deps, |deps| build_with(package, deps, opts, entry, stage))
}

/// `deps`, with the standard library among them, as every package has it,
/// for `f`: unless the package is the standard library, or already depends
/// on it by that name. What the package names its own things with starts
/// after what Std named its with.
pub(crate) fn with_std<R>(package: &str, deps: Deps<'_>, f: impl FnOnce(Deps<'_>) -> R) -> R {
    use crate::stdlib::{self, NAME};
    if package == NAME || deps.direct.iter().any(|(n, _)| *n == NAME) {
        return f(deps);
    }
    let std_package = stdlib::compiled();
    let all: Vec<&CompiledPackage> = std::iter::once(&*std_package).chain(deps.all.iter().copied()).collect();
    let direct: Vec<(&str, &CompiledPackage)> = std::iter::once((NAME, &*std_package)).chain(deps.direct.iter().copied()).collect();
    f(Deps { all: &all, direct: &direct, first_unique: deps.first_unique.max(std_package.uniques.end) })
}

/// The packages a checked program is to be linked with: all of `deps`,
/// but Std only if something uses it -- the program itself, when Std is all
/// there is, or any other package, which may.
pub(crate) fn to_link<'a>(checked: &typecheck::TypeCheckResult, deps: &[&'a CompiledPackage]) -> Vec<&'a CompiledPackage> {
    let others = deps.iter().any(|p| p.name != crate::stdlib::NAME);
    deps.iter()
        .copied()
        .filter(|p| p.name != crate::stdlib::NAME || others || typecheck::packages::uses(checked, p))
        .collect()
}

fn build_with(package: &PackageSource, deps: Deps<'_>, opts: &Options, entry: bool, stage: Stage) -> Output {
    let Front { diagnostics, sources, module, mut checked, decls, mut declarations, parted, .. } = match front(package, deps, opts, entry) {
        Ok(front) => front,
        Err(unread) => return unread,
    };
    let hir = opts.debug.then(|| format!("{module:#?}"));
    declarations.extend(deps.direct.iter().flat_map(|(name, p)| exported(name, p)));

    // a program that uses other packages is put together with them
    let checks = !diagnostics.iter().any(Diagnostic::is_error);
    if !matches!(stage, Stage::Check) && checks && checked.program.is_some() {
        let linking = to_link(&checked, deps.all);
        if !linking.is_empty() || parted {
            typecheck::packages::link(&mut checked, &linking);
        }
    }
    let mut out = finish(checked, diagnostics, sources, decls, hir, stage);
    out.declarations = declarations;
    out
}

/// A package read, renamed and checked: the front of the pipeline.
pub(crate) struct Front {
    pub diagnostics: Vec<Diagnostic>,
    pub sources: SourceMap,
    pub module: idyllic_hir::Module,
    pub spans: idyllic_hir::SpanTable,
    pub checked: typecheck::TypeCheckResult,
    /// the program's own definitions and their types, as written
    pub decls: Vec<(String, String)>,
    /// what it declares, for the finder and the like
    pub declarations: Vec<Declaration>,
    /// whether it was compiled in several parts, which a program is then
    /// linked from even when it depends on nothing
    pub parted: bool,
}

/// Read, rename and check a package; or, if it cannot be read, what there
/// is to say about it.
pub(crate) fn front(package: &PackageSource, deps: Deps<'_>, opts: &Options, entry: bool) -> Result<Front, Output> {
    let _ = opts;
    let mut diagnostics = Vec::new();
    let mut sources = SourceMap::default();
    let programs: Vec<Option<idyllic_ast::Program>> =
        package.modules.iter().map(|m| read(&mut sources, &m.file, &m.text, &mut diagnostics)).collect();
    let programs: Option<Vec<idyllic_ast::Program>> = programs.into_iter().collect();
    let asts = match programs {
        Some(asts) => asts,
        None => {
            return Err(Output {
                sources,
                diagnostics,
                body_type: None,
                decls: Vec::new(),
                hir: None,
                cut: None,
                not_lowered: Vec::new(),
                evaluation: None,
                printed: String::new(),
                errors: String::new(),
                ran_main: false,
                checked: None,
                declarations: Vec::new(),
            });
        }
    };

    // name resolution -> HIR, then SCC / retopologize, then type check
    let Some(prelude) = read(&mut sources, "<prelude>", PRELUDE, &mut diagnostics) else {
        panic!("the prelude does not parse: {diagnostics:#?}")
    };
    let paths: Vec<Vec<String>> = package.modules.iter().map(|m| m.path.clone()).collect();
    let texts: Vec<&str> = package.modules.iter().map(|m| m.text.as_str()).collect();
    let parts = crate::parts::parts(&package.name, &paths, &texts);
    // (a module written inside a file whose path another module has is
    // the renamer's to report, which it can only with both in one part)
    if parts.len() > 1 && !crate::parts::clash(&paths, &asts) {
        return Ok(parted(package, deps, &asts, &prelude, &parts, entry, diagnostics, sources));
    }
    let modules: Vec<rename::ModuleSource<'_>> = package
        .modules
        .iter()
        .zip(&asts)
        .map(|(m, program)| rename::ModuleSource { path: m.path.clone(), program })
        .collect();
    let direct: Vec<(&str, &idyllic_hir::PackageNames)> = deps.direct.iter().map(|(n, p)| (*n, &p.names)).collect();
    let first_unique = deps.first_unique.max(idyllic_builtins::FIRST_USER_UNIQUE);
    let mut renamed = rename::rename_package(&package.name, &modules, &prelude, &direct, first_unique, package.linear);
    renamed.module.version = package.version.clone();
    diagnostics.extend(
        renamed
            .errors
            .iter()
            .map(rename::RenameError::to_diagnostic),
    );
    let module = scc::scc_module(renamed.module);
    let checked = typecheck::check_module(&module, renamed.spans.len(), entry, deps.all);
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
    let declarations = declarations(&checked, &renamed.spans, &sources);
    Ok(Front { diagnostics, sources, module, spans: renamed.spans, checked, decls, declarations, parted: false })
}

/// Where each part of a package starts naming its things: on a boundary of
/// this many uniques after the part before it, so that a part that grows
/// moves the parts after it only once it passes one.
const PART_CHUNK: usize = 1 << 12;

/// A package of several parts (see [`crate::parts`]), compiled a part at a
/// time, each against the packages it depends on and its own parts that it
/// imports: the root's part checked last, its result the package's, with
/// the parts made one compiled package. A part that does not check stops
/// the parts that import it, and its result is the package's.
#[allow(clippy::too_many_arguments)]
fn parted(
    package: &PackageSource,
    deps: Deps<'_>,
    asts: &[idyllic_ast::Program],
    prelude: &idyllic_ast::Program,
    parts: &[crate::parts::Part],
    entry: bool,
    mut diagnostics: Vec<Diagnostic>,
    sources: SourceMap,
) -> Front {
    let programs: Vec<&idyllic_ast::Program> = asts.iter().collect();
    let visibility_mentioned = rename::package_mentions_visibility(&programs);
    let direct: Vec<(&str, &idyllic_hir::PackageNames)> = deps.direct.iter().map(|(n, p)| (*n, &p.names)).collect();
    let mut compiled: Vec<Option<CompiledPackage>> = Vec::new();
    let mut declared = Vec::new();
    let mut decls: Vec<(usize, String, String)> = Vec::new();
    let mut failed = None;
    let mut next = deps.first_unique.max(idyllic_builtins::FIRST_USER_UNIQUE);
    let mut last = None;
    for part in parts {
        // a part that imports one that did not check is not checked
        let imported: Option<Vec<&CompiledPackage>> = part.imports.iter().map(|&j| compiled[j].as_ref()).collect();
        let Some(imported) = imported else {
            compiled.push(None);
            continue;
        };
        let modules: Vec<rename::ModuleSource<'_>> =
            part.modules.iter().map(|&i| rename::ModuleSource { path: package.modules[i].path.clone(), program: &asts[i] }).collect();
        let siblings: Vec<&idyllic_hir::PackageNames> = imported.iter().map(|p| &p.names).collect();
        let at = rename::PartOf { siblings: &siblings, root: part.root, visibility_mentioned };
        let mut renamed = rename::rename_part(&package.name, &modules, prelude, &direct, next, package.linear, at);
        renamed.module.version = package.version.clone();
        let mut said: Vec<Diagnostic> = renamed.errors.iter().map(rename::RenameError::to_diagnostic).collect();
        let module = scc::scc_module(renamed.module);
        let all: Vec<&CompiledPackage> = deps.all.iter().copied().chain(imported.iter().copied()).collect();
        let checked = typecheck::check_module(&module, renamed.spans.len(), entry && part.root, &all);
        said.extend(checked.errors.iter().map(|e| e.to_diagnostic(&renamed.spans)));
        next = module.uniques.1.div_ceil(PART_CHUNK) * PART_CHUNK;
        declared.extend(declarations(&checked, &renamed.spans, &sources));
        // (where in the sources each is: the order they were written, all
        // parts together, below)
        decls.extend(checked.decl_types.iter().map(|(id, x, ty)| (renamed.spans.get(*id).map_or(usize::MAX, |s| s.start), x.clone(), ty.to_string())));
        let wrong = said.iter().any(Diagnostic::is_error);
        diagnostics.extend(said);
        compiled.push(if wrong { None } else { checked.compiled.clone() });
        if wrong {
            if failed.is_none() {
                failed = Some((module, renamed.spans, checked));
            }
        } else if part.root {
            last = Some((module, renamed.spans, checked));
        }
    }
    let ((module, spans, mut checked), whole) = match (failed, last) {
        (Some(f), _) => (f, false),
        (None, Some(root)) => (root, true),
        (None, None) => unreachable!("the root's part is checked once every part before it is"),
    };
    if whole {
        // the package, whole: what is stored, imported and linked
        let parts = compiled.into_iter().map(|c| c.expect("every part checked")).collect();
        checked.compiled = Some(typecheck::packages::merge(parts));
    }
    decls.sort_by_key(|(at, ..)| *at);
    let decls = decls.into_iter().map(|(_, x, ty)| (x, ty)).collect();
    Front { diagnostics, sources, module, spans, checked, decls, declarations: declared, parted: true }
}

/// What [`Output::declarations`] lists, from the checker's catalog.
fn declarations(checked: &typecheck::TypeCheckResult, spans: &idyllic_hir::SpanTable, sources: &SourceMap) -> Vec<Declaration> {
    checked
        .catalog
        .iter()
        .filter_map(|d| {
            let span = spans.get(d.node)?;
            let (file, offset) = sources.locate(span.start)?;
            let file = sources.file(file);
            let (line, column) = LineIndex::new(&file.text).line_col(offset);
            // a definition's signature and equations are all its own: it is
            // written where the first of them is, under its comment
            let last = d.name.rsplit('.').next().unwrap_or(&d.name);
            let line = own_lines_from(&file.text, line, last);
            Some(Declaration {
                name: d.name.clone(),
                kind: d.kind,
                ty: d.ty.clone(),
                shown: d.ty.to_string(),
                prelude: d.prelude,
                span,
                file: file.name.clone(),
                line: line + 1,
                column: column + 1,
                doc: doc_above(&file.text, line),
                package: None,
            })
        })
        .collect()
}

/// The first of the lines, ending at line `line` (zero-based), that a
/// definition of `name` is written on: its signature and equations each
/// start with it.
fn own_lines_from(text: &str, line: usize, name: &str) -> usize {
    let lines: Vec<&str> = text.lines().collect();
    let own = |l: &str| {
        let rest = l.strip_prefix(name).or_else(|| l.strip_prefix(&format!("({name})")));
        rest.is_some_and(|r| r.starts_with([' ', ':']))
    };
    let mut first = line.min(lines.len());
    while first > 0 && own(lines[first - 1]) {
        first -= 1;
    }
    first
}

/// What a package depended on gives its dependents, as declarations: each
/// public name, written in full (`Std.Collections.List.map`), with its type.
fn exported(package: &str, p: &CompiledPackage) -> Vec<Declaration> {
    use typecheck::DeclKind;
    let types: std::collections::HashMap<usize, &std::rc::Rc<typecheck::core::Core>> =
        p.constants.iter().map(|c| (c.unique, &c.ty)).collect();
    let datas: std::collections::HashSet<usize> = p.datas.iter().map(|(u, _)| *u).collect();
    let ctors: std::collections::HashSet<usize> = p.ctors.iter().map(|(u, _)| *u).collect();
    let effects: std::collections::HashSet<usize> = p.effects.iter().map(|(u, _)| *u).collect();
    let ops: std::collections::HashSet<usize> = p.ops.iter().map(|(u, _)| *u).collect();
    // each public name, with what a dependent writes before it: its
    // module's path, and for a constructor its type's
    let mut public: Vec<(Vec<String>, String, usize)> = Vec::new();
    for (path, name, u, vis) in &p.names.exports {
        if *vis != idyllic_ast::Visibility::Public {
            continue;
        }
        public.push((path.clone(), name.clone(), *u));
        if let Some((_, _, members)) = p.names.types.iter().find(|(t, ..)| t == u) {
            let under: Vec<String> = path.iter().cloned().chain(std::iter::once(name.clone())).collect();
            public.extend(members.iter().map(|(c, cu)| (under.clone(), c.clone(), *cu)));
        }
    }
    let mut seen = std::collections::HashSet::new();
    public.retain(|(path, name, u)| seen.insert((path.clone(), name.clone(), *u)));
    let mut out = Vec::new();
    for (path, name, u) in &public {
        let Some(ty) = types.get(u) else { continue };
        let kind = if datas.contains(u) {
            DeclKind::Type
        } else if ctors.contains(u) {
            DeclKind::Constructor
        } else if effects.contains(u) {
            DeclKind::Effect
        } else if ops.contains(u) {
            DeclKind::Operation
        } else {
            DeclKind::Value
        };
        let full = std::iter::once(package.to_string()).chain(path.iter().cloned()).chain(std::iter::once(name.clone()));
        out.push(Declaration {
            name: full.collect::<Vec<_>>().join("."),
            kind,
            ty: (*ty).clone(),
            shown: ty.to_string(),
            prelude: false,
            span: Span::new(0, 0),
            file: format!("<{package}>"),
            line: 0,
            column: 0,
            doc: None,
            package: Some(package.to_string()),
        });
    }
    out
}

/// The `--` comment on the lines just above line `line` (zero-based) of
/// `text`, without its dashes.
fn doc_above(text: &str, line: usize) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut doc: Vec<&str> = lines[..line.min(lines.len())]
        .iter()
        .rev()
        .map(|l| l.trim())
        .take_while(|l| l.starts_with("--"))
        .map(|l| {
            let text = l.trim_start_matches('-');
            text.strip_prefix(' ').unwrap_or(text)
        })
        .collect();
    doc.reverse();
    (!doc.is_empty()).then(|| doc.join("\n"))
}

/// The rest of the pipeline, for a program checked (and linked).
pub(crate) fn finish(
    mut checked: typecheck::TypeCheckResult,
    diagnostics: Vec<Diagnostic>,
    sources: SourceMap,
    decls: Vec<(String, String)>,
    hir: Option<String>,
    stage: Stage,
) -> Output {
    let ran_main = checked.ran_main;
    // only a program that checks is lowered and run, and only one with a
    // program: what only declares has nothing to run
    let runs = !diagnostics.iter().any(Diagnostic::is_error) && checked.program.is_some();
    let ran = match stage {
        _ if !runs => None,
        Stage::Check => None,
        Stage::Lower => Some(evaluate(&mut checked, None)),
        Stage::Run(running) => Some(evaluate(&mut checked, Some(&running))),
    };
    let (cut, not_lowered, evaluation, printed, errors) = match ran {
        Some(r) => (r.cut, r.not_lowered, r.evaluation, r.printed, r.errors),
        None => (None, Vec::new(), None, String::new(), String::new()),
    };

    Output {
        diagnostics,
        cut,
        not_lowered,
        evaluation,
        printed,
        errors,
        ran_main,
        body_type: checked.body_type.as_ref().map(|ty| idyllic_typecheck::number_metas(&mut Default::default(), &ty.to_string())),
        decls,
        hir,
        checked: Some(checked),
        sources,
        declarations: Vec::new(),
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

/// How far [`Session::enter_in`] takes an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// check it, and no more; nor does it extend the session
    Check,
    /// and lower it, for whoever runs it some other way
    Lower,
    /// and run it on Glade
    Run,
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
        self.enter_in(entry, opts, Mode::Run)
    }

    /// The same, taken as far as `mode` says.
    pub fn enter_in(&mut self, entry: &str, opts: &Options, mode: Mode) -> Entered {
        let source = format!("{}{}\n", self.source, entry.trim_end());
        let stage = match mode {
            Mode::Check => Stage::Check,
            Mode::Lower => Stage::Lower,
            // stopped if it runs on: an entry that does not end must give
            // the prompt back, though it prints as it goes
            Mode::Run => Stage::Run(Running { timeout: Some(timeout()), ..running(opts, Vec::new()) }),
        };
        // defining `main` is not running it: that is `main ()`
        let mut output = run_with(&source, opts, false, stage);
        let earlier = self.source.len();
        output
            .diagnostics
            .retain(|d| d.labels.is_empty() || d.labels.iter().any(|l| l.span.start >= earlier));
        let defined = output.decls.len();
        output.decls.drain(..self.seen.min(defined));
        // (an entry of declarations is kept even if it defines nothing new:
        // `use Nat.*` is one)
        if output.body_type.is_none() && !output.has_errors() && mode != Mode::Check {
            self.seen = defined;
            self.source = source.clone();
        }
        Entered { output, source }
    }

    /// The declarations entered so far, as one source text.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Forget every declaration.
    pub fn reset(&mut self) {
        *self = Session::default();
    }
}
