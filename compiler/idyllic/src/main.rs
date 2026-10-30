use clap::Parser;
use idyllic::pipeline::{self, Options};
use idyllic_diagnostics::OutputStyle;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The Idyll compiler: checks a program, lowers it to Cut, and runs that on
/// Meadow's runtimes (`meadow cut`): Glade, or Silo for native code.  The
/// REPL is the `idyll` command, in idyll-driver/.
#[derive(Parser, Debug)]
#[command(name = "idyllic", version, about)]
struct Cli {
    /// The program: a source file, or a package's directory (with `src/`,
    /// and optionally an `Idyll.toml`).
    file: PathBuf,

    /// Dump the HIR (after renaming and the SCC pass).
    #[arg(long)]
    debug: bool,

    /// Never colour output (the NO_COLOR environment variable also works).
    #[arg(long)]
    no_color: bool,

    /// Draw diagnostics with ASCII instead of box-drawing characters.
    #[arg(long)]
    ascii: bool,

    /// Print the Cut the program is lowered to, before its result.
    #[arg(long)]
    cut: bool,

    /// Run it as native code, built by Silo, rather than on Glade.
    #[arg(long)]
    release: bool,

    /// Build the program into a native executable, with Silo, and write it
    /// here, rather than run it.
    #[arg(short, long, value_name = "EXE")]
    output: Option<PathBuf>,

    /// Make every unannotated binder linear, `[1]`, as if each module said
    /// `@linear`; otherwise it is unrestricted, `[]`.
    #[arg(long)]
    linear: bool,

    /// Arguments for the program, after `--`: what its `args ()` gives it.
    #[arg(last = true)]
    args: Vec<String>,
}

/// The stack the whole command runs on, as the `idyll` command's: the
/// compiler walks a program's structure recursively, and a main thread's
/// stack is 8 MB at most (1 MB on Windows).
const STACK: usize = 256 << 20;

fn main() -> ExitCode {
    let run = std::thread::Builder::new().name("idyllic".into()).stack_size(STACK).spawn(command).expect("the main thread starts");
    // A panic has said what it was already; the exit status is Rust's own.
    run.join().unwrap_or(ExitCode::from(101))
}

fn command() -> ExitCode {
    let cli = Cli::parse();
    let mut style = OutputStyle::detect();
    style.color &= !cli.no_color;
    style.unicode &= !cli.ascii;
    let opts = Options { debug: cli.debug, style, live: true };

    if cli.output.is_some() {
        compile_file(&cli.file, &opts, &cli)
    } else {
        check_file(&cli.file, &opts, &cli)
    }
}

/// The package at `path`: a directory's, or one file's, which is a package
/// of one module named after it.
fn load(path: &Path, linear: bool) -> Result<pipeline::PackageSource, String> {
    let mut package = if path.is_dir() {
        pipeline::load_package(path)?
    } else {
        let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let name = path.file_stem().map_or("Main".into(), |s| s.to_string_lossy().into_owned());
        pipeline::PackageSource::single(&name, &path.display().to_string(), &text)
    };
    package.linear = linear;
    Ok(package)
}

fn compile_file(path: &Path, opts: &Options, cli: &Cli) -> ExitCode {
    let linear = cli.linear;
    let package = match load(path, linear) {
        Ok(package) => package,
        Err(why) => {
            eprintln!("error: {why}");
            return ExitCode::FAILURE;
        }
    };
    let out = pipeline::compile_package(&package, opts);
    eprint!("{}", idyllic_diagnostics::render_map(&out.diagnostics, &out.sources, opts.style));
    let fail = |why: String| {
        eprintln!("error: {why}");
        ExitCode::FAILURE
    };
    if out.has_errors() {
        return ExitCode::FAILURE;
    }
    let Some(cut) = &out.cut else {
        let why = if out.not_lowered.is_empty() { "the program only declares".to_string() } else { out.not_lowered.join("; ") };
        return fail(format!("there is nothing to compile: {why}"));
    };
    if cli.cut {
        println!("{cut}\n");
    }
    let Some(exe) = &cli.output else { return ExitCode::SUCCESS };
    let dir = std::env::temp_dir().join(format!("idyllic-{}", std::process::id()));
    let name = exe.file_stem().map_or("program".into(), |s| s.to_string_lossy().into_owned());
    let built = idyllic::meadow::build(cut, idyllic::meadow::Runtime::Silo, &dir, &name)
        .and_then(|made| std::fs::copy(&made, exe).map(|_| ()).map_err(|e| format!("cannot write {}: {e}", exe.display())));
    let _ = std::fs::remove_dir_all(&dir);
    match built {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => fail(why),
    }
}

fn check_file(path: &Path, opts: &Options, cli: &Cli) -> ExitCode {
    let package = match load(path, cli.linear) {
        Ok(package) => package,
        Err(why) => {
            eprintln!("error: {why}");
            return ExitCode::FAILURE;
        }
    };
    let runtime = if cli.release { idyllic::meadow::Runtime::Silo } else { idyllic::meadow::Runtime::Glade };
    let out = pipeline::run_package_on(&package, opts, cli.args.clone(), runtime);
    if let Some(report) = out.cut_report().filter(|_| cli.cut) {
        print!("{report}");
    }
    // (a `main` that printed nothing shows nothing)
    let root = &package.modules[0];
    let shown = out.display(&root.file, &root.text, opts.style);
    if !shown.is_empty() {
        println!("{shown}");
    }
    match &out.evaluation {
        _ if out.has_errors() => ExitCode::FAILURE,
        // what the program asked for
        Some(pipeline::Evaluation::Exited(code)) => ExitCode::from(*code as u8),
        Some(pipeline::Evaluation::Stopped(_) | pipeline::Evaluation::Failed(_)) => ExitCode::FAILURE,
        _ => ExitCode::SUCCESS,
    }
}
