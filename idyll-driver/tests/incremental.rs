//! Incremental compilation: a package is compiled again exactly when
//! something it was compiled from changed, and what is read back is what
//! compiling would have made.

mod common;

use common::write;
use idyll::Profile;
use idyll::pipeline::{self, BuildOutput};
use std::path::{Path, PathBuf};

fn scratch(who: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idyll-incr-{}-{who}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

fn append(path: &Path, text: &str) {
    let old = std::fs::read_to_string(path).unwrap();
    std::fs::write(path, old + text).unwrap();
}

/// What `App` prints: `double 21`, and `label 5`.
const RAN: &str = "42\n52\n";

/// A workspace of `App`, which uses `Text`, which uses `Util`, and `Other`,
/// which uses nothing.
fn workspace(who: &str) -> PathBuf {
    let root = scratch(who).join("ws");
    write(
        &root.join("Idyll.toml"),
        "[workspace]\nmembers = [\"App\", \"libs/*\"]\n\n[workspace.dependencies]\n\
         Util = { path = \"libs/Util\" }\nText = { path = \"libs/Text\" }\n",
    );
    for (name, deps, src) in [
        (
            "libs/Util",
            "",
            "@pub double : Int -> Int\ndouble x = x * 2\n\n\
             @pub data Shape = Circle Int | Square Int\n\n\
             @pub area : Shape -> Int\narea s = case s of\n  Shape.Circle r -> 3 * r\n  Shape.Square n -> 4 * n\n",
        ),
        (
            "libs/Text",
            "Util = { workspace = true }\n",
            "use Util (double, Shape, area)\n\n@pub label : Int -> Int\nlabel s = s * 10 + double 1\n\n\
             @pub unit : Int\nunit = area (Shape.Square 1)\n",
        ),
        ("libs/Other", "", "@pub triple : Int -> Int\ntriple x = x * 3\n"),
        (
            "App",
            "Util = { workspace = true }\nText = { workspace = true }\n",
            "use Util (double)\nuse Text (label)\n\nmain : () -> () ! Eff\nmain () = let u = printInt (double 21) in printInt (label 5)\n",
        ),
    ] {
        let short = name.rsplit('/').next().unwrap();
        write(
            &root.join(name).join("Idyll.toml"),
            &format!("[package]\nname = \"{}\"\n\n[dependencies]\n{deps}", idyll::package::as_package_name(short)),
        );
        let file = if name == "App" { "Main.idl" } else { "Lib.idl" };
        write(&root.join(name).join("src").join(file), src);
    }
    root
}

fn members(root: &Path) -> Vec<PathBuf> {
    ["App", "libs/Other"].iter().map(|m| root.join(m)).collect()
}

/// Build `App` and `Other` together, answering what was compiled.
fn build(root: &Path, profile: Profile) -> (Vec<String>, pipeline::ManyOutput) {
    let paths = members(root);
    let refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
    let out = pipeline::build_each(&refs, profile, &common::OPTS);
    let said: Vec<String> =
        out.problem.iter().map(|p| p.msg.clone()).chain(out.reported.iter().flat_map(common::messages)).collect();
    assert!(said.is_empty(), "{said:?}");
    (out.compiled.clone(), out)
}

/// Everything a build says about a program: each of its packages as it is
/// kept, and what running it prints.
fn everything(out: &BuildOutput) -> (Vec<Vec<u8>>, String) {
    let packages = out.program.iter().map(|p| postcard::to_stdvec(p).unwrap()).collect();
    let printed = if out.program.last().is_some_and(|p| p.main) { common::printed(out) } else { String::new() };
    (packages, printed)
}

#[test]
fn nothing_changed_compiles_nothing_and_builds_the_same() {
    let root = workspace("same");
    let profile = Profile::Debug;
    let (first, fresh) = build(&root, profile);
    assert_eq!(first, ["Util", "Text", "App", "Other"]);
    let (second, reused) = build(&root, profile);
    assert!(second.is_empty(), "{second:?}");
    for (a, b) in fresh.each.iter().zip(&reused.each) {
        assert_eq!(everything(a), everything(b));
    }
    assert_eq!(everything(&reused.each[0]).1, RAN, "and it runs");
}

/// `package` as it is kept, with its tables -- what is known about each unique,
/// listed in no order anything reads them by -- each in the order of its
/// uniques.
///
/// The orders do come from hash maps, for now: the renamer
/// (`idyllic-rename`) lists a module's exports out of one, and the checker
/// (`idyllic-typecheck`) its constructors, so two compiles of one package
/// list them differently. Dependents read them into maps again, so the
/// lists are compared as the maps they are; everything else -- the code, its
/// uniques, its types -- has to be the same byte for byte.
fn as_tables(package: &idyllic::pipeline::CompiledPackage) -> Vec<u8> {
    let mut p = package.clone();
    p.names.exports.sort_by(|a, b| (&a.0, &a.1, a.2).cmp(&(&b.0, &b.1, b.2)));
    p.ctors.sort_by_key(|(u, _)| *u);
    p.datas.sort_by_key(|(u, _)| *u);
    p.positive_params.sort_by_key(|(u, _)| *u);
    p.seqs.sort_by_key(|(u, _)| *u);
    p.effects.sort_by_key(|(u, _)| *u);
    p.ops.sort_by_key(|(u, _)| *u);
    postcard::to_stdvec(&p).unwrap()
}

/// Reusing a package trusts that its dependency, compiled again from the same
/// inputs, mints the same uniques and says the same things: a saved `App`
/// refers to `Util`'s definitions by number. Every hash map in a process is
/// seeded differently, so two compiles in one process disagree wherever an
/// order comes from one.
#[test]
fn compiling_the_same_package_twice_gives_the_same_package() {
    let root = workspace("twice");
    let describe = || {
        // With the cache gone, both are compiled.
        let _ = std::fs::remove_dir_all(root.join("target"));
        let (compiled, out) = build(&root, Profile::Debug);
        assert_eq!(compiled.len(), 4, "{compiled:?}");
        out.each.iter().flat_map(|b| b.program.iter()).map(|p| (p.name.clone(), as_tables(p))).collect::<Vec<_>>()
    };
    let (first, second) = (describe(), describe());
    assert_eq!(first.len(), second.len());
    for ((name, a), (_, b)) in first.iter().zip(&second) {
        assert!(a == b, "{name} differs between two compiles");
    }
}

#[test]
fn a_change_recompiles_the_package_and_what_depends_on_it() {
    let root = workspace("downstream");
    let profile = Profile::Debug;
    build(&root, profile);

    append(&root.join("App/src/Main.idl"), "\n-- a comment\n");
    assert_eq!(build(&root, profile).0, ["App"]);

    append(&root.join("libs/Text/src/Lib.idl"), "\n@pub more : Int\nmore = 1\n");
    assert_eq!(build(&root, profile).0, ["Text", "App"]);

    append(&root.join("libs/Util/src/Lib.idl"), "\n-- Util\n");
    assert_eq!(build(&root, profile).0, ["Util", "Text", "App"]);

    append(&root.join("libs/Other/src/Lib.idl"), "\n-- Other\n");
    assert_eq!(build(&root, profile).0, ["Other"]);
    assert!(build(&root, profile).0.is_empty());
}

/// `Other` is compiled after `App`'s libraries, so its uniques start above
/// theirs. A library growing must not move them, or every edit anywhere would
/// be a change to everything built after it.
#[test]
fn a_package_growing_does_not_recompile_the_packages_beside_it() {
    let root = workspace("grow");
    let profile = Profile::Debug;
    build(&root, profile);
    let many: String = (0..200).map(|i| format!("@pub f{i} : Int -> Int\nf{i} x = x + {i}\n")).collect();
    append(&root.join("libs/Util/src/Lib.idl"), &many);
    // (Two hundred definitions take more stack to check than a test thread
    // has: the `idyll` command runs on a thread of its own for that.)
    let (compiled, ran) = common::on_big_stack(|| {
        let (compiled, out) = build(&root, profile);
        (compiled, everything(&out.each[0]).1)
    });
    assert_eq!(compiled, ["Util", "Text", "App"]);
    assert_eq!(ran, RAN);
}

/// Meadow keeps a package per set of options (a `cfg` flag is a change).
/// Idyll's checking depends on nothing but the sources, so what is kept
/// apart is the profile's: each has a `target/<profile>` of its own.
#[test]
fn different_profiles_keep_their_own_packages() {
    let root = workspace("options");
    assert_eq!(build(&root, Profile::Debug).0.len(), 4);
    assert_eq!(build(&root, Profile::Release).0.len(), 4, "a profile is a cache of its own");
    assert!(build(&root, Profile::Debug).0.is_empty(), "and each is kept");
    assert!(build(&root, Profile::Release).0.is_empty());
    assert!(root.join("target/debug/incremental").is_dir());
    assert!(root.join("target/release/incremental").is_dir());
}

#[test]
fn a_package_with_errors_is_compiled_every_time() {
    let root = workspace("errors");
    let profile = Profile::Debug;
    build(&root, profile);
    append(&root.join("libs/Text/src/Lib.idl"), "\nbroken : Int\nbroken = 1 + \"one\"\n");
    let paths = members(&root);
    let refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
    for _ in 0..2 {
        let out = pipeline::build_each(&refs, profile, &common::OPTS);
        assert!(out.errors() > 0, "the error is reported again");
        // (Meadow compiles `App` against the broken `Text` too; Idyll does not
        // compile what depends on a package that failed, since it has nothing
        // to check it against.)
        assert_eq!(out.compiled, ["Text"]);
        assert!(out.each[0].program.is_empty(), "nothing of `App` to run");
        assert!(!out.each[1].program.is_empty(), "but `Other` is untouched");
    }
}

#[test]
fn a_damaged_cache_is_compiled_over() {
    let root = workspace("damaged");
    let profile = Profile::Debug;
    build(&root, profile);
    let dir = root.join("target/debug/incremental");
    let mut files = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.starts_with("Util@") {
            // What this compiler writes in front of a package -- its magic and
            // the fingerprint it was saved with -- and then rubbish: the file
            // is damaged, not somebody else's, or out of date.
            let mut bytes = std::fs::read(&path).unwrap();
            bytes.truncate(16);
            bytes.extend_from_slice(b" not a package");
            std::fs::write(&path, bytes).unwrap();
            files += 1;
        }
    }
    assert_eq!(files, 1);
    let (compiled, out) = build(&root, profile);
    assert_eq!(compiled, ["Util"], "its dependents' inputs did not change");
    assert_eq!(everything(&out.each[0]).1, RAN);
}

