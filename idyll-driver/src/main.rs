//! The `idyll` command-line entry point: the build system (in this crate's
//! library, `lib.rs`) and the REPL. This file only parses arguments, wires
//! things together, and prints results.

mod repl;
mod tour;

use clap::{Parser, Subcommand};
use idyll::{
    Backend, Profile, Resolved, artifacts, init, native,
    package::ProfileConfig,
    pipeline::{self, BuildOutput},
    status,
    workspace::{Selected, Selection},
};
use idyllic::pipeline::{Evaluation, Options};
use idyllic_diagnostics::OutputStyle;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The Idyll tool: builds and runs packages. With no command, starts the
/// REPL.
#[derive(Parser)]
#[command(name = "idyll", version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    /// Never fetch a dependency. What is already in the cache is used; anything
    /// else is an error saying what would have had to be fetched.
    #[arg(long, global = true)]
    offline: bool,
    /// Refuse anything that would change `idyll.lock`. What CI wants: a build
    /// that quietly re-pins a dependency is a build of something nobody
    /// reviewed.
    #[arg(long, global = true)]
    locked: bool,
    /// Never colour output (the NO_COLOR environment variable also works).
    #[arg(long, global = true)]
    no_color: bool,
    /// Draw diagnostics with ASCII instead of box-drawing characters.
    #[arg(long, global = true)]
    ascii: bool,
    /// Print the Cut each program is lowered to before its result: what
    /// Meadow's runtimes are given.
    #[arg(long, global = true)]
    cut: bool,
    /// In the REPL: dump the HIR (after renaming and the SCC pass) for each
    /// entry.
    #[arg(long)]
    debug: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check a package and everything it depends on; for a native build,
    /// compile it into an executable.
    Build {
        /// Package directory (or a single `.idl` file), or a workspace.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// What to write, in place of what a build writes by default -- an
        /// executable, for a release build: `cut` (the program as Cut, which
        /// Meadow's runtimes run) or `exe`. Comma-separated, or repeated.
        #[arg(long, value_name = "KIND", value_delimiter = ',', value_parser = ["cut", "exe"])]
        emit: Vec<String>,
        #[command(flatten)]
        packages: PackageArgs,
        #[command(flatten)]
        profile: ProfileArgs,
    },
    /// Build a package, then run it: its `main`, or its expression.
    Run {
        /// Package directory (or a single `.idl` file), or a workspace.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// In a workspace: the member to run.
        #[arg(short = 'p', long = "package", value_name = "NAME")]
        package: Option<String>,
        #[command(flatten)]
        profile: ProfileArgs,
        /// Arguments for the program, after `--`: what `args ()` gives it.
        /// `idyll run -- in.txt -o out` passes `in.txt -o out`.
        #[arg(last = true, value_name = "ARGS")]
        args: Vec<String>,
    },
    /// Create a package: an `Idyll.toml` and a `src/Main.idl` that runs.
    ///
    /// Inside a workspace, the new package is added to its `members`.
    Init {
        /// Where to put it, created if it does not exist.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// The package's name. Defaults to the directory's.
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
        /// Create a workspace instead: an `Idyll.toml` with an empty
        /// `[workspace]`, for packages made inside it to join.
        #[arg(long, conflicts_with = "name")]
        workspace: bool,
    },
    /// Add a dependency to a package's manifest.
    ///
    /// Takes a git URL, a GitHub `owner/name`, or a directory. There is no
    /// registry to look a name up in, so the repository is fetched and its own
    /// `Idyll.toml` says what the package is called.
    Add {
        /// `https://github.com/owner/name`, `owner/name`, or `../a/directory`.
        #[arg(value_name = "WHAT")]
        what: String,
        /// Take this branch. Without one, the repository's default branch.
        #[arg(long, value_name = "NAME", conflicts_with_all = ["tag", "rev"])]
        branch: Option<String>,
        /// Take this tag.
        #[arg(long, value_name = "NAME", conflicts_with_all = ["branch", "rev"])]
        tag: Option<String>,
        /// Take this commit, which cannot come to mean anything else.
        #[arg(long, value_name = "COMMIT", conflicts_with_all = ["branch", "tag"])]
        rev: Option<String>,
        /// Call it this, rather than what it calls itself.
        #[arg(long, value_name = "NAME")]
        rename: Option<String>,
        /// The package to add it to.
        #[arg(long, default_value = ".", value_name = "DIR")]
        path: PathBuf,
    },
    /// Remove what a build wrote: the package's `target` directory, with the
    /// executables and incremental cache in it.
    ///
    /// Sources, `Idyll.toml` and `idyll.lock` are not touched, and neither is
    /// anything fetched into the dependency cache, which other packages share.
    Clean {
        /// The package, or a workspace, whose `target` to remove.
        #[arg(default_value = ".", value_name = "PATH")]
        path: PathBuf,
        /// Only this profile's, rather than all of them.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// Say what would be removed without removing it.
        #[arg(long)]
        dry_run: bool,
    },
    /// Bring a package's dependencies forward to what their branches and tags
    /// now name, rewriting `idyll.lock`.
    ///
    /// The manifest is not touched: a dependency keeps the branch or tag it
    /// follows.
    Update {
        /// Only these, by name. Without any, every git dependency.
        #[arg(value_name = "NAME")]
        only: Vec<String>,
        /// The package, or a member of the workspace holding the lockfile.
        #[arg(long, default_value = ".", value_name = "DIR")]
        path: PathBuf,
        /// Say what would change without changing it.
        #[arg(long)]
        dry_run: bool,
    },
}

