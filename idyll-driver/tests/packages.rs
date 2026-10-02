//! Multi-package builds: `Idyll.toml` manifests, a local path dependency, and
//! `@pub` gating between packages.

mod common;

use common::said;
use idyll::package::{DepSource, Manifest};
use idyll::{Profile, pipeline};
use std::path::{Path, PathBuf};

/// A directory of this test's own, holding the fixtures, emptied first.
fn scratch(who: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idyll-pkg-{}-{who}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    common::fixtures(&dir);
    dir
}

#[test]
fn manifest_parses_cargo_style() {
    let dir = scratch("manifest");
    let m = Manifest::load(&common::fixture(&dir, "App")).unwrap().expect("app has a manifest");
    assert_eq!(m.name, "App");
    assert_eq!(m.version, "0.1.0");
    assert_eq!(m.deps.len(), 1);
    assert_eq!(m.deps[0].name, "Util");
    assert_eq!(m.deps[0].source, DepSource::Path(PathBuf::from("../Util")));
}

#[test]
fn builds_app_against_a_path_dependency() {
    let dir = scratch("path-dep");
    let out = pipeline::build(&common::fixture(&dir, "App"), Profile::Debug, &common::OPTS);
    assert!(said(&out).is_empty(), "unexpected diagnostics: {:?}", said(&out));
    // [1..5] -> double -> *3 -> sum  ==  (2+4+6+8+10)*3 == 90
    assert_eq!(common::value(&out), "90");
}