#[test]
fn renaming_a_package_is_a_change() {
    let root = workspace("rename");
    let profile = Profile::Debug;
    build(&root, profile);
    write(&root.join("libs/Other/Idyll.toml"), "[package]\nname = \"Another\"\n");
    assert_eq!(build(&root, profile).0, ["Another"]);
}

#[test]
fn the_cache_can_be_turned_off() {
    let root = workspace("off");
    let run = |incremental: &str| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_idyll"))
            .current_dir(&root)
            .args(["run", "-p", "App"])
            .env("IDYLL_INCREMENTAL", incremental)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n")
    };
    let dir = root.join("target/debug/incremental");
    assert!(run("0").contains(RAN));
    assert!(!dir.exists(), "nothing is written");
    assert!(run("1").contains(RAN));
    // `Util`, `Text` and `App`: the prelude is checked with every package,
    // and is not kept.
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
    assert!(run("1").contains(RAN), "and read back");
}

/// Every file under `dir` whose name has `part` in it.
fn files_with(dir: &Path, part: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(files_with(&p, part));
        } else if p.file_name().is_some_and(|n| n.to_string_lossy().contains(part)) {
            out.push(p);
        }
    }
    out
}

#[test]
fn a_package_of_several_parts_keeps_each_and_rebuilds_from_them() {
    let root = scratch("parts").join("Mods");
    write(&root.join("Idyll.toml"), "[package]\nname = \"Mods\"\n");
    write(&root.join("src/Main.idl"), "use Even (even)\nuse Words (word)\n\nmain : () -> () ! {Eff, Div}\nmain () = putStrLn \"${even 4} ${word}\"\n");
    write(&root.join("src/Even.idl"), "use Odd (odd)\n\n@pub\neven : Int -> Bool ! Div\neven 0 = True\neven n = odd (n - 1)\n");
    write(&root.join("src/Odd.idl"), "use Even (even)\n\n@pub\nodd : Int -> Bool ! Div\nodd 0 = False\nodd n = even (n - 1)\n");
    write(&root.join("src/Words.idl"), "@pub\nword : String\nword = \"one\"\n");
    let run = || {
        let out = pipeline::build_each(&[root.as_path()], Profile::Debug, &common::OPTS);
        let said: Vec<String> = out.problem.iter().map(|p| p.msg.clone()).chain(out.reported.iter().flat_map(common::messages)).collect();
        assert!(said.is_empty(), "{said:?}");
        common::printed(&out.each[0])
    };
    assert_eq!(run(), "True one\n");
    // the parts that are not the root's: `Even` and `Odd` together, `Words`
    assert_eq!(files_with(&root.join("target"), ".part-").len(), 2);
    write(&root.join("src/Words.idl"), "@pub\nword : String\nword = \"two\"\n");
    assert_eq!(run(), "True two\n");
    assert_eq!(files_with(&root.join("target"), ".part-").len(), 2);
}
