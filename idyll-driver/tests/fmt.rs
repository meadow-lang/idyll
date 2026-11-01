//! `idyll fmt`: which files it reads, what it leaves alone, and what
//! `--check` says. How a file is set out is `idyllic-format`'s to test.

use idyll::fmt::{Options, Outcome, run};
use idyllic_diagnostics::OutputStyle;
use std::path::{Path, PathBuf};

fn scratch(who: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idyll-fmt-{}-{who}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fmt(paths: &[&Path], check: bool) -> Outcome {
    let paths = paths.iter().map(|p| p.to_path_buf()).collect();
    run(&Options { paths, check, stdout: false, style: OutputStyle::PLAIN })
}

const LOOSE: &str = "x   =   1\n";
const SET_OUT: &str = "x = 1\n";

#[test]
fn a_file_is_set_out_in_place() {
    let dir = scratch("place");
    let file = dir.join("a.idl");
    std::fs::write(&file, LOOSE).unwrap();
    let outcome = fmt(&[&file], false);
    assert_eq!(outcome.changed, vec![file.clone()]);
    assert!(outcome.ok());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), SET_OUT);
    assert_eq!(fmt(&[&file], false), Outcome::default(), "nothing more to do");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_says_which_and_changes_nothing() {
    let dir = scratch("check");
    let (loose, set) = (dir.join("a.idl"), dir.join("b.idl"));
    std::fs::write(&loose, LOOSE).unwrap();
    std::fs::write(&set, SET_OUT).unwrap();
    let outcome = fmt(&[&loose, &set], true);
    assert_eq!(outcome.unformatted, vec![loose.clone()]);
    assert!(!outcome.ok());
    assert_eq!(std::fs::read_to_string(&loose).unwrap(), LOOSE);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_directory_is_its_sources_but_for_what_is_built() {
    let dir = scratch("dir");
    for sub in ["src/inner", "target/debug", ".git"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    for file in ["src/a.idl", "src/inner/b.idl", "target/debug/c.idl", ".git/d.idl", "src/notes.txt"] {
        std::fs::write(dir.join(file), LOOSE).unwrap();
    }
    let outcome = fmt(&[&dir], false);
    assert_eq!(outcome.changed, vec![dir.join("src").join("a.idl"), dir.join("src").join("inner").join("b.idl")]);
    assert_eq!(std::fs::read_to_string(dir.join("target/debug/c.idl")).unwrap(), LOOSE);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_with_errors_is_left_as_it_is_and_said() {
    let dir = scratch("errors");
    let file = dir.join("a.idl");
    std::fs::write(&file, "x = (1 +\n").unwrap();
    let outcome = fmt(&[&file], false);
    assert_eq!(outcome.failed.len(), 1);
    assert!(!outcome.ok());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "x = (1 +\n");
    let _ = std::fs::remove_dir_all(&dir);
}