/// `-p`, `--workspace` and `--exclude` -- which members of a workspace a
/// command means.
#[derive(clap::Args)]
struct PackageArgs {
    /// In a workspace: the member to use, by name. Repeat it for more.
    #[arg(short = 'p', long = "package", value_name = "NAME")]
    package: Vec<String>,
    /// Every member of the workspace.
    #[arg(long, visible_alias = "all", conflicts_with = "package")]
    workspace: bool,
    /// With `--workspace`: leave this member out. Repeat it for more.
    #[arg(long, value_name = "NAME", requires = "workspace")]
    exclude: Vec<String>,
}

impl PackageArgs {
    fn selection(&self) -> Selection {
        Selection { packages: self.package.clone(), workspace: self.workspace, exclude: self.exclude.clone() }
    }
}

/// What `selection` means at `path`, exiting with the reason when it means
/// nothing -- after warning about any member profile the workspace ignores.
fn select(selection: &Selection, path: &Path) -> Selected {
    let selected = exit_on_error(selection.select(path));
    if let Some(ws) = &selected.workspace {
        for m in ws.ignored_profiles(&selected.paths) {
            status::warning(format!(
                "the `[profile]` sections of `{}` are ignored: a workspace member \
                 builds with the profiles in {}",
                m.name,
                idyll::workspace::shown(&ws.root.join("Idyll.toml"))
            ));
        }
    }
    selected
}

/// `--release` / `--debug` — the build profile — plus the backend, which
/// overrides whatever the profile chose.
///
/// Debug is the default: the program runs on Glade, Meadow's bytecode.
/// Release builds it into native code with Silo. Either can be overridden,
/// here or in the package's `Idyll.toml`.
#[derive(clap::Args)]
struct ProfileArgs {
    /// Build with the release profile: native code.
    #[arg(long, conflicts_with = "debug")]
    release: bool,
    /// Build with the debug profile (the default).
    #[arg(long)]
    debug: bool,
    /// Which of Meadow's runtimes runs the program: `glade` (its bytecode)
    /// or `silo` (native code, an executable built first). Debug builds
    /// default to `glade` and release builds to `silo`; `backend = "..."` in
    /// a `[profile.<name>]` of `Idyll.toml` overrides that, and this
    /// overrides both.
    #[arg(long, value_name = "BACKEND", value_parser = ["glade", "silo"], conflicts_with_all = ["silo", "glade"])]
    backend: Option<String>,
    /// `--backend silo`.
    #[arg(long, conflicts_with = "glade")]
    silo: bool,
    /// `--backend glade`.
    #[arg(long)]
    glade: bool,
}

impl ProfileArgs {
    fn profile(&self) -> Profile {
        if self.release { Profile::Release } else { Profile::Debug }
    }

    /// The switches named on the command line, which win over everything.
    fn overrides(&self) -> ProfileConfig {
        ProfileConfig {
            backend: self
                .backend
                .as_deref()
                .and_then(Backend::parse)
                .or(self.silo.then_some(Backend::Silo))
                .or(self.glade.then_some(Backend::Glade)),
        }
    }

