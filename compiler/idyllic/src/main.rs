use clap::Parser;
use idyllic::pipeline::{self, Options};
use idyllic_diagnostics::OutputStyle;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The Idyll compiler: checks a program, runs it, and compiles it to native
/// code.  The REPL is the `idyll` command, in idyll-driver/.
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

    /// Arguments for the program, after `--`: what its `args ()` gives it.
    #[arg(last = true)]
    args: Vec<String>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let mut style = OutputStyle::detect();
    style.color &= !cli.no_color;
    style.unicode &= !cli.ascii;
    let opts = Options { debug: cli.debug, style, live: true };

    if cli.emit_llvm.is_some() || cli.output.is_some() {
        compile_file(&cli.file, &opts, &cli)
    } else {
        check_file(&cli.file, &opts, cli.axcut, cli.args.clone())
    }
}

/// Check a program and compile it: to LLVM IR, and with `-o`, on through
/// clang to an executable.  The runtime is compiled along with it, with
/// link-time optimisation, so that allocation is inlined into the program.
/// The package at `path`: a directory's, or one file's, which is a package
/// of one module named after it.
fn load(path: &Path) -> Result<pipeline::PackageSource, String> {
    if path.is_dir() {
        return pipeline::load_package(path);
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let name = path.file_stem().map_or("Main".into(), |s| s.to_string_lossy().into_owned());
    Ok(pipeline::PackageSource::single(&name, &path.display().to_string(), &text))
}

fn compile_file(path: &Path, opts: &Options, cli: &Cli) -> ExitCode {
    let package = match load(path) {
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
    let Some(program) = &out.native else { return fail("there is no expression to compile: the program only declares".into()) };
    if cli.axcut {
        println!("{program}\n");
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

fn check_file(path: &Path, opts: &Options, axcut: bool, args: Vec<String>) -> ExitCode {
    let package = match load(path) {
        Ok(package) => package,
        Err(why) => {
            eprintln!("error: {why}");
            return ExitCode::FAILURE;
        }
    };
    let out = pipeline::run_package(&package, opts, args);
    if let Some(report) = out.axcut_report().filter(|_| axcut) {
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
