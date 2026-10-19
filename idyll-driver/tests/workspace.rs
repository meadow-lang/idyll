//! Workspaces: which packages are members, what a command selects, and that
//! members share a `target`, their profiles and what they depend on.

mod common;

use common::{said, write};
use idyll::package::{Manifest, ProfileConfig};
use idyll::profile::{Backend, Profile, Resolved};
use idyll::workspace::{Selection, Workspace};
use idyll::{init, pipeline};
use std::path::{Path, PathBuf};

/// A directory of our own, named after the test so concurrent ones do not
/// collide.
fn scratch(who: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idyll-ws-{}-{who}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

/// What `app` prints: `double 21`, and `label 5`.
const RAN: &str = "42\n52\n";

/// `shop/`: a virtual workspace of `app`, which uses `text` and `util`, and
/// `text`, which uses `util` -- the libraries under `libs/*`, taking their
/// version and dependencies from the root, which also says how release
/// builds run.
fn shop(who: &str) -> PathBuf {
    let root = scratch(who).join("shop");
    write(
        &root.join("Idyll.toml"),
        r#"[workspace]
members = [
    "app",   # the program
    "libs/*",
]

[workspace.package]
version = "0.3.0"

[workspace.dependencies]
Util = { path = "libs/util" }
Text = { path = "libs/text" }

[profile.release]
backend = "glade"
"#,
    );
    write(&root.join("libs/util/Idyll.toml"), "[package]\nname = \"Util\"\nversion.workspace = true\n");
    write(&root.join("libs/util/src/Lib.idl"), "@pub double : Int -> Int\ndouble x = x * 2\n");
    write(
        &root.join("libs/text/Idyll.toml"),
        "[package]\nname = \"Text\"\nversion = { workspace = true }\n\n\
         [dependencies]\nUtil.workspace = true\n",
    );
    write(
        &root.join("libs/text/src/Lib.idl"),
        "use Util (double)\n\n@pub label : Int -> Int\nlabel s = s * 10 + double 1\n",
    );
    write(
        &root.join("app/Idyll.toml"),
        "[package]\nname = \"App\"\nversion.workspace = true\n\n[dependencies]\n\
         Util = { workspace = true }\nText = { workspace = true }\n",
    );
    write(
        &root.join("app/src/Main.idl"),
        "use Util (double)\nuse Text (label)\n\n\
         main : () -> () ! Eff\nmain () = let u = printInt (double 21) in printInt (label 5)\n",
    );
    root
}

fn names(ws: &Workspace) -> Vec<&str> {
    ws.members.iter().map(|m| m.name.as_str()).collect()
}

fn idyll(dir: &Path, args: &[&str]) -> (bool, String) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_idyll"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("the idyll binary runs");
    (
        out.status.success(),
        (String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr))
            .replace("\r\n", "\n"),
    )
}

#[test]
fn a_root_manifest_says_what_the_workspace_is() {
    let root = shop("manifest");
    let m = Manifest::load(&root).unwrap().unwrap();
    assert!(!m.is_package, "no [package]: a virtual manifest");
    let ws = m.workspace.expect("a [workspace]");
    assert_eq!(ws.members, ["app", "libs/*"], "an array over several lines");
    assert_eq!(ws.version.as_deref(), Some("0.3.0"));
    assert_eq!(ws.deps.len(), 2);

    // Both spellings of inheriting.
    let text = Manifest::load(&root.join("libs/text")).unwrap().unwrap();
    assert!(text.problems.is_empty(), "{:?}", text.problems);
    assert_eq!(text.version, "0.3.0");
    assert_eq!(text.deps.len(), 1);
    let idyll::package::DepSource::Path(p) = &text.deps[0].source else {
        panic!("a path dependency, got {:?}", text.deps[0].source);
    };
    assert_eq!(std::fs::canonicalize(p).unwrap(), root.join("libs/util"));
    let app = Manifest::load(&root.join("app")).unwrap().unwrap();
    assert_eq!(app.version, "0.3.0");
    assert_eq!(app.deps.len(), 2);
}

#[test]
fn members_come_from_patterns_and_path_dependencies() {
    let root = shop("members");
    // A library the app depends on, under the root but in no pattern.
    write(&root.join("vendor/extra/Idyll.toml"), "[package]\nname = \"Extra\"\n");
    write(&root.join("vendor/extra/src/Lib.idl"), "@pub one : Int\none = 1\n");
    write(
        &root.join("app/Idyll.toml"),
        "[package]\nname = \"App\"\n\n[dependencies]\nUtil = { workspace = true }\n\
         Text = { workspace = true }\nExtra = \"../vendor/extra\"\n",
    );
    // A directory a pattern matches that is not a package.
    std::fs::create_dir_all(root.join("libs/notes")).unwrap();

    let ws = Workspace::load(&root).unwrap();
    assert_eq!(names(&ws), ["App", "Text", "Util", "Extra"]);
    assert!(ws.default_members.is_empty());
}

