//! `idyll fmt`: set files out in place (`idyllic_format`).

use idyllic_diagnostics::OutputStyle;
use std::path::{Path, PathBuf};

pub struct Options {
    /// Files or directories; a directory means every `.idl` under it.
    pub paths: Vec<PathBuf>,
    /// Change nothing: say which files are not set out.
    pub check: bool,
    /// Write the result to standard output, not back to the file.
    pub stdout: bool,
    pub style: OutputStyle,
}

/// What became of the files.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Those set out now.
    pub changed: Vec<PathBuf>,
    /// With `check`: those that are not set out.
    pub unformatted: Vec<PathBuf>,
    /// Those that could not be read, or have errors, which are left as they
    /// are; each with what was wrong.
    pub failed: Vec<(PathBuf, String)>,
}

impl Outcome {
    pub fn ok(&self) -> bool {
        self.unformatted.is_empty() && self.failed.is_empty()
    }
}

/// The `.idl` files `path` names, in order: itself, or those under it, but
/// for what is built and what is hidden.
fn sources(path: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !path.is_dir() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    let mut entries = std::fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let file = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if file.is_dir() {
            if name != "target" && !name.starts_with('.') {
                sources(&file, out)?;
            }
        } else if name.ends_with(".idl") {
            out.push(file);
        }
    }
    Ok(())
}

pub fn run(opts: &Options) -> Outcome {
    let mut outcome = Outcome::default();
    let mut files = Vec::new();
    for path in &opts.paths {
        if let Err(e) = sources(path, &mut files) {
            outcome.failed.push((path.clone(), format!("{}: could not read it: {e}\n", path.display())));
        }
    }
    for file in files {
        let shown = file.display().to_string();
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) => {
                outcome.failed.push((file, format!("{shown}: could not read it: {e}\n")));
                continue;
            }
        };
        let out = match idyllic_format::format(&text) {
            Ok(out) => out,
            Err(problems) => {
                let said = idyllic_diagnostics::render(&problems, &shown, &text, opts.style);
                outcome.failed.push((file, said));
                continue;
            }
        };
        if opts.stdout {
            print!("{out}");
        } else if out != text {
            if opts.check {
                outcome.unformatted.push(file);
            } else if let Err(e) = std::fs::write(&file, out) {
                outcome.failed.push((file, format!("{shown}: could not write it: {e}\n")));
            } else {
                outcome.changed.push(file);
            }
        }
    }
    outcome
}
