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
    /// Source file to check.
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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let mut style = OutputStyle::detect();
    style.color &= !cli.no_color;
    style.unicode &= !cli.ascii;
    let opts = Options { debug: cli.debug, style };

    if cli.emit_llvm.is_some() || cli.output.is_some() {
        compile_file(&cli.file, &opts, &cli)
    } else {
        check_file(&cli.file, &opts, cli.axcut)
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

fn check_file(path: &Path, opts: &Options, axcut: bool) -> ExitCode {
    let src = match std::fs::read_to_string(path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let out = pipeline::run(&src, opts);
    if let Some(report) = out.axcut_report().filter(|_| axcut) {
        print!("{report}");
    }
    println!("{}", out.display(&path.display().to_string(), &src, opts.style));
    if out.has_errors() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