    /// The profile, the package's `Idyll.toml`, and these flags, in that order
    /// of increasing authority.
    fn resolve(&self, path: &Path) -> Resolved {
        Resolved::resolve(self.profile(), path, self.overrides())
    }
}

/// The stack the whole command runs on: the compiler walks a program's
/// structure recursively, and a main thread's stack is 1 MB on Windows.
const STACK: usize = 256 << 20;

fn main() -> ExitCode {
    let run = std::thread::Builder::new().name("idyll".into()).stack_size(STACK).spawn(command).expect("the main thread starts");
    // A panic has said what it was already; the exit status is Rust's own.
    run.join().unwrap_or(ExitCode::from(101))
}

fn command() -> ExitCode {
    let cli = Cli::parse();
    let mut style = OutputStyle::detect();
    style.color &= !cli.no_color;
    style.unicode &= !cli.ascii;
    if cli.no_color {
        // (what the status lines go by)
        // Safety: before any other thread is started.
        unsafe { std::env::set_var("NO_COLOR", "1") };
    }
    let opts = Options { debug: cli.debug, style, live: true };
    // Said once, before anything resolves a dependency.
    idyll::package::set_policy(idyll::package::Policy {
        net: if cli.offline { idyll::git::Net::Offline } else { idyll::git::Net::Allowed },
        locked: cli.locked,
        update: false,
    });
    // Say what is being done -- `Compiling`, `Finished` -- except in the
    // REPL, whose terminal is not ours to write to.
    if cli.cmd.is_some() {
        status::enable();
    }
    match cli.cmd {
        // No subcommand → interactive REPL.
        None => repl::run(&opts, cli.cut),
        Some(Cmd::Build { path, emit, packages, profile }) => {
            let selected = select(&packages.selection(), &path);
            let profile = profile.resolve(&path);
            match &selected.paths[..] {
                [one] => build(one, &emit, profile, &opts),
                many => build_many(many, &emit, profile, &opts),
            }
        }
        Some(Cmd::Run { path, package, profile, args }) => {
            let resolved = profile.resolve(&path);
            let selection = Selection { packages: package.into_iter().collect(), ..Selection::default() };
            let selected = select(&selection, &path);
            let one = exit_on_error(selected.one("run"));
            run(one, resolved, &opts, cli.cut, &args)
        }
        Some(Cmd::Add { what, branch, tag, rev, rename, path }) => {
            let reference = match (branch, tag, rev) {
                (Some(b), _, _) => idyll::package::GitRef::Branch(b),
                (_, Some(t), _) => idyll::package::GitRef::Tag(t),
                (_, _, Some(r)) => idyll::package::GitRef::Rev(r),
                _ => idyll::package::GitRef::Default,
            };
            match idyll::add::run(&idyll::add::Options { what, reference, rename, dir: path }) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    status::error(e);
                    ExitCode::FAILURE
                }
            }
        }
        Some(Cmd::Init { path, name, workspace }) => match init::run(&init::Options { path, name, workspace }) {
            Ok(made) => {
                if workspace {
                    println!("created a workspace at {}", made.root.display());
                } else {
                    println!("created package `{}` at {}", made.name, made.root.display());
                }
                for f in &made.files {
                    println!("  {}", nearby(f).display());
                }
                if let Some(ws) = &made.joined {
                    println!("added `{}` to the members of {}", made.name, nearby(ws).display());
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                status::error(e);
                ExitCode::FAILURE
            }
        },
        Some(Cmd::Clean { path, profile, dry_run }) => match idyll::clean::run(&path, profile.as_deref(), dry_run) {
            Ok(out) if out.removed.is_empty() => {
                status::status("Clean", "nothing to remove");
                ExitCode::SUCCESS
            }
            Ok(out) => {
                let what = if dry_run { "Would remove" } else { "Removed" };
                for dir in &out.removed {
                    status::status(what, format!("{} ({})", dir.display(), idyll::clean::bytes(out.bytes)));
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                status::error(e);
                ExitCode::FAILURE
            }
        },
        Some(Cmd::Update { only, path, dry_run }) => {
            match idyll::update::run(&idyll::update::Options { dir: path, only, dry_run }) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    status::error(e);
                    ExitCode::FAILURE
                }
            }
        }
    }
}

/// `result`'s value, or its error said and the process ended.
fn exit_on_error<T>(result: Result<T, String>) -> T {
    result.unwrap_or_else(|e| {
        status::error(e);
        std::process::exit(1);
    })
}