#[test]
fn a_package_inside_a_workspace_must_be_a_member() {
    let root = shop("stray");
    write(&root.join("stray/Idyll.toml"), "[package]\nname = \"Stray\"\n");
    write(&root.join("stray/src/Main.idl"), "1\n");

    let err = Workspace::find(&root.join("stray")).unwrap_err();
    assert!(err.contains("not a member"), "{err}");
    let out = pipeline::build(&root.join("stray"), Profile::Debug, &common::OPTS);
    assert!(out.program.is_empty());
    assert!(said(&out)[0].contains("not a member"), "{:?}", said(&out));

    // Unless it is excluded, when it is a package of its own.
    let manifest = std::fs::read_to_string(root.join("Idyll.toml")).unwrap();
    write(
        &root.join("Idyll.toml"),
        &manifest.replace("[workspace.package]", "exclude = [\"stray\"]\n\n[workspace.package]"),
    );
    assert!(Workspace::find(&root.join("stray")).unwrap().is_none());
    let out = pipeline::build(&root.join("stray"), Profile::Debug, &common::OPTS);
    assert!(said(&out).is_empty(), "{:?}", said(&out));
    assert_eq!(out.package.unwrap().0, root.join("stray"));
}

#[test]
fn a_workspace_is_found_from_anywhere_inside_a_member() {
    let root = shop("find");
    for inside in
        [root.clone(), root.join("app"), root.join("app/src"), root.join("app/src/Main.idl"), root.join("libs/util")]
    {
        let ws = Workspace::find(&inside).unwrap().expect("found");
        assert_eq!(ws.root, root, "from {}", inside.display());
    }
    assert!(Workspace::find(root.parent().unwrap()).unwrap().is_none());
}

#[test]
fn what_a_command_selects() {
    let root = shop("select");
    let dirs = |sel: &Selection, at: &Path| -> Vec<PathBuf> { sel.select(at).unwrap().paths };

    // At a virtual root, everything; in a member, that member.
    assert_eq!(dirs(&Selection::default(), &root).len(), 3);
    assert_eq!(dirs(&Selection::default(), &root.join("app")), [root.join("app")]);

    // `-p` from anywhere, `--workspace` less `--exclude`.
    let p = Selection { packages: vec!["Text".into()], ..Selection::default() };
    assert_eq!(dirs(&p, &root.join("app/src")), [root.join("libs/text")]);
    let all_but_app = Selection { workspace: true, exclude: vec!["App".into()], ..Selection::default() };
    assert_eq!(dirs(&all_but_app, &root.join("app")), [root.join("libs/text"), root.join("libs/util")]);

    let unknown = Selection { packages: vec!["nope".into()], ..Selection::default() };
    let err = unknown.select(&root).unwrap_err();
    assert!(err.contains("no member `nope`") && err.contains("`App`"), "{err}");

    // `run` needs one.
    let err = Selection::default().select(&root).unwrap().one("run").unwrap_err();
    assert!(err.contains("-p NAME"), "{err}");

    // `default-members` is what the root means.
    let manifest = std::fs::read_to_string(root.join("Idyll.toml")).unwrap();
    write(
        &root.join("Idyll.toml"),
        &manifest.replace("[workspace.package]", "default-members = [\"app\"]\n\n[workspace.package]"),
    );
    assert_eq!(dirs(&Selection::default(), &root), [root.join("app")]);
}

#[test]
fn members_build_together_into_one_target() {
    let root = shop("build");
    let paths = [root.join("app"), root.join("libs/text")];
    let refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
    let profile = Resolved::resolve(Profile::Debug, &root, ProfileConfig::default()).profile;
    let out = pipeline::build_each(&refs, profile, &common::OPTS);
    assert!(out.errors() == 0 && out.reported.is_empty(), "{:?}", out.problem);
    assert_eq!(out.each.len(), 2);

    // (Meadow's root also set a `cfg` flag here, and saw it reach `util`,
    // which only `app` asked for; Idyll has no `cfg`.)
    assert_eq!(common::printed(&out.each[0]), RAN);
    for (built, name) in out.each.iter().zip(["App", "Text"]) {
        let (dir, pkg) = built.package.clone().unwrap();
        assert_eq!(dir, root, "{name} builds into the workspace's target");
        assert_eq!(pkg, name);
    }
    assert!(root.join("target/debug/incremental").is_dir());
    // `text` is linked with what it depends on and nothing else.
    let text = &out.each[1].program;
    assert!(text.iter().all(|p| p.name != "App"));
    assert_eq!(text.len(), 2, "Util and Text");
}

#[test]
fn every_member_builds_with_the_root_profiles() {
    let root = shop("profiles");
    write(
        &root.join("app/Idyll.toml"),
        "[package]\nname = \"App\"\n\n[dependencies]\nUtil = { workspace = true }\n\
         Text = { workspace = true }\n\n[profile.release]\nbackend = \"silo\"\n",
    );
    let resolved = Resolved::resolve(Profile::Release, &root.join("app"), ProfileConfig::default());
    assert_eq!(resolved.backend, Backend::Glade, "the root's");
    assert!(resolved.backend_named, "a member's own is ignored");

    // On Glade, as the root says, whatever the member says -- so no Silo
    // build is needed for this to run.
    let (ok, output) = idyll(&root, &["run", "-p", "App", "--release"]);
    assert!(ok, "{output}");
    assert!(output.contains(RAN), "{output}");
    assert!(output.contains("warning: the `[profile]` sections of `App` are ignored"), "{output}");
    assert!(root.join("target/release/incremental").is_dir());
    assert!(!root.join("app/target").exists());
}

