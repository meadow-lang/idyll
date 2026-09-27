//! `idyll`: the user-facing tool around the compiler.  For now that is the
//! REPL; it is also where the build system will live.  The compiler itself
//! (`idyllic`, in compiler/) only checks and compiles single programs.

mod repl;

use clap::Parser;
use idyllic::pipeline::Options;
use idyllic_diagnostics::OutputStyle;
use std::process::ExitCode;

/// The Idyll tool.  With no arguments, starts the REPL.
#[derive(Parser, Debug)]
#[command(name = "idyll", version, about)]
struct Cli {
    /// Dump the HIR (after renaming and the SCC pass) for each entry.
    #[arg(long)]
    debug: bool,

    /// Never colour output (the NO_COLOR environment variable also works).
    #[arg(long)]
    no_color: bool,

    /// Draw diagnostics with ASCII instead of box-drawing characters.
    #[arg(long)]
    ascii: bool,

    /// Print the AxCut each entry is lowered to, and what running it did with
    /// memory, before its result.
    #[arg(long)]
    axcut: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let mut style = OutputStyle::detect();
    style.color &= !cli.no_color;
    style.unicode &= !cli.ascii;
    let opts = Options { debug: cli.debug, style };
    repl::run(&opts, cli.axcut)
}