/// `path` as seen from the current directory, when it is inside it: `init`
/// finds a workspace by its full path, and says so more briefly.
fn nearby(path: &Path) -> PathBuf {
    let here = std::env::current_dir().ok().and_then(|d| std::fs::canonicalize(d).ok());
    let full = std::fs::canonicalize(path).ok();
    match (here, full) {
        (Some(here), Some(full)) => match full.strip_prefix(&here) {
            Ok(rest) if rest.as_os_str().is_empty() => PathBuf::from("."),
            Ok(rest) => rest.to_path_buf(),
            Err(_) => path.to_path_buf(),
        },
        _ => path.to_path_buf(),
    }
}

/// Say what compiling said.
fn report(problem: &Option<idyll::package::Diagnostic>, reported: &[pipeline::Reported]) {
    if let Some(p) = problem {
        status::problem(p);
    }
    for r in reported {
        status::diagnostics(&r.diagnostics, &r.sources);
    }
}

/// The package a build was of, by name.
fn name_of(out: &BuildOutput, path: &Path) -> String {
    out.package.as_ref().map_or_else(|| idyll::workspace::shown(path), |(_, n)| n.clone())
}

/// `    Finished `debug` profile [machine] in 0.42s`
fn finished(profile: &Resolved, started: std::time::Instant) {
    status::status(
        "Finished",
        format!("`{}` profile [{}] in {}", profile.profile.name(), profile.how(), status::elapsed(started.elapsed())),
    );
}

/// `error: could not compile `app` due to 2 previous errors`
fn could_not_compile(name: &str, errors: usize) -> ExitCode {
    status::error(format!("could not compile `{name}` due to {errors} previous error{}", if errors == 1 { "" } else { "s" }));
    ExitCode::FAILURE
}

/// Discover and compile the package at `path`, and write what its profile
/// (or `emit`) asks for.
fn build(path: &Path, emit: &[String], profile: Resolved, opts: &Options) -> ExitCode {
    let started = std::time::Instant::now();
    let out = pipeline::build(path, profile.profile, opts);
    report(&out.problem, &out.reported);
    let name = name_of(&out, path);
    if out.errors() > 0 {
        return could_not_compile(&name, out.errors());
    }
    match write_artifacts(&out, &name, emit, profile, opts) {
        Ok(profile) => {
            finished(&profile, started);
            ExitCode::SUCCESS
        }
        Err(e) => {
            status::error(e);
            ExitCode::FAILURE
        }
    }
}

/// [`build`] for several packages of a workspace, sharing what they have in
/// common. Nothing runs: `run` takes one package.
fn build_many(paths: &[PathBuf], emit: &[String], profile: Resolved, opts: &Options) -> ExitCode {
    let started = std::time::Instant::now();
    let paths: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
    let out = pipeline::build_each(&paths, profile.profile, opts);
    report(&out.problem, &out.reported);
    if out.errors() > 0 {
        return could_not_compile("the workspace", out.errors());
    }
    let mut used = profile;
    for (built, path) in out.each.iter().zip(&paths) {
        match write_artifacts(built, &name_of(built, path), emit, profile, opts) {
            Ok(p) => used = p,
            Err(e) => {
                status::error(e);
                return ExitCode::FAILURE;
            }
        }
    }
    finished(&used, started);
    ExitCode::SUCCESS
}