#[test]
fn every_package_has_std_without_asking_for_it() {
    // no `[dependencies]` line names it, and the package builds and runs
    // against it, on the machine as compiled from what the build kept
    let dir = scratch("std");
    let app = common::fixture(&dir, "App");
    common::write(&app.join("src/Main.idl"), "use Std.List (sum, reverse)
use Std.Nat (fromInt)

sum (reverse (Std.List.range 1 (fromInt 4)))
");
    let out = pipeline::build(&app, Profile::Debug, &common::OPTS);
    assert!(said(&out).is_empty(), "{:?}", said(&out));
    assert_eq!(common::value(&out), "10");
}

#[test]
fn private_names_do_not_cross_package_boundaries() {
    // `Util` exports `double`, `map`, `scale` and `sum` (all `@pub`) but not
    // `secret`.
    let dir = scratch("private");
    let out = pipeline::build(&common::fixture(&dir, "Util"), Profile::Debug, &common::OPTS);
    assert!(said(&out).is_empty(), "{:?}", said(&out));
    let util = out.program.last().unwrap();
    let mut names: Vec<_> = util
        .names
        .exports
        .iter()
        .filter(|(_, _, _, vis)| format!("{vis:?}") == "Public")
        .map(|(_, name, _, _)| name.clone())
        .collect();
    names.sort();
    assert_eq!(names, vec!["double", "map", "scale", "sum"]);

    // And a dependent that asks for it anyway is told why not.
    let app = common::fixture(&dir, "App");
    common::write(&app.join("src/Main.idl"), "use Util (secret)\n\nsecret 1\n");
    let out = pipeline::build(&app, Profile::Debug, &common::OPTS);
    assert_eq!(said(&out), ["secret is private to Util: used here"]);
}

/// A package that never mentions visibility has none: every module sees all
/// of every other. Unlike Meadow, where such a package also exports every
/// binding, that is as far as it goes in Idyll: another package sees only
/// what is `@pub`.
#[test]
fn without_pub_everything_is_visible_inside_the_package() {
    let dir = scratch("no-pub");
    let plain = dir.join("Plain");
    common::write(&plain.join("Idyll.toml"), "[package]\nname = \"Plain\"\n");
    common::write(
        &plain.join("src/Helpers.idl"),
        "a : Int -> Int\na x = x\n\nb : Int -> Int\nb y = y\n\nc : Int\nc = 1\n",
    );
    common::write(&plain.join("src/Lib.idl"), "use Helpers (a, b, c)\n\nd : Int\nd = a (b c)\n\nd\n");
    let out = pipeline::build(&plain, Profile::Debug, &common::OPTS);
    assert!(said(&out).is_empty(), "{:?}", said(&out));
    assert_eq!(common::value(&out), "1");

    let app = dir.join("User");
    common::write(
        &app.join("Idyll.toml"),
        "[package]\nname = \"User\"\n\n[dependencies]\nPlain = { path = \"../Plain\" }\n",
    );
    common::write(&app.join("src/Main.idl"), "use Plain (d)\n\nd\n");
    let out = pipeline::build(&app, Profile::Debug, &common::OPTS);
    assert_eq!(said(&out), ["d is private to Plain: used here"]);
}

// --- module ordering within a package ----------------------------------------

#[test]
fn modules_are_compiled_in_dependency_order() {
    // `Layers` has `Alpha.idl` (needs `Zeta`), `Zeta.idl` and `Main.idl`.
    // Modules are discovered in filename order, so `Alpha` comes first and
    // everything it uses comes later -- checking and running both have to
    // sort that out.
    let dir = scratch("layers");
    let out = pipeline::build(&common::fixture(&dir, "Layers"), Profile::Debug, &common::OPTS);
    assert!(said(&out).is_empty(), "unexpected diagnostics: {:?}", said(&out));
    // describe 20 == (20 / 2) + 1 == 11; twentyOne == 42 / 2 == 21
    assert_eq!(common::value(&out), "32");
}

/// Debug runs on the machine and release as an executable, unless
/// `Idyll.toml` or a flag says otherwise -- and a release backend nobody named
/// gives way to the machine when there is no executable to be had.
#[test]
fn a_manifest_chooses_the_backend() {
    use idyll::package::ProfileConfig;
    use idyll::{Backend, Resolved};

    let bare = Path::new("no/such/place");
    let debug = Resolved::resolve(Profile::Debug, bare, ProfileConfig::default());
    assert_eq!(debug.backend, Backend::Machine);
    assert_eq!(debug.fallback(), None);
    let release = Resolved::resolve(Profile::Release, bare, ProfileConfig::default());
    assert_eq!(release.backend, Backend::Native);
    assert_eq!(release.fallback().map(|r| r.backend), Some(Backend::Machine));

    let dir = std::env::temp_dir().join(format!("idyll-backend-manifest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/Main.idl"), "1\n").unwrap();
    std::fs::write(
        dir.join("Idyll.toml"),
        "[package]\n\
         name = \"Chosen\"\n\
         \n\
         [profile.debug]\n\
         backend = \"machine\"   # what it would be anyway\n\
         \n\
         [profile.release]\n\
         native = true\n",
    )
    .unwrap();

    let m = Manifest::load(&dir).unwrap().expect("a manifest");
    assert_eq!(m.profile("debug").backend, Some(Backend::Machine));
    assert_eq!(m.profile("release").backend, Some(Backend::Native));
    // A profile the manifest says nothing about keeps its built-in meaning.
    assert_eq!(m.profile("bench"), ProfileConfig::default());

    let debug = Resolved::resolve(Profile::Debug, &dir, ProfileConfig::default());
    assert_eq!(debug.backend, Backend::Machine);
    // Named in the manifest, so it is what the package gets or an error.
    let release = Resolved::resolve(Profile::Release, &dir, ProfileConfig::default());
    assert_eq!(release.backend, Backend::Native);
    assert_eq!(release.fallback(), None);

    // A flag beats the manifest.
    let flagged = Resolved::resolve(Profile::Release, &dir, ProfileConfig { backend: Some(Backend::Machine) });
    assert_eq!(flagged.backend, Backend::Machine);

    std::fs::remove_dir_all(&dir).ok();
}

// --- constructors a package re-exports -----------------------------------------

/// A library whose root re-exports a sibling module's type and constructors,
/// and an app using it as `main` says.
fn reexporting_pair(what: &str, main: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idyll-reexport-{}-{what}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = dir.join("shapes");
    let app = dir.join("app");
    for (path, text) in [
        (lib.join("Idyll.toml"), "[package]\nname = \"Shapes\"\nversion = \"0.1.0\"\n"),
        (lib.join("src/Lib.idl"), "@pub use Shapes.Kinds (Shape, area)\n@pub use Shapes.Kinds.Shape.*\n"),
        (
            lib.join("src/Kinds.idl"),
            "@pub data Shape = Square Int | Rect Int Int\n\n\
             @pub area : (s : [] Shape) -> Int\narea s = case s of\n  Shape.Square n -> n * n\n  Shape.Rect w h -> w * h\n",
        ),
        (
            app.join("Idyll.toml"),
            "[package]\nname = \"App\"\nversion = \"0.1.0\"\n\n\
             [dependencies]\nShapes = { path = \"../shapes\" }\n",
        ),
        (app.join("src/Main.idl"), main),
    ] {
        common::write(&path, text);
    }
    app
}

/// What the app comes to: its value, or what was said instead.
fn run_app(app: &Path) -> String {
    let out = pipeline::build(app, Profile::Debug, &common::OPTS);
    if !said(&out).is_empty() {
        return said(&out).join("\n");
    }
    let ran = common::run(&out);
    match ran.evaluation {
        Some(idyllic::pipeline::Evaluation::Value(v)) if !ran.ran_main => v,
        _ => ran.printed,
    }
}

#[test]
fn a_package_root_can_re_export_constructors_flat() {
    // Named in the `use`, as `use Shapes::Square` would be in Rust.
    let named = reexporting_pair("named", "use Shapes (area, Square)\n\narea (Square 4)\n");
    assert_eq!(run_app(&named), "16");
    // A bare `use` brings every one.
    let all = reexporting_pair("all", "use Shapes\n\narea (Rect 2 3)\n");
    assert_eq!(run_app(&all), "6");
    // Without either, they stay under their type.
    let qualified = reexporting_pair("qualified", "use Shapes (area, Shape)\n\narea (Shape.Square 3)\n");
    assert_eq!(run_app(&qualified), "9");
    let unnamed = reexporting_pair("unnamed", "use Shapes (area)\n\narea (Square 4)\n");
    assert!(run_app(&unnamed).contains("not in scope: Square"), "{}", run_app(&unnamed));
}

/// Two libraries, `shapes` and `figures`, each with its own `Shape`, and an
/// app using both.
fn two_shapes(what: &str, main: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idyll-two-shapes-{}-{what}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let app = dir.join("app");
    let mut files = vec![
        (
            app.join("Idyll.toml"),
            "[package]\nname = \"App\"\nversion = \"0.1.0\"\n\n\
             [dependencies]\nShapes = { path = \"../shapes\" }\nFigures = { path = \"../figures\" }\n"
                .to_string(),
        ),
        (app.join("src/Main.idl"), main.to_string()),
    ];
    for (lib, ctor, area) in [("shapes", "Square", "n * n"), ("figures", "Circle", "3 * n * n")] {
        let name = idyll::package::as_package_name(lib);
        let root = dir.join(lib);
        files.push((root.join("Idyll.toml"), format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n")));
        files.push((
            root.join("src/Lib.idl"),
            format!(
                "@pub data Shape = {ctor} Int\n\n\
                 @pub {lib}Area : (s : [] Shape) -> Int\n{lib}Area s = case s of\n  Shape.{ctor} n -> {area}\n\n\
                 @pub {lib}Unit : Shape\n{lib}Unit = Shape.{ctor} 1\n"
            ),
        ));
    }
    for (path, text) in files {
        common::write(&path, &text);
    }
    app
}

#[test]
fn two_packages_may_each_declare_a_type_of_one_name() {
    // Each package's `Shape` is its own: both work side by side.
    let both = two_shapes(
        "both",
        "use Shapes (shapesArea, shapesUnit)\nuse Figures (figuresArea, figuresUnit)\n\n\
         main : () -> () ! Eff\nmain () = let u = printInt (shapesArea shapesUnit) in printInt (figuresArea figuresUnit)\n",
    );
    assert_eq!(run_app(&both), "1\n3\n");
    // One package's value is no value of the other's type.
    let crossed =
        two_shapes("crossed", "use Shapes (shapesArea)\nuse Figures (figuresUnit)\n\nshapesArea figuresUnit\n");
    // (Meadow names each with its package and version -- `Shape` (from
    // `Shapes@0.1.0`) -- which Idyll's message does not yet: both are
    // printed `Shape`.)
    let got = run_app(&crossed);
    assert!(got.contains("type mismatch: expected Shape, but this has type Shape"), "{got}");
    // Named bare, `Shape` is neither's: a dependency's type is in scope only
    // where a `use` brings it.
    let unnamed = two_shapes("unnamed", "f : Shape -> Shape\nf s = s\n\n1\n");
    assert!(run_app(&unnamed).contains("not in scope: Shape"), "{}", run_app(&unnamed));
    let chosen = two_shapes(
        "chosen",
        "use Shapes (Shape, shapesArea)\n\nf : (s : [] Shape) -> Int\nf s = shapesArea s\n\nf (Shape.Square 5)\n",
    );
    assert_eq!(run_app(&chosen), "25");
}

#[test]
fn a_type_of_the_package_itself_shadows_one_of_the_same_name_elsewhere() {
    // `Shape` here is the app's, whatever its dependencies declare -- and so
    // is `Maybe`, which the prelude declares too.
    let local = two_shapes(
        "local",
        "data Shape = Dot\n\ndata Maybe = Maybe Int\n\n\
         f : Shape -> Int\nf s = case s of\n  Shape.Dot -> 7\n\n\
         g : Maybe -> Int\ng m = case m of\n  Maybe.Maybe n -> n\n\n\
         main : () -> () ! Eff\nmain () = let u = printInt (f Shape.Dot) in printInt (g (Maybe.Maybe 2))\n",
    );
    assert_eq!(run_app(&local), "7\n2\n");
}
