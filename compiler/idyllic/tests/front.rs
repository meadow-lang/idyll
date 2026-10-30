//! The compiler written in Meadow, `front/`, checked against this one, pass
//! by pass, as MeadowBoot is checked against the Rust Meadow compiler.
//!
//! Each pass of `front/` writes what it made as text, and the same pass here
//! is written the same way; a program whose two texts differ is a difference
//! between the compilers, shown at its first token. The inputs are every
//! program of the compiler's corpora (`tests/corpus/`).
//!
//! `front/` is built once a run, natively (`meadow build --release`), with the
//! `meadow` on the path; with none, the tests say so and pass, so that a
//! machine without Meadow can still run the rest. `IDYLL_FRONT_MEADOW` names
//! another `meadow` to build it with.

use idyllic_lexer::token::Token;
use idyllic_span::LineIndex;

#[path = "support/tutorial_examples.rs"]
#[allow(dead_code)]
mod tutorial_examples;

#[path = "support/front.rs"]
#[allow(dead_code)]
mod harness;
use harness::*;

/// The tokens this compiler's lexer and layout make of `src`, as `front/`
/// writes them: each as the text it covers, the virtual ones as «{» «;» «}»,
/// a space between each two.
fn tokens(src: &str) -> String {
    let lexed = idyllic_lexer::tokenize(src);
    let laid = idyllic_layout::layout(&LineIndex::new(src), &lexed.tokens);
    laid.tokens
        .iter()
        .map(|t| match t.node {
            Token::VLBrace => "«{»".to_string(),
            Token::VSemi => "«;»".to_string(),
            Token::VRBrace => "«}»".to_string(),
            _ => src[t.span.start..t.span.end].to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn the_tokens_and_their_layout_agree() {
    agree("tokens", tokens);
}

#[test]
fn the_parses_agree() {
    agree("parse", parsed);
}

/// A parse dump with what is named after where it is in the text -- an
/// instance with no name of its own, `#instance@<offset>` -- named alike.
fn unplaced(dump: &str) -> String {
    let mut out = String::with_capacity(dump.len());
    let mut rest = dump;
    while let Some(at) = rest.find("#instance@") {
        let (head, tail) = rest.split_at(at + "#instance@".len());
        out.push_str(head);
        rest = tail.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    out.push_str(rest);
    out
}

/// `Idyll fmt` sets every program out as the same program: this compiler
/// parses what it wrote as it parsed what was written (layout and all), and
/// formatting it again changes nothing. Each program this compiler parses
/// is written to a file, formatted in place, and formatted again.
#[test]
fn front_formats_every_program_as_the_same_program() {
    let exe = match front() {
        Ok(exe) => exe,
        Err(why) if why.starts_with("no meadow") => {
            eprintln!("{why}: front/ was not checked");
            return;
        }
        Err(why) => panic!("{why}"),
    };
    let programs: Vec<(String, String)> = corpus().into_iter().filter(|(_, src)| parsed(src) != "(error)").collect();
    let dir = std::env::temp_dir().join(format!("idyll-front-fmt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory for the programs");
    let files: Vec<String> = programs.iter().map(|(name, _)| format!("{name}.idl")).collect();
    for ((_, src), file) in programs.iter().zip(&files) {
        std::fs::write(dir.join(file), src).expect("a program written");
    }
    // a few hundred at a time, as a command line has a length it may not
    // pass; a program front/ will not set out (one with errors it finds
    // and this compiler's parser does not) is left as it is, and said
    let format = || {
        for batch in files.chunks(200) {
            std::process::Command::new(&exe).arg("fmt").args(batch).current_dir(&dir).output().expect("front/ runs");
        }
        files.iter().map(|f| std::fs::read_to_string(dir.join(f)).expect("a program read").replace("\r\n", "\n")).collect::<Vec<_>>()
    };
    let once = format();
    let twice = format();
    let _ = std::fs::remove_dir_all(&dir);
    let mut differ = Vec::new();
    let mut changed = 0;
    for (((name, src), once), twice) in programs.iter().zip(&once).zip(&twice) {
        if once != src {
            changed += 1;
        }
        if std::env::var("IDYLL_FRONT_SHOW").is_ok_and(|v| v.split(',').any(|n| n == name)) {
            eprintln!("--- {name}, as written:\n{src}\n--- set out:\n{once}");
        }
        let (before, after) = (unplaced(&parsed(src)), unplaced(&parsed(once)));
        if before != after {
            differ.push(format!("{name}: read as another program, {}", first_difference(&before, &after)));
        } else if once != twice {
            differ.push(format!("{name}: set out again, it changes:\n{}", first_difference(once, twice)));
        }
    }
    eprintln!("fmt: {} programs, {changed} of them changed, {} not the same", programs.len(), differ.len());
    assert!(
        differ.is_empty(),
        "{} of {} programs are not set out as the same program:\n{}",
        differ.len(),
        programs.len(),
        differ.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn the_renames_agree() {
    let source = idyllic::pipeline::PRELUDE;
    let prelude = program(source).expect("the prelude parses");
    agree_with(
        &["rename", "--prelude", "_prelude.txt"],
        &[("_prelude.txt", source)],
        |src| renamed(&prelude, src),
        renumbered,
    );
}

#[test]
fn the_types_agree() {
    let source = idyllic::pipeline::PRELUDE;
    let prelude = program(source).expect("the prelude parses");
    let std_dir = repo().join("lib/Std/src");
    let std_dir = std_dir.to_string_lossy();
    agree_with(
        &["types", "--prelude", "_prelude.txt", "--std", &std_dir],
        &[("_prelude.txt", source)],
        |src| typed(&prelude, src),
        |d| renumbered_by(d, '?'),
    );
}

/// The headlines of the checker's errors that front/ says as this
/// compiler does, its kinds ported a few at a time (front/'s `Messages`),
/// each as a pattern: `*` stands for what a headline names (a name, a
/// type, a number), and the rest must be as written.
const PORTED: &[&str] = &[
    "type mismatch",
    "this is not a function",
    "unsolved metavariable",
    "the patterns do not cover every case",
    "unreachable",
    "more than one hint gives a *",
    "nothing in scope gives *",
    "cannot tell what to look for",
    "no auto argument is taken here",
    "no implicit argument is taken here",
    "unsolved hole",
    "found hole",
    "usage mismatch",
    "a linear value used more than once",
    "* may not terminate",
    "* is not a constructor",
    "* is not a constructor of this type",
    "* has * fields, not *",
    "this cannot be matched on",
    "this is not a box",
    "a sequence literal needs a sequence type",
    "* is resumed more than once",
    "a type-level grade needs a function type to check it against",
    "the recursive binding * needs a type signature",
    "* does not fit in an *",
    "not impossible",
    "no implicit argument is here to match",
    "strings cannot be matched on yet",
    "a universe level is a number",
    "one handler, one effect",
    "this handler handles nothing",
    "effects not allowed",
    "unhandled effects",
    "a type must be pure",
    "an erased argument must be pure",
    "a type cannot depend on this",
];

/// Whether `headline` is of the shape `pattern`, each `*` in it any text.
fn shaped(headline: &str, pattern: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return headline == pattern;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !headline.starts_with(first) || !headline.ends_with(last) || headline.len() < first.len() + last.len() {
        return false;
    }
    let mut rest = &headline[first.len()..headline.len() - last.len()];
    for part in &parts[1..parts.len() - 1] {
        match rest.find(part) {
            Some(i) => rest = &rest[i + part.len()..],
            None => return false,
        }
    }
    true
}

/// Each error the checker found, said in words, by both: its headline and
/// its label, for the kinds front/ says as this compiler does (`PORTED`).
/// Each distinct line once, as front/ keeps one error of a kind at a node
/// where this compiler may keep one for each part of it.
#[test]
fn the_messages_agree() {
    let source = idyllic::pipeline::PRELUDE;
    let prelude = program(source).expect("the prelude parses");
    let std_dir = repo().join("lib/Std/src");
    let std_dir = std_dir.to_string_lossy();
    agree_with(
        &["messages", "--prelude", "_prelude.txt", "--std", &std_dir],
        &[("_prelude.txt", source)],
        |src| messages(&prelude, src),
        ported_lines,
    );
}

/// A message dump's lines of the kinds ported, each once, sorted.
fn ported_lines(dump: &str) -> String {
    if dump.starts_with('(') {
        return dump.to_string();
    }
    let headline = |l: &str| l.split(": ").next().unwrap_or("").to_string();
    // (the renamer's errors are each said as the Rust renamer says them)
    let mut lines: Vec<&str> =
        dump.lines().filter(|l| l.starts_with("rename: ") || PORTED.iter().any(|p| shaped(&headline(l), p))).collect();
    lines.sort_unstable();
    lines.dedup();
    // (each side's metavariables numbered by first appearance)
    renumbered_by(&lines.join("\n"), '?')
}

/// A program's errors, said in words: each diagnostic's headline and label,
/// a line each; `(error)` if it does not parse or rename.
fn messages(prelude: &idyllic_ast::Program, src: &str) -> String {
    let Some(p) = program(src) else { return "(error)".to_string() };
    let std = idyllic::stdlib::compiled();
    let uses_std = src.contains("Std");
    let result = if uses_std {
        let modules = [idyllic_rename::ModuleSource { path: Vec::new(), program: &p }];
        let first = std.uniques.end.max(idyllic_builtins::FIRST_USER_UNIQUE);
        idyllic_rename::rename_package("Main", &modules, prelude, &[("Std", &std.names)], first, false)
    } else {
        idyllic_rename::rename_module(&p, prelude)
    };
    // what renaming found, each said as it says it
    if !result.errors.is_empty() {
        return result.errors.iter().map(|e| format!("rename: {}", e.to_diagnostic().message)).collect::<Vec<_>>().join("\n");
    }
    let spans = result.spans.clone();
    let module = idyllic_scc::scc_module(result.module);
    let deps: Vec<&idyllic_typecheck::packages::CompiledPackage> = if uses_std { vec![&*std] } else { Vec::new() };
    let checked = idyllic_typecheck::check_module(&module, spans.len(), false, &deps);
    checked
        .errors
        .iter()
        .map(|e| {
            let d = e.to_diagnostic(&spans);
            match d.labels.first() {
                Some(l) => format!("{}: {}", d.message, l.message),
                None => d.message.clone(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The standard library, checked as the package it is, by both: each of
/// its declarations' types, and its errors (none).
/// A compiled package as front/'s `compiled` writes one: a line for each
/// constant (its type, whether the types may see its value, how many
/// arguments it unfolds after), each top-level `let`, each recursive
/// definition, each hint and each trait, sorted.
fn compiled_dump(p: &idyllic_typecheck::packages::CompiledPackage) -> String {
    idyllic_typecheck::core::print_linear(false);
    let name_of = |u: usize| p.constants.iter().find(|c| c.unique == u).map_or("?".to_string(), |c| c.name.clone());
    let mut lines: Vec<String> = Vec::new();
    for c in &p.constants {
        let value = if c.value.is_some() { " = .." } else { "" };
        let unfold = c.unfold.as_ref().map(|(arity, _)| format!(" unfold {arity}")).unwrap_or_default();
        lines.push(format!("constant {} : {}{value}{unfold}", c.name, c.ty));
    }
    lines.extend(p.lets.iter().map(|(_, name, _)| format!("let {name}")));
    lines.extend(p.rec_defs.iter().map(|d| if d.local { format!("rec {} local", d.name) } else { format!("rec {}", d.name) }));
    lines.extend(p.hints.iter().map(|&u| format!("hint {}", name_of(u))));
    lines.extend(p.traits.iter().map(|&u| format!("trait {}", name_of(u))));
    lines.sort();
    lines.join("\n")
}

/// Std compiled whole, as a package its dependents are checked against:
/// front/'s `Compiled` and the Rust compiler's `CompiledPackage` say the
/// same of it.
#[test]
fn the_compiled_packages_agree() {
    let Ok(exe) = front() else {
        eprintln!("no front/ to compile the Std package with");
        return;
    };
    let source = idyllic::pipeline::PRELUDE;
    let prelude = program(source).expect("the prelude parses");
    let dir = std::env::temp_dir().join(format!("idyll-front-compiled-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory");
    std::fs::write(dir.join("_prelude.txt"), source).expect("the prelude can be written");
    let out = std::process::Command::new(&exe)
        .args(["compiled", "--prelude", "_prelude.txt", "Std"])
        .arg(repo().join("lib/Std/src"))
        .current_dir(&dir)
        .output()
        .expect("front/ runs");
    let _ = std::fs::remove_dir_all(&dir);
    let text = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    let theirs = text.strip_prefix("== Std\n").unwrap_or(&text).trim_end().to_string();
    let programs: Vec<(Vec<String>, idyllic_ast::Program)> = idyllic::stdlib::MODULES
        .iter()
        .map(|(path, text)| {
            let path = if path.is_empty() { Vec::new() } else { path.split('.').map(String::from).collect() };
            (path, program(text).expect("a Std module parses"))
        })
        .collect();
    let modules: Vec<idyllic_rename::ModuleSource<'_>> =
        programs.iter().map(|(path, program)| idyllic_rename::ModuleSource { path: path.clone(), program }).collect();
    let result = idyllic_rename::rename_package("Std", &modules, &prelude, &[], idyllic_builtins::FIRST_USER_UNIQUE, false);
    let module = idyllic_scc::scc_module(result.module);
    let checked = idyllic_typecheck::check_module(&module, result.spans.len(), false, &[]);
    let compiled = checked.compiled.expect("Std compiles");
    let ours = renumbered_by(&compiled_dump(&compiled), '?');
    let theirs = renumbered_by(&theirs, '?');
    eprintln!("compiled: {} lines", ours.lines().count());
    if let Ok(dir) = std::env::var("IDYLL_DUMP") {
        let _ = std::fs::write(format!("{dir}/compiled-here.txt"), &ours);
        let _ = std::fs::write(format!("{dir}/compiled-front.txt"), &theirs);
    }
    assert!(ours == theirs, "the compiled Std package differs: {}", first_difference(&ours, &theirs));
}

/// Std compiled a part at a time, each part against the parts it imports,
/// and the parts made one: front/'s `parted` and the Rust compiler's Std,
/// which is compiled so, say the same of it.
#[test]
fn the_compiled_parts_agree() {
    let Ok(exe) = front() else {
        eprintln!("no front/ to compile the Std package with");
        return;
    };
    let source = idyllic::pipeline::PRELUDE;
    let dir = std::env::temp_dir().join(format!("idyll-front-parted-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory");
    std::fs::write(dir.join("_prelude.txt"), source).expect("the prelude can be written");
    let out = std::process::Command::new(&exe)
        .args(["parted", "--prelude", "_prelude.txt", "Std"])
        .arg(repo().join("lib/Std/src"))
        .current_dir(&dir)
        .output()
        .expect("front/ runs");
    let _ = std::fs::remove_dir_all(&dir);
    let text = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    let theirs = renumbered_by(text.strip_prefix("== Std\n").unwrap_or(&text).trim_end(), '?');
    let ours = renumbered_by(&compiled_dump(&idyllic::stdlib::compiled()), '?');
    eprintln!("parted: {} lines", ours.lines().count());
    assert!(ours == theirs, "the Std package compiled in parts differs: {}", first_difference(&ours, &theirs));
}

#[test]
fn the_std_package_agrees() {
    let Ok(exe) = front() else {
        eprintln!("no front/ to check the Std package with");
        return;
    };
    let source = idyllic::pipeline::PRELUDE;
    let prelude = program(source).expect("the prelude parses");
    let dir = std::env::temp_dir().join(format!("idyll-front-std-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory");
    std::fs::write(dir.join("_prelude.txt"), source).expect("the prelude can be written");
    let std_dir = repo().join("lib/Std/src");
    let out = std::process::Command::new(&exe)
        .args(["package", "--prelude", "_prelude.txt", "Std"])
        .arg(&std_dir)
        .current_dir(&dir)
        .output()
        .expect("front/ runs");
    let _ = std::fs::remove_dir_all(&dir);
    let text = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    let theirs = text.strip_prefix("== Std\n").unwrap_or(&text).trim_end().to_string();
    // here: the package's modules, checked as `Std` is
    let programs: Vec<(Vec<String>, idyllic_ast::Program)> = idyllic::stdlib::MODULES
        .iter()
        .map(|(path, text)| {
            let path = if path.is_empty() { Vec::new() } else { path.split('.').map(String::from).collect() };
            (path, program(text).expect("a Std module parses"))
        })
        .collect();
    // (each module after those it uses, as the package's parts have them)
    let paths: Vec<Vec<String>> = programs.iter().map(|(p, _)| p.clone()).collect();
    let texts: Vec<&str> = idyllic::stdlib::MODULES.iter().map(|(_, t)| *t).collect();
    let order: Vec<usize> = idyllic::parts::parts("Std", &paths, &texts).into_iter().flat_map(|p| p.modules).collect();
    let modules: Vec<idyllic_rename::ModuleSource<'_>> =
        order.iter().map(|&i| idyllic_rename::ModuleSource { path: programs[i].0.clone(), program: &programs[i].1 }).collect();
    let result = idyllic_rename::rename_package("Std", &modules, &prelude, &[], idyllic_builtins::FIRST_USER_UNIQUE, false);
    assert!(result.errors.is_empty(), "Std renames");
    let module = idyllic_scc::scc_module(result.module);
    let checked = idyllic_typecheck::check_module(&module, result.spans.len(), false, &[]);
    let ours = renumbered_by(&checked_dump(&checked), '?');
    let theirs = renumbered_by(&theirs, '?');
    eprintln!("std: {} declarations", ours.lines().count());
    assert!(ours == theirs, "the Std package differs: {}", first_difference(&ours, &theirs));
}

#[test]
fn the_cut_agrees() {
    let source = idyllic::pipeline::PRELUDE;
    let prelude = program(source).expect("the prelude parses");
    let std_dir = repo().join("lib/Std/src");
    let std_dir = std_dir.to_string_lossy();
    agree_with(
        &["cut", "--prelude", "_prelude.txt", "--std", &std_dir],
        &[("_prelude.txt", source)],
        |src| cut(&prelude, src),
        cut_canonical,
    );
}

/// Every example package (`examples/`), a whole program of modules using
/// Std, built by front/ (`Idyll build`): to the same Cut as this
/// compiler's, which runs on Meadow (on Silo where the package's debug
/// profile says so, as `idyll run` would) to print what this compiler's
/// build prints.
#[test]
fn the_example_packages_build_and_run_alike() {
    let Ok(exe) = front() else {
        eprintln!("no front/ to build the examples with");
        return;
    };
    let opts = idyllic::pipeline::Options { debug: false, style: idyllic_diagnostics::OutputStyle::PLAIN, live: false };
    let dir = std::env::temp_dir().join(format!("idyll-front-examples-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory");
    let prelude = dir.join("_prelude.txt");
    std::fs::write(&prelude, idyllic::pipeline::PRELUDE).expect("the prelude can be written");
    let std_dir = repo().join("lib/Std/src");
    let mut packages: Vec<std::path::PathBuf> = std::fs::read_dir(repo().join("examples"))
        .expect("the examples")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("Idyll.toml").exists())
        .collect();
    packages.sort();
    let mut built = Vec::new();
    for pkg in &packages {
        let name = pkg.file_name().unwrap_or_default().to_string_lossy().to_string();
        let out = std::process::Command::new(&exe)
            .arg("build")
            .arg("--prelude")
            .arg(&prelude)
            .arg("--std")
            .arg(&std_dir)
            .arg(pkg)
            .output()
            .expect("front/ runs");
        let theirs = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
        assert!(theirs.starts_with("cut "), "front/ did not build {name}: {theirs}{}", String::from_utf8_lossy(&out.stderr));
        // the same Cut
        let package = idyllic::pipeline::load_package(pkg).expect("the package reads");
        let ours = idyllic::pipeline::compile_package(&package, &opts);
        let ours_cut = ours.cut.unwrap_or_else(|| panic!("{name} is not lowered here: {:?}", ours.not_lowered));
        let (a, b) = (cut_canonical(&ours_cut), cut_canonical(theirs.trim_end()));
        if a != b && std::env::var_os("IDYLL_FRONT_KEEP").is_some() {
            let _ = std::fs::write(std::env::temp_dir().join(format!("{name}.here.cut")), &a);
            let _ = std::fs::write(std::env::temp_dir().join(format!("{name}.front.cut")), &b);
        }
        assert!(a == b, "{name}'s Cut differs: {}", first_difference(&a, &b));
        // and it runs to what this compiler's build prints
        let silo = std::fs::read_to_string(pkg.join("Idyll.toml")).unwrap_or_default().contains("backend = \"silo\"");
        let file = dir.join(format!("{name}.cut"));
        std::fs::write(&file, &theirs).expect("the Cut can be written");
        let mut cmd = std::process::Command::new(meadow());
        cmd.arg("cut");
        if silo {
            cmd.args(["--runtime", "silo"]);
        }
        let ran = cmd.arg(&file).output().expect("meadow runs");
        let printed = String::from_utf8_lossy(&ran.stdout).replace("\r\n", "\n");
        let runtime = if silo { idyllic::meadow::Runtime::Silo } else { idyllic::meadow::Runtime::Glade };
        let expected = idyllic::pipeline::run_package_on(&package, &opts, Vec::new(), runtime);
        assert!(expected.diagnostics.is_empty(), "{name} does not build here");
        // (Silo says what it is building first)
        assert!(printed.ends_with(&expected.printed), "{name} printed\n{printed}\nwhere this compiler's build prints\n{}", expected.printed);
        assert!(!expected.printed.is_empty(), "{name} printed nothing");
        built.push(name);
    }
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("examples: {} built by front/ and run alike ({})", built.len(), built.join(", "));
    assert!(built.len() >= 6, "the examples are all there");
}

/// front/'s own command line (`Idyll run`, a `cli!` over a `make!`), run
/// in `dir`: what it printed, and whether it succeeded.
fn front_tool(exe: &std::path::Path, dir: &std::path::Path, args: &[&str]) -> (String, bool) {
    // (Std, and what is fetched, in a home of the tests' own)
    let home = std::env::temp_dir().join("idyll-front-home");
    let out = std::process::Command::new(exe).args(args).current_dir(dir).env("IDYLL_HOME", home).output().expect("front/ runs");
    // what it printed, the build's lines first and then the program's; and,
    // if it failed, what it wrote as errors (Silo's build says what it is
    // doing there too)
    let text = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    let errors = String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n");
    (if out.status.success() { text } else { text + &errors }, out.status.success())
}

/// A package with a dependency by path, built and run by front/'s own
/// `idyll run`: each package a unit of Lingua's `make!`, compiled only when
/// what it was compiled from changed.
#[test]
fn front_builds_and_runs_a_package_with_a_dependency() {
    let Ok(exe) = front() else {
        eprintln!("no front/ to build with");
        return;
    };
    let root = std::env::temp_dir().join(format!("idyll-front-make-{}", std::process::id()));
    let (util, app) = (root.join("util"), root.join("app"));
    for d in [&util, &app] {
        std::fs::create_dir_all(d.join("src")).expect("a directory");
    }
    std::fs::write(util.join("Idyll.toml"), "[package]\nname = \"Util\"\nversion = \"0.1.0\"\n").unwrap();
    std::fs::write(util.join("src/Lib.idl"), "@pub\nanswer : Int\nanswer = 42\n").unwrap();
    std::fs::write(app.join("Idyll.toml"), "[package]\nname = \"App\"\nversion = \"0.1.0\"\n\n[dependencies]\nUtil = { path = \"../util\" }\n").unwrap();
    std::fs::write(app.join("src/Main.idl"), "use Util (answer)\n\nmain : () -> () ! Eff\nmain () = putStrLn \"the answer is ${answer}\"\n").unwrap();
    // built, both and Std (a unit of its own, written out under the root's
    // target/), and run
    let (said, ok) = front_tool(&exe, &app, &["run"]);
    assert!(ok, "{said}");
    assert!(said.contains("idyll: 3 compiled, 0 up to date") && said.contains("the answer is 42\n"), "{said}");
    // nothing changed: nothing compiled, and the program is what it was
    let (said, ok) = front_tool(&exe, &app, &["run"]);
    assert!(ok && said.contains("idyll: 0 compiled, 3 up to date") && said.contains("Fresh link") && said.contains("the answer is 42\n"), "{said}");
    // the dependency changed: both compiled again
    std::fs::write(util.join("src/Lib.idl"), "@pub\nanswer : Int\nanswer = 6 * 7 + 1\n").unwrap();
    let (said, ok) = front_tool(&exe, &app, &["run"]);
    assert!(ok && said.contains("idyll: 2 compiled, 1 up to date") && said.contains("the answer is 43\n"), "{said}");
    // the package alone changed: only it
    std::fs::write(app.join("src/Main.idl"), "use Util (answer)\n\nmain : () -> () ! Eff\nmain () = putStrLn \"now ${answer}\"\n").unwrap();
    let (said, ok) = front_tool(&exe, &app, &["run"]);
    assert!(ok && said.contains("idyll: 1 compiled, 2 up to date") && said.contains("now 43\n"), "{said}");
    // and a package that does not check says so, and runs nothing
    std::fs::write(app.join("src/Main.idl"), "main : () -> () ! Eff\nmain () = putStrLn 5\n").unwrap();
    let (said, ok) = front_tool(&exe, &app, &["run"]);
    assert!(!ok && said.contains("Failed"), "{said}");
    let _ = std::fs::remove_dir_all(&root);
}

/// `git` in `dir`, which must succeed.
fn git(dir: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(["-c", "user.email=test@example.com", "-c", "user.name=test"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A package with a dependency by git, at a version, built by front/'s own
/// driver on Lingua's resolver: the newest release the version allows is
/// fetched and kept in `idyll.lock`, in the Rust driver's format; a build
/// after that needs no network; `idyll update` moves it to a newer release.
#[test]
fn front_builds_with_a_git_dependency() {
    let Ok(exe) = front() else {
        eprintln!("no front/ to build with");
        return;
    };
    if std::process::Command::new("git").arg("--version").output().is_err() {
        eprintln!("no git to make a dependency with");
        return;
    }
    let root = std::env::temp_dir().join(format!("idyll-front-git-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (json, app, home) = (root.join("json"), root.join("app"), root.join("home"));
    for d in [&json, &app] {
        std::fs::create_dir_all(d.join("src")).expect("a directory");
    }
    // the dependency, released as v1.0.0
    std::fs::write(json.join("Idyll.toml"), "[package]\nname = \"Json\"\nversion = \"1.0.0\"\n").unwrap();
    std::fs::write(json.join("src/Lib.idl"), "@pub\ngreet : String\ngreet = \"one\"\n").unwrap();
    git(&json, &["init", "-q"]);
    git(&json, &["add", "-A"]);
    git(&json, &["commit", "-qm", "one"]);
    git(&json, &["tag", "v1.0.0"]);
    let url = format!("file:///{}", json.display().to_string().replace('\\', "/").trim_start_matches('/'));
    std::fs::write(app.join("Idyll.toml"), format!("[package]\nname = \"App\"\n\n[dependencies]\nJson = {{ git = \"{url}\", version = \"1.0.0\" }}\n")).unwrap();
    std::fs::write(app.join("src/Main.idl"), "use Json (greet)\n\nmain : () -> () ! Eff\nmain () = putStrLn greet\n").unwrap();
    let run = |args: &[&str]| {
        let out = std::process::Command::new(&exe).args(args).current_dir(&app).env("IDYLL_HOME", &home).output().expect("front/ runs");
        let text = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n") + &String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n");
        (text, out.status.success())
    };
    let (said, ok) = run(&["run"]);
    assert!(ok && said.ends_with("one\n"), "{said}");
    let lock = std::fs::read_to_string(app.join("idyll.lock")).expect("the build writes idyll.lock");
    assert!(lock.contains("version = 1\n") && lock.contains("name = \"Json\"") && lock.contains("version = \"1.0.0\""), "{lock}");
    assert!(lock.contains(&format!("source = \"git+{url}?version=1.0.0\"")), "{lock}");
    // a newer release: the lock still says the old one, and needs no network
    std::fs::write(json.join("src/Lib.idl"), "@pub\ngreet : String\ngreet = \"two\"\n").unwrap();
    std::fs::write(json.join("Idyll.toml"), "[package]\nname = \"Json\"\nversion = \"1.1.0\"\n").unwrap();
    git(&json, &["commit", "-qam", "two"]);
    git(&json, &["tag", "v1.1.0"]);
    let (said, ok) = run(&["--offline", "--locked", "run"]);
    assert!(ok && said.ends_with("one\n"), "{said}");
    // a dry run says what would move, and moves nothing
    let (said, ok) = run(&["update", "--dry-run"]);
    assert!(ok && said.contains("Updating Json 1.0.0 -> 1.1.0"), "{said}");
    assert_eq!(std::fs::read_to_string(app.join("idyll.lock")).unwrap(), lock);
    // `idyll update` moves it
    let (said, ok) = run(&["update"]);
    assert!(ok && said.contains("Updating Json 1.0.0 -> 1.1.0"), "{said}");
    assert!(std::fs::read_to_string(app.join("idyll.lock")).unwrap().contains("version = \"1.1.0\""));
    let (said, ok) = run(&["run"]);
    assert!(ok && said.ends_with("two\n"), "{said}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Every example package run by front/'s own `idyll run` (on Silo where its
/// profile says so), from a copy of it: what this compiler's build prints.
#[test]
fn front_runs_every_example_itself() {
    let Ok(exe) = front() else {
        eprintln!("no front/ to build with");
        return;
    };
    let opts = idyllic::pipeline::Options { debug: false, style: idyllic_diagnostics::OutputStyle::PLAIN, live: false };
    let root = std::env::temp_dir().join(format!("idyll-front-run-{}", std::process::id()));
    let mut ran = Vec::new();
    for e in std::fs::read_dir(repo().join("examples")).expect("the examples").flatten() {
        let pkg = e.path();
        if !pkg.join("Idyll.toml").exists() {
            continue;
        }
        let name = pkg.file_name().unwrap_or_default().to_string_lossy().to_string();
        let copy = root.join(&name);
        copy_tree(&pkg, &copy);
        let silo = std::fs::read_to_string(pkg.join("Idyll.toml")).unwrap_or_default().contains("backend = \"silo\"");
        let (said, ok) = front_tool(&exe, &copy, if silo { &["run", "--release"] } else { &["run"] });
        assert!(ok, "{name}: {said}");
        let package = idyllic::pipeline::load_package(&pkg).expect("the package reads");
        let runtime = if silo { idyllic::meadow::Runtime::Silo } else { idyllic::meadow::Runtime::Glade };
        let expected = idyllic::pipeline::run_package_on(&package, &opts, Vec::new(), runtime);
        assert!(said.ends_with(&expected.printed), "{name} printed\n{said}\nwhere this compiler's build prints\n{}", expected.printed);
        ran.push(name);
    }
    let _ = std::fs::remove_dir_all(&root);
    eprintln!("examples: {} run by front/'s own idyll run ({})", ran.len(), ran.join(", "));
    assert!(ran.len() >= 6);
}

/// A directory copied, all but what a build left in it.
fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).expect("a directory");
    for e in std::fs::read_dir(from).expect("a directory").flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            if e.file_name() != "target" {
                copy_tree(&src, &dst);
            }
        } else {
            std::fs::copy(&src, &dst).expect("a copy");
        }
    }
}

// --- the Cut, written out ------------------------------------------------------
//
// Both compilers' Cut as its text (`front/` builds it with the Cut package,
// this compiler with its own printer), compared once canonical: with no
// whitespace, which the two printers lay out differently; with the names
// the lowering made (`k_23`, `d_a_4`) numbered afresh in each declaration in
// the order they appear, since this compiler lowers the recursive
// definitions in a hash map's order, and so numbers them differently from
// run to run; and with the declarations sorted, for the same reason.

/// What this compiler lowers `src` to, after the prelude, as Cut's text.
fn cut(prelude: &idyllic_ast::Program, src: &str) -> String {
    let Some(p) = program(src) else { return "(error)".to_string() };
    // a program that names `Std` depends on it: compiled, and linked with it
    if src.contains("Std") {
        let opts = idyllic::pipeline::Options { debug: false, style: idyllic_diagnostics::OutputStyle::PLAIN, live: false };
        let out = idyllic::pipeline::compile(src, &opts);
        if out.has_errors() {
            return "(error)".to_string();
        }
        let Some(mut checked) = out.checked else { return "(error)".to_string() };
        return match idyllic_cut::lower::lower(&mut checked, "Main") {
            Ok(program) => program.to_string(),
            Err(why) => format!("(not lowered: {})", why.join("; ")),
        };
    }
    let result = idyllic_rename::rename_module(&p, prelude);
    if !result.errors.is_empty() {
        return "(error)".to_string();
    }
    let module = idyllic_scc::scc_module(result.module);
    let mut checked = idyllic_typecheck::check_module(&module, result.spans.len(), false, &[]);
    if !checked.errors.is_empty() {
        return "(error)".to_string();
    }
    match idyllic_cut::lower::lower(&mut checked, "Main") {
        Ok(program) => program.to_string(),
        Err(why) => format!("(not lowered: {})", why.join("; ")),
    }
}

/// A Cut text in the canonical form described above.
fn cut_canonical(text: &str) -> String {
    if text.starts_with('(') {
        return text.to_string();
    }
    let starts = ["data ", "roles {", "native {", "effect ", "val ", "def "];
    let (mut header, mut decls): (Vec<&str>, Vec<Vec<&str>>) = (Vec::new(), Vec::new());
    for line in text.lines() {
        if starts.iter().any(|s| line.starts_with(s)) {
            decls.push(vec![line]);
        } else if let Some(d) = decls.last_mut() {
            d.push(line);
        } else {
            header.push(line);
        }
    }
    let squeeze = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let mut out: Vec<String> = decls.iter().map(|d| renamed_fresh(&squeeze(&d.join("\n")))).collect();
    out.sort();
    let mut all = vec![squeeze(&header.join("\n"))];
    all.extend(out);
    all.join("\n")
}

/// Each name of the form `hint_123` numbered by where it first appears:
/// `#0`, `#1`, ...
fn renamed_fresh(decl: &str) -> String {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let chars: Vec<char> = decl.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    while i < chars.len() {
        if word(chars[i]) && (i == 0 || !word(chars[i - 1])) {
            let start = i;
            while i < chars.len() && word(chars[i]) {
                i += 1;
            }
            let w: String = chars[start..i].iter().collect();
            let fresh = w.rsplit_once('_').is_some_and(|(h, n)| !h.is_empty() && !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
            if fresh {
                let n = seen.len();
                out.push_str(&format!("#{}", seen.entry(w).or_insert(n)));
            } else {
                out.push_str(&w);
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

// --- the types, written out ---------------------------------------------------
//
// As `front/src/Check.mw` writes them: a line each, sorted, for each of the
// program's declarations with its type (`name : type`), the type of its
// expression (`body : type`), and each error, by the name of its kind
// (`error: Mismatch`). Metavariables are numbered by first appearance on
// both sides.

/// What this compiler's checker makes of `src`, after the prelude, written
/// out; a program that names `Std` is checked against it, compiled, as a
/// package depends on it.
fn typed(prelude: &idyllic_ast::Program, src: &str) -> String {
    let Some(p) = program(src) else { return "(error)".to_string() };
    let std = idyllic::stdlib::compiled();
    let uses_std = src.contains("Std");
    let result = if uses_std {
        let modules = [idyllic_rename::ModuleSource { path: Vec::new(), program: &p }];
        let first = std.uniques.end.max(idyllic_builtins::FIRST_USER_UNIQUE);
        idyllic_rename::rename_package("Main", &modules, prelude, &[("Std", &std.names)], first, false)
    } else {
        idyllic_rename::rename_module(&p, prelude)
    };
    if !result.errors.is_empty() {
        return "(error)".to_string();
    }
    let module = idyllic_scc::scc_module(result.module);
    let deps: Vec<&idyllic_typecheck::packages::CompiledPackage> = if uses_std { vec![&*std] } else { Vec::new() };
    let checked = idyllic_typecheck::check_module(&module, result.spans.len(), false, &deps);
    checked_dump(&checked)
}

/// A checked module written out: its own declarations with their types, its
/// expression's type, and its errors by kind, a line each, sorted.
fn checked_dump(checked: &idyllic_typecheck::TypeCheckResult) -> String {
    let mut lines: Vec<String> =
        checked.catalog.iter().filter(|d| !d.prelude).map(|d| format!("{} : {}", d.name, d.ty)).collect();
    lines.extend(checked.body_type.iter().map(|ty| format!("body : {ty}")));
    for e in &checked.errors {
        let kind = format!("{:?}", e.kind);
        let name = kind.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("").to_string();
        lines.push(format!("error: {name}"));
    }
    lines.sort();
    lines.join("\n")
}

// --- the rename, written out --------------------------------------------------
//
// As `front/src/Hir.mw` writes it: the program's own part of the module, a line
// for each declaration, every name `text@unique`. `renumbered` then numbers
// the uniques in the order they first appear, on both sides, so that what is
// compared is which use is of which binder, not how each renamer counts.

/// What this compiler's renamer makes of `src`, after the prelude, written
/// out.
fn renamed(prelude: &idyllic_ast::Program, src: &str) -> String {
    let Some(p) = program(src) else { return "(error)".to_string() };
    let result = idyllic_rename::rename_module(&p, prelude);
    if !result.errors.is_empty() {
        return "(error)".to_string();
    }
    h::module(&result.module)
}

mod h {
    use idyllic_hir::*;

    std::thread_local! {
        /// the module's overloaded names, by node, while it is written out:
        /// each as front/'s HIR has it, `(overload x@u x@v)`
        static OVERLOADS: std::cell::RefCell<std::collections::HashMap<NodeId, Vec<usize>>> = Default::default();
    }

    /// The name at `node`, with every meaning it has there.
    fn ident_at(node: NodeId, x: &Ident) -> String {
        match OVERLOADS.with(|o| o.borrow().get(&node).cloned()) {
            Some(us) => sx(std::iter::once("overload".to_string()).chain(us.iter().map(|u| format!("{}@{u}", x.text)))),
            None => ident(x),
        }
    }

    fn sx<S: AsRef<str>>(parts: impl IntoIterator<Item = S>) -> String {
        let parts: Vec<String> = parts.into_iter().map(|s| s.as_ref().to_string()).collect();
        format!("({})", parts.join(" "))
    }

    fn opt<T>(m: &Option<T>, f: impl Fn(&T) -> String) -> String {
        m.as_ref().map_or("-".to_string(), f)
    }

    fn flag(yes: &str, b: bool) -> String {
        if b { yes } else { "-" }.to_string()
    }

    fn with<S: Into<String>>(head: impl IntoIterator<Item = S>, rest: Vec<String>) -> String {
        sx(head.into_iter().map(Into::into).chain(rest))
    }

    fn ident(x: &Ident) -> String {
        match x.unique {
            Some(u) => format!("{}@{u}", x.text),
            None => format!("{}@?", x.text),
        }
    }

    fn grade(g: &GradeAnn) -> String {
        sx(["grade".into(), term(&g.subject), opt(&g.in_type, |t| term(t))])
    }

    fn term(t: &Term) -> String {
        match &t.kind {
            TermKind::Type => "Type".into(),
            TermKind::Var(x) => ident_at(t.id, x),
            TermKind::Lit(l) => super::w::literal(l),
            TermKind::Pi(x, icit, g, dom, cod) => sx([
                "pi".into(),
                opt(x, ident),
                match icit {
                    Icit::Explicit => "e",
                    Icit::Implicit => "i",
                    Icit::Auto => "a",
                }
                .into(),
                opt(g, grade),
                term(dom),
                term(cod),
            ]),
            TermKind::Lam(x, g, ty, body) => sx(["lam".into(), ident(x), opt(g, grade), opt(ty, |t| term(t)), term(body)]),
            TermKind::App(f, a) => sx(["app".into(), term(f), term(a)]),
            TermKind::ImplicitApp(f, a) => sx(["iapp".into(), term(f), term(a)]),
            TermKind::AutoApp(f, a) => sx(["aapp".into(), term(f), term(a)]),
            TermKind::Search => "search".into(),
            TermKind::Ann(e, ty) => sx(["ann".into(), term(e), term(ty)]),
            TermKind::Hole => "_".into(),
            TermKind::Goal(n) => format!("?{n}"),
            TermKind::BoxIntro(e) => sx(["box".into(), term(e)]),
            TermKind::Inf => "inf".into(),
            TermKind::Range(a, b) => sx(["range".into(), term(a), term(b)]),
            TermKind::LetBox(x, e, body) => sx(["letbox".into(), ident(x), term(e), term(body)]),
            TermKind::Let(gs, body) => sx(["let".into(), sx(gs.iter().map(group)), term(body)]),
            TermKind::Case(e, cs) => with(["case".to_string(), term(e)], cs.iter().map(clause).collect()),
            TermKind::Match(cs) => with(["match"], cs.iter().map(clause).collect()),
            TermKind::Omitted => "?".into(),
            TermKind::Eff(b, e) => sx(["eff".into(), term(b), term(e)]),
            TermKind::Row(ls, tail) => sx(["row".into(), sx(ls.iter().map(term)), opt(tail, |t| term(t))]),
            TermKind::Handle(e, cs) => with(["handle".to_string(), term(e)], cs.iter().map(handle_clause).collect()),
            TermKind::SeqNil => "nil".into(),
            TermKind::SeqCons(h, tl) => sx(["cons".into(), term(h), term(tl)]),
            TermKind::ArrayLit(es) => with(["array"], es.iter().map(term).collect()),
        }
    }

    fn handle_clause(c: &HandleClause) -> String {
        sx(["clause".into(), opt(&c.op, ident), sx(c.params.iter().map(ident)), term(&c.body)])
    }

    fn clause(c: &Clause) -> String {
        sx(["clause".into(), sx(c.pats.iter().map(pattern)), rhs(&c.rhs)])
    }

    fn rhs(r: &Rhs) -> String {
        match r {
            Rhs::Plain(e) => sx(["=".into(), term(e)]),
            Rhs::Guarded(gs) => with(["guards"], gs.iter().map(|(c, e)| sx([term(c), term(e)])).collect()),
            Rhs::Impossible => "impossible".into(),
        }
    }

    fn pattern(p: &Pattern) -> String {
        match &p.kind {
            PatKind::Var(x) => ident(x),
            PatKind::Wild => "_".into(),
            PatKind::Lit(l) => super::w::literal(l),
            PatKind::Con(c, ps) => with(["con".to_string(), ident_at(p.id, c)], ps.iter().map(pattern).collect()),
            PatKind::As(x, q) => sx(["as".into(), ident(x), pattern(q)]),
            PatKind::Dot(e) => sx(["dot".into(), term(e)]),
            PatKind::SeqNil => "pnil".into(),
            PatKind::SeqCons(ps) => with(["pcons"], ps.iter().map(pattern).collect()),
            PatKind::Implicit(icit, q) => sx(["pimplicit".into(), super::w::icit(*icit).into(), pattern(q)]),
        }
    }

    fn bind(b: &Bind, terminating: &std::collections::HashSet<usize>, hints: &std::collections::HashSet<usize>) -> String {
        let vouched = b.name.unique.is_some_and(|u| terminating.contains(&u));
        let hinted = b.name.unique.is_some_and(|u| hints.contains(&u));
        sx([
            "bind".into(),
            ident(&b.name),
            opt(&b.ty, term),
            term(&b.rhs),
            flag("linear", b.linear),
            flag("terminating", vouched),
            flag("hint", hinted),
        ])
    }

    fn group(g: &BindGroup) -> String {
        let none = Default::default();
        match g {
            BindGroup::NonRec(b) => sx(["nonrec".into(), bind(b, &none, &none)]),
            BindGroup::Rec(bs) => with(["rec"], bs.iter().map(|b| bind(b, &none, &none)).collect()),
        }
    }

    fn type_param((x, k): &(Ident, Option<Term>)) -> String {
        sx([ident(x), opt(k, term)])
    }

    fn data(d: &DataDecl) -> String {
        let ctor = |c: &CtorDecl| match &c.shape {
            CtorShape::Fields(ts) => sx([ident(&c.name), with(["fields"], ts.iter().map(term).collect())]),
            CtorShape::Sig(ty) => sx([ident(&c.name), sx(["sig".into(), term(ty)])]),
        };
        sx([
            "data".into(),
            ident(&d.name),
            sx(d.params.iter().map(type_param)),
            opt(&d.kind, term),
            sx(d.ctors.iter().map(ctor)),
            opt(&d.seq, |(n, c)| sx(["seq".into(), ident(n), ident(c)])),
            flag("linear", d.linear),
            flag("trait", d.is_trait),
        ])
    }

    /// A trait's superclass projections and its methods, in their fields'
    /// order: each method with its field and how many parameters the trait
    /// has.
    fn trait_line(m: &Module, d: &DataDecl) -> String {
        let u = d.name.unique.unwrap_or(usize::MAX);
        let supers = m.supers.get(&u).cloned().unwrap_or_default();
        let mut methods: Vec<(usize, (usize, usize, usize))> = m.methods.iter().filter(|(_, (t, ..))| *t == u).map(|(m, i)| (*m, *i)).collect();
        methods.sort_by_key(|(_, (_, i, _))| *i);
        sx([
            "trait".into(),
            ident(&d.name),
            sx(supers.iter().map(|p| format!("super@{p}"))),
            sx(methods.iter().map(|(mu, (_, i, n))| sx([format!("method@{mu}"), i.to_string(), n.to_string()]))),
        ])
    }

    fn effect(e: &EffectDecl) -> String {
        let op = |o: &OpDecl| sx(["op".into(), ident(&o.name), flag("many", o.many), term(&o.ty)]);
        sx(["effect".into(), ident(&e.name), sx(e.params.iter().map(type_param)), sx(e.ops.iter().map(op))])
    }

    /// The program's own part of a module, a line each: data types, effects,
    /// natives, bindings, then the body, `main` and whether it is linear.
    pub fn module(m: &Module) -> String {
        OVERLOADS.with(|o| *o.borrow_mut() = m.overloads.clone());
        let own = |x: &Ident| x.unique.is_some_and(|u| u >= m.uniques.0);
        let mut lines: Vec<String> = Vec::new();
        lines.extend(m.datas.iter().filter(|d| own(&d.name)).map(data));
        lines.extend(m.datas.iter().filter(|d| own(&d.name) && d.is_trait).map(|d| trait_line(m, d)));
        lines.extend(m.effects.iter().filter(|e| own(&e.name)).map(effect));
        lines.extend(m.natives.iter().filter(|n| own(&n.name)).map(|n| sx(["native".into(), ident(&n.name), term(&n.ty)])));
        let binds = m.groups.iter().flat_map(|g| g.binds());
        lines.extend(binds.filter(|b| own(&b.name)).map(|b| bind(b, &m.terminating, &m.hints)));
        lines.extend(m.body.iter().map(|e| sx(["body".into(), term(e)])));
        lines.extend(m.main.iter().map(|u| sx(["main".to_string(), format!("main@{u}")])));
        lines.push(sx(["linear", if m.linear_body { "true" } else { "false" }]));
        lines.join("\n")
    }
}

// --- the parse, written out ---------------------------------------------------
//
// As `front/src/View.mw` writes it: an S-expression a line for each part of the
// program, nothing about where anything was written; `(error)` for a program
// the lexer, the layout or the parser found anything wrong with.

/// What this compiler's parser makes of `src`, written out.
fn parsed(src: &str) -> String {
    let lexed = idyllic_lexer::tokenize(src);
    let laid = idyllic_layout::layout(&LineIndex::new(src), &lexed.tokens);
    if !lexed.diagnostics.is_empty() || !laid.errors.is_empty() {
        return "(error)".to_string();
    }
    match idyllic_parser::parse(src.len(), &laid.tokens) {
        Ok(p) => w::program(&p),
        Err(_) => "(error)".to_string(),
    }
}

mod w {
    use idyllic_ast::*;

    fn sx<S: AsRef<str>>(parts: impl IntoIterator<Item = S>) -> String {
        let parts: Vec<String> = parts.into_iter().map(|s| s.as_ref().to_string()).collect();
        format!("({})", parts.join(" "))
    }

    fn opt<T>(m: &Option<T>, f: impl Fn(&T) -> String) -> String {
        m.as_ref().map_or("-".to_string(), f)
    }

    fn flag(yes: &str, b: bool) -> String {
        if b { yes } else { "-" }.to_string()
    }

    /// In quotes, with `\`, `"` and the control characters escaped; any other
    /// byte is itself.
    fn quoted(s: &str) -> String {
        let mut out = vec![b'"'];
        for &b in s.as_bytes() {
            match b {
                b'\\' => out.extend_from_slice(b"\\\\"),
                b'"' => out.extend_from_slice(b"\\\""),
                b'\n' => out.extend_from_slice(b"\\n"),
                b'\t' => out.extend_from_slice(b"\\t"),
                b'\r' => out.extend_from_slice(b"\\r"),
                b if b < 32 || b == 127 => out.extend_from_slice(format!("\\u{{{b:x}}}").as_bytes()),
                b => out.push(b),
            }
        }
        out.push(b'"');
        String::from_utf8(out).expect("escaping keeps UTF-8 whole")
    }

    pub fn literal(l: &Lit) -> String {
        match l {
            Lit::Unit => "(lit ())".into(),
            Lit::Int(n) => format!("(lit {n})"),
            Lit::Bool(b) => format!("(lit {})", if *b { "True" } else { "False" }),
            Lit::Str(s) => format!("(lit {})", quoted(s)),
            Lit::Char(c) => format!("(lit {})", idyllic_ast::show_char(*c)),
            Lit::Float(_) | Lit::Float32(_) | Lit::Word(..) => format!("(lit {l})"),
        }
    }

    fn grade(g: &GradeAnn) -> String {
        sx(["grade".into(), term(&g.subject), opt(&g.in_type, |t| term(t))])
    }

    fn param(p: &Param) -> String {
        sx(["param".into(), p.name.node.clone(), flag("unbox", p.unbox), opt(&p.grade, grade), opt(&p.ty, term)])
    }

    fn fields<T>(fs: &[(Ident, T)], f: impl Fn(&T) -> String) -> Vec<String> {
        fs.iter().map(|(x, e)| sx([x.node.clone(), f(e)])).collect()
    }

    fn with<S: Into<String>>(head: impl IntoIterator<Item = S>, rest: Vec<String>) -> String {
        sx(head.into_iter().map(Into::into).chain(rest))
    }

    pub fn term(t: &LTerm) -> String {
        match &t.node {
            Term::Type => "Type".into(),
            Term::Var(x) => x.node.clone(),
            Term::Lit(l) => literal(l),
            Term::Pi(x, icit, g, dom, cod) => sx([
                "pi".into(),
                opt(x, |x| x.node.clone()),
                match icit {
                    Icit::Explicit => "e",
                    Icit::Implicit => "i",
                    Icit::Auto => "a",
                }
                .into(),
                opt(g, grade),
                term(dom),
                term(cod),
            ]),
            Term::Lam(p, body) => sx(["lam".into(), param(&p.node), term(body)]),
            Term::App(f, a) => sx(["app".into(), term(f), term(a)]),
            Term::ImplicitApp(f, a) => sx(["iapp".into(), term(f), term(a)]),
            Term::AutoApp(f, a) => sx(["aapp".into(), term(f), term(a)]),
            Term::Resolved(u, x) => format!("{x}@{u}"),
            Term::RecordOf(r, args, fields, me) => with(
                ["recordof".to_string(), r.node.clone(), sx(args.iter().map(term)), me.node.clone()],
                fields.iter().map(|(f, e)| sx([f.node.clone(), term(e)])).collect(),
            ),
            Term::Search => "search".into(),
            Term::Equations(eqs) => with(["equations"], eqs.iter().map(|(ps, r)| sx([sx(ps.iter().map(pat)), rhs(r)])).collect()),
            Term::Ann(e, ty) => sx(["ann".into(), term(e), term(ty)]),
            Term::Hole => "_".into(),
            Term::Goal(n) => format!("?{n}"),
            Term::BoxIntro(e) => sx(["box".into(), term(e)]),
            Term::Inf => "inf".into(),
            Term::Range(a, b) => sx(["range".into(), term(a), term(b)]),
            Term::LetBox(x, e, body) => sx(["letbox".into(), x.node.clone(), term(e), term(body)]),
            Term::Let(ds, body) => sx(["let".into(), sx(ds.iter().map(decl)), term(body)]),
            Term::Case(e, alts) => with(["case".to_string(), term(e)], alts.iter().map(alt).collect()),
            Term::Eff(b, e) => sx(["eff".into(), term(b), term(e)]),
            Term::Row(ls, tail) => sx(["row".into(), sx(ls.iter().map(term)), opt(tail, |t| term(t))]),
            Term::Handle(e, cs) => with(["handle".to_string(), term(e)], cs.iter().map(clause).collect()),
            Term::Seq(es) => with(["seq"], es.iter().map(term).collect()),
            Term::ArrayLit(es) => with(["array"], es.iter().map(term).collect()),
            Term::RecordLit(r, fs) => with(["record".to_string(), r.node.clone()], fields(fs, term)),
            Term::Proj(e, f) => sx(["proj".into(), term(e), f.node.clone()]),
            Term::Update(e, fs) => with(["update".to_string(), term(e)], fields(fs, term)),
        }
    }

    fn pat(p: &LPat) -> String {
        match &p.node {
            Pat::Bind(x) => param(x),
            Pat::Wild => "_".into(),
            Pat::Lit(l) => literal(l),
            Pat::Con(c, ps) => with(["con".to_string(), c.node.clone()], ps.iter().map(pat).collect()),
            Pat::As(x, q) => sx(["as".into(), x.node.clone(), pat(q)]),
            Pat::Dot(e) => sx(["dot".into(), term(e)]),
            Pat::Seq(ps) => with(["pseq"], ps.iter().map(pat).collect()),
            Pat::Record(r, fs) => with(["precord".to_string(), r.node.clone()], fields(fs, pat)),
            Pat::Implicit(i, q) => sx(["pimplicit".into(), icit(*i).into(), pat(q)]),
        }
    }

    pub(super) fn icit(icit: Icit) -> &'static str {
        match icit {
            Icit::Explicit => "e",
            Icit::Implicit => "i",
            Icit::Auto => "a",
        }
    }

    fn rhs(r: &Rhs) -> String {
        match r {
            Rhs::Plain(e) => sx(["=".into(), term(e)]),
            Rhs::Guarded(gs) => with(["guards"], gs.iter().map(|(c, e)| sx([term(c), term(e)])).collect()),
            Rhs::Impossible => "impossible".into(),
        }
    }

    fn alt(a: &Alt) -> String {
        sx(["alt".into(), pat(&a.pat), rhs(&a.rhs)])
    }

    fn clause(c: &HandleClause) -> String {
        sx(["clause".into(), c.op.node.clone(), sx(c.params.iter().map(|x| x.node.clone())), term(&c.body)])
    }

    fn decl(d: &LDecl) -> String {
        match &d.node {
            Decl::Sig(f, ty) => sx(["sig".into(), f.node.clone(), term(ty)]),
            Decl::Def(f, ps, ty, r) => {
                sx(["def".into(), f.node.clone(), sx(ps.iter().map(pat)), opt(ty, term), rhs(r)])
            }
            Decl::Native(f, ty) => sx(["native".into(), f.node.clone(), term(ty)]),
        }
    }

    fn visibility(v: Visibility) -> String {
        match v {
            Visibility::Private => "private",
            Visibility::Super => "super",
            Visibility::Package => "package",
            Visibility::Public => "public",
        }
        .into()
    }

    fn type_param((x, k): &(Ident, Option<LTerm>)) -> String {
        sx([x.node.clone(), opt(k, term)])
    }

    fn data(d: &Data) -> String {
        let ctor = |(name, c): &(Ident, Ctor)| match c {
            Ctor::Fields(ts) => sx([name.node.clone(), with(["fields"], ts.iter().map(term).collect())]),
            Ctor::Sig(ty) => sx([name.node.clone(), sx(["sig".into(), term(ty)])]),
        };
        sx([
            "data".into(),
            d.name.node.clone(),
            sx(d.params.iter().map(type_param)),
            opt(&d.kind, term),
            sx(d.ctors.iter().map(ctor)),
            opt(&d.seq, |s| sx(["seq".into(), s.nil.node.clone(), s.cons.node.clone()])),
            visibility(d.visibility),
            opt(&d.fields, |fs| sx(fs.iter().map(|f| f.node.clone()))),
            flag("trait", d.is_trait),
            sx(d.defaults.iter().map(|(m, name, function)| sx([m.node.clone(), name.clone(), flag("function", *function)]))),
        ])
    }

    fn effect(e: &EffectDecl) -> String {
        let op = |o: &OpSig| sx(["op".into(), o.name.node.clone(), flag("many", o.many), term(&o.ty)]);
        sx([
            "effect".into(),
            e.name.node.clone(),
            sx(e.params.iter().map(type_param)),
            sx(e.ops.iter().map(op)),
            visibility(e.visibility),
        ])
    }

    fn use_decl(u: &Use) -> String {
        sx([
            "use".into(),
            flag("pub", u.public),
            sx(u.path.iter().map(|x| x.node.clone())),
            flag("glob", u.glob),
            opt(&u.alias, |a| a.node.clone()),
            opt(&u.names, |ns| sx(ns.iter().map(|x| x.node.clone()))),
        ])
    }

    /// A line for each part: whether it is linear, then its uses, data types,
    /// effects, declarations, visibilities, `@terminating`s and body.
    pub fn program(p: &Program) -> String {
        let mut lines = vec![sx(["linear", if p.linear { "true" } else { "false" }])];
        lines.extend(p.uses.iter().map(|u| use_decl(&u.node)));
        lines.extend(p.datas.iter().map(|d| data(&d.node)));
        lines.extend(p.effects.iter().map(|e| effect(&e.node)));
        lines.extend(p.decls.iter().map(decl));
        lines.extend(p.visibility.iter().map(|(x, v)| sx(["vis".into(), x.node.clone(), visibility(*v)])));
        lines.extend(p.terminating.iter().map(|x| sx(["terminating".into(), x.node.clone()])));
        lines.extend(p.hints.iter().map(|x| sx(["hint".into(), x.node.clone()])));
        lines.extend(p.body.iter().map(|e| sx(["body".into(), term(e)])));
        // a module inside the file, its own program indented under it
        for (name, inner) in &p.modules {
            lines.push(sx(["module".into(), name.node.clone()]));
            lines.extend(program(inner).lines().map(|l| format!("  {l}")));
        }
        lines.join("\n")
    }
}