/// Write what a build of a program makes: an executable, for a Silo build
/// -- or what `emit` asks for instead. A package with nothing to run is
/// only checked. Answers the profile as it turned out: a Silo build nobody
/// asked for gives way to Glade when it cannot be had.
fn write_artifacts(out: &BuildOutput, name: &str, emit: &[String], profile: Resolved, opts: &Options) -> Result<Resolved, String> {
    if !out.runnable() {
        if !emit.is_empty() {
            status::warning(format!("`{name}` has nothing to run, so nothing to emit: give it a `main`, or an expression"));
        }
        return Ok(profile);
    }
    let wants = |kind: &str| emit.iter().any(|e| e == kind);
    let exe = wants("exe") || (emit.is_empty() && profile.backend == Backend::Silo);
    if emit.is_empty() && !exe {
        return Ok(profile);
    }
    let Some((root, _)) = &out.package else {
        let why = "a file of its own has no `target` to write to: make it a package with `idyll init`";
        return match profile.fallback() {
            Some(glade) if emit.is_empty() => {
                status::warning(format!("`{name}` was not built into native code: {why}; it runs on Glade"));
                Ok(glade)
            }
            _ => Err(format!("`{name}`: {why}")),
        };
    };
    let linked = idyllic::pipeline::link_compiled(&out.packages(), opts);
    if !linked.not_lowered.is_empty() {
        return Err(format!("`{name}` could not be lowered to Cut -- this is a bug in idyll: {}", linked.not_lowered.join("; ")));
    }
    if wants("cut") {
        let program = native::cut(&linked).map_err(|e| e.to_string())?;
        let file = artifacts::cut_path(root, profile.profile, name);
        artifacts::write_text(&file, program)?;
        status::status("Emitted", idyll::workspace::shown(&file));
    }
    if !exe {
        return Ok(profile);
    }
    match executable(out, &linked, root, name, profile) {
        Ok(_) => Ok(profile),
        Err(native::Failure::Unavailable(why)) if !wants("exe") && profile.fallback().is_some() => {
            status::warning(format!("`{name}` was not built into native code: {why}; it runs on Glade"));
            Ok(profile.fallback().expect("checked"))
        }
        Err(e) => Err(e.to_string()),
    }
}

/// The executable of a linked program, compiled unless the one there was
/// made from this very package.
fn executable(
    out: &BuildOutput,
    linked: &idyllic::pipeline::Output,
    root: &Path,
    name: &str,
    profile: Resolved,
) -> Result<PathBuf, native::Failure> {
    let exe = artifacts::exe_path(root, profile.profile, name);
    if out.fingerprint.is_some_and(|f| native::fresh(&exe, f)) {
        return Ok(exe);
    }
    let cut = native::cut(linked)?;
    status::status("Building", format!("{name} with Silo ({})", idyll::workspace::shown(&exe)));
    native::compile(cut, &exe, out.fingerprint)?;
    Ok(exe)
}

/// Build the package at `path` and run it.
fn run(path: &Path, profile: Resolved, opts: &Options, cut: bool, args: &[String]) -> ExitCode {
    let started = std::time::Instant::now();
    let out = pipeline::build(path, profile.profile, opts);
    report(&out.problem, &out.reported);
    let name = name_of(&out, path);
    if out.errors() > 0 {
        return could_not_compile(&name, out.errors());
    }
    if !out.runnable() {
        status::error(format!("`{name}` has nothing to run: give it a `main`, or an expression"));
        return ExitCode::FAILURE;
    }
    let mut profile = profile;
    if profile.backend == Backend::Silo {
        let made = match &out.package {
            Some((root, _)) => {
                let linked = idyllic::pipeline::link_compiled(&out.packages(), opts);
                executable(&out, &linked, root, &name, profile)
            }
            None => Err(native::Failure::Unavailable("a file of its own has no `target` to put an executable in".into())),
        };
        match made {
            Ok(exe) => {
                finished(&profile, started);
                status::status("Running", format!("`{}`", idyll::workspace::shown(&exe)));
                return match native::run(&exe, args) {
                    Ok(code) => ExitCode::from(code as u8),
                    Err(e) => {
                        status::error(e);
                        ExitCode::FAILURE
                    }
                };
            }
            Err(native::Failure::Unavailable(why)) if profile.fallback().is_some() => {
                status::warning(format!("`{name}` was not built into native code: {why}; running it on Glade"));
                profile = profile.fallback().expect("checked");
            }
            Err(e) => {
                status::error(e.to_string());
                return ExitCode::FAILURE;
            }
        }
    }
    finished(&profile, started);
    status::status("Running", format!("`{name}`"));
    let ran = idyllic::pipeline::run_compiled(&out.packages(), opts, args.to_vec());
    if let Some(report) = ran.cut_report().filter(|_| cut) {
        print!("{report}");
    }
    let shown = ran.display("", "", opts.style);
    if !shown.is_empty() {
        println!("{shown}");
    }
    match &ran.evaluation {
        _ if ran.has_errors() => ExitCode::FAILURE,
        // what the program asked for
        Some(Evaluation::Exited(code)) => ExitCode::from(*code as u8),
        Some(Evaluation::Stopped(_) | Evaluation::Failed(_)) => ExitCode::FAILURE,
        _ => ExitCode::SUCCESS,
    }
}
