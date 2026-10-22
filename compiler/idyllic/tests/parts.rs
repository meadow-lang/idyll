//! A package's parts: the strongly connected components of its modules'
//! imports, checked one after another, each against the parts it imports
//! (see `idyllic::parts`). Modules may import each other in a circle; what
//! a package means is the same as if it were checked whole.

use idyllic::pipeline::{self, Deps, ModuleFile, Options, PackageSource};
use idyllic_diagnostics::OutputStyle;

const OPTIONS: Options = Options { debug: false, style: OutputStyle::PLAIN, live: false };

fn diags(out: &pipeline::Output) -> Vec<String> {
    out.diagnostics
        .iter()
        .map(|d| match d.labels.first() {
            Some(l) => format!("{}: {}", d.message, l.message),
            None => d.message.clone(),
        })
        .collect()
}

/// A package of these modules, by path (`""` the root, first).
fn package(modules: &[(&str, &str)]) -> PackageSource {
    let modules = modules
        .iter()
        .map(|(path, text)| {
            let path: Vec<String> = if path.is_empty() { Vec::new() } else { path.split('.').map(String::from).collect() };
            let file = if path.is_empty() { "src/Main.idl".to_string() } else { format!("src/{}.idl", path.join("/")) };
            ModuleFile { path, file, text: text.to_string() }
        })
        .collect();
    PackageSource { name: "App".into(), version: Some("0.1.0".into()), linear: false, modules }
}

fn run(modules: &[(&str, &str)]) -> pipeline::Output {
    pipeline::run_package_with(&package(modules), Deps::default(), &OPTIONS, Vec::new())
}

const EVEN: &str = "use Odd (odd)\n\n@pub\neven : Int -> Bool ! Div\neven 0 = True\neven n = odd (n - 1)\n";
const ODD: &str = "use Even (even)\n\n@pub\nodd : Int -> Bool ! Div\nodd 0 = False\nodd n = even (n - 1)\n";

#[test]
fn modules_may_import_each_other() {
    let out = run(&[
        ("", "use Even (even)\nuse Shapes\n\nmain : () -> () ! {Console, Div}\nmain u = putStrLn \"${even 10} ${Shapes.area (Shapes.Shape.Square 3)}\"\n"),
        ("Even", EVEN),
        ("Odd", ODD),
        ("Shapes", "@pub\ndata Shape = Square Int | Rect Int Int\n\nuse Shape.*\n\n@pub\narea : Shape -> Int\narea (Square n) = n * n\narea (Rect a b) = a * b\n"),
    ]);
    assert!(out.diagnostics.is_empty(), "{:#?}", diags(&out));
    assert_eq!(out.printed, "True 9\n");
}

#[test]
fn a_type_computes_across_parts_and_an_instance_is_seen_where_its_module_is() {
    let out = run(&[
        (
            "",
            "use Vectors (Vec, Length, three)\nuse Pretty\n\n\
             -- `Length` unfolds here, a part away from where it is defined\n\
             same : Vec 3 -> Vec (Length 1 2)\nsame v = v\n\n\
             main : () -> () ! Console\nmain u = putStrLn (display (same three))\n",
        ),
        ("Vectors", "@pub\ndata Vec (n : Int) = Vec Int\n\n@pub\nLength : Int -> Int -> Int\nLength a b = a + b\n\n@pub\nthree : Vec 3\nthree = Vec.Vec 3\n"),
        ("Pretty", "use Vectors (Vec)\n\ninstance Display (Vec n) where\n  display v = case v of\n    Vec.Vec k -> \"a vector of ${k}\"\n"),
    ]);
    assert!(out.diagnostics.is_empty(), "{:#?}", diags(&out));
    assert_eq!(out.printed, "a vector of 3\n");
}

#[test]
fn a_part_that_does_not_check_stops_those_that_import_it() {
    let out = run(&[
        ("", "use Broken (oops)\nuse Fine (fine)\n\nmain : () -> () ! Console\nmain u = putStrLn (fine ++ oops)\n"),
        ("Broken", "@pub\noops : String\noops = 2\n"),
        ("Fine", "@pub\nfine : String\nfine = 2\n"),
    ]);
    // each part that could be checked was: both errors are reported, and
    // the root, which imports them, is not checked to report more
    assert_eq!(diags(&out).len(), 2, "{:#?}", diags(&out));
    assert!(out.has_errors());
}

#[test]
fn what_a_module_keeps_to_itself_stays_there() {
    let out = run(&[
        ("", "use Secret (hidden)\n\nmain : () -> () ! Console\nmain u = putStrLn hidden\n"),
        ("Secret", "@pub\nshown : String\nshown = \"a\"\n\n@pub(self)\nhidden : String\nhidden = \"b\"\n"),
    ]);
    assert!(out.has_errors(), "a private name was taken from another part");
}