#[test]
fn a_virtual_root_is_not_a_package() {
    let root = shop("virtual");
    let out = pipeline::build(&root, Profile::Debug, &common::OPTS);
    assert!(out.program.is_empty());
    let msg = &said(&out)[0];
    assert!(msg.contains("no package of its own"), "{msg}");
}

#[test]
fn inheriting_what_the_workspace_lacks_is_an_error() {
    let root = shop("inherit");
    write(&root.join("app/Idyll.toml"), "[package]\nname = \"App\"\n\n[dependencies]\nhttp = { workspace = true }\n");
    let out = pipeline::build(&root.join("app"), Profile::Debug, &common::OPTS);
    assert!(out.program.is_empty());
    let msg = &said(&out)[0];
    assert!(msg.contains("`http`") && msg.contains("[workspace.dependencies]"), "{msg}");

    let lone = scratch("inherit-lone").join("lone");
    write(&lone.join("Idyll.toml"), "[package]\nname = \"Lone\"\nversion.workspace = true\n");
    let m = Manifest::load(&lone).unwrap().unwrap();
    assert!(m.problems[0].contains("not in one"), "{:?}", m.problems);
}

#[test]
fn init_makes_a_workspace_and_members_join_it() {
    let dir = scratch("init");
    let root = dir.join("shop");
    let made = init::run(&init::Options { path: root.clone(), name: None, workspace: true }).unwrap();
    assert!(made.files.contains(&root.join(".gitignore")));

    let member = |path: PathBuf| init::run(&init::Options { path, name: None, workspace: false }).unwrap();
    let app = member(root.join("app"));
    assert!(app.joined.is_some());
    assert!(!root.join("app/.gitignore").exists(), "the workspace ignores target");
    member(root.join("libs").join("util"));
    assert_eq!(
        std::fs::read_to_string(root.join("Idyll.toml")).unwrap(),
        "[workspace]\nmembers = [\"app\", \"libs/util\"]\n"
    );

    // A pattern that already covers a new package leaves the manifest be.
    write(&root.join("Idyll.toml"), "[workspace]\nmembers = [\"app\", \"libs/*\"]\n");
    let text = member(root.join("libs").join("text"));
    assert!(text.joined.is_none());
    let ws = Workspace::load(&root).unwrap();
    assert_eq!(names(&ws), ["App", "Text", "Util"]);

    // No workspace inside a workspace, and no second member of a name.
    let err = init::run(&init::Options { path: root.join("inner"), name: None, workspace: true }).unwrap_err();
    assert!(err.contains("do not nest"), "{err}");
    let err =
        init::run(&init::Options { path: root.join("other"), name: Some("App".into()), workspace: false }).unwrap_err();
    assert!(err.contains("already has a member called `App`"), "{err}");

    // And what `init` made builds.
    let (ok, output) = idyll(&root, &["build", "--workspace"]);
    assert!(ok, "{output}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two packages beside each other, both depending on nothing, used to be
/// numbered from the same variable: linked into one program -- here by a
/// package depending on both -- one's definitions replaced the other's.
#[test]
fn packages_beside_each_other_keep_their_own_definitions() {
    let dir = scratch("siblings");
    for (name, body) in [
        ("a", "@pub fa : Int -> Int\nfa x = x + 1\n\n@pub va : Int\nva = fa 10\n"),
        ("b", "@pub fb : Int -> Int\nfb x = x * 100\n\n@pub vb : Int\nvb = fb 3\n"),
    ] {
        write(
            &dir.join(name).join("Idyll.toml"),
            &format!("[package]\nname = \"{}\"\n", idyll::package::as_package_name(name)),
        );
        write(&dir.join(name).join("src/Lib.idl"), body);
    }
    // (`A` and `B` rather than Meadow's `a` and `b`: a `use` path is
    // PascalCase from its first segment.)
    write(
        &dir.join("app/Idyll.toml"),
        "[package]\nname = \"App\"\n\n[dependencies]\nA = { path = \"../a\" }\nB = { path = \"../b\" }\n",
    );
    write(
        &dir.join("app/src/Main.idl"),
        "use A (fa, va)\nuse B (fb, vb)\n\n\
         main : () -> () ! Eff\n\
         main () = let u = printInt (fa 1) in let v = printInt va in let w = printInt (fb 2) in printInt vb\n",
    );
    let out = pipeline::build(&dir.join("app"), Profile::Debug, &common::OPTS);
    assert!(said(&out).is_empty(), "{:?}", said(&out));
    assert_eq!(common::printed(&out), "2\n11\n200\n300\n");
    let _ = std::fs::remove_dir_all(&dir);
}
