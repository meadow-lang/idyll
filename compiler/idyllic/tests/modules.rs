//! Modules (phase 3 of the roadmap): constructors living under their types,
//! `use`, packages of several modules that may mention each other, and who
//! may see what.

use idyllic::pipeline::{self, Evaluation, ModuleFile, Options, PackageSource};
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

/// A package called `App`: the root's text, then each other module's path
/// (`"Geometry.Shapes"`) and text.
fn package(root: &str, others: &[(&str, &str)]) -> PackageSource {
    let mut modules = vec![ModuleFile { path: Vec::new(), file: "src/Main.idl".into(), text: root.into() }];
    for (path, text) in others {
        let path: Vec<String> = path.split('.').map(String::from).collect();
        let file = format!("src/{}.idl", path.join("/"));
        modules.push(ModuleFile { path, file, text: (*text).into() });
    }
    PackageSource { name: "App".into(), version: None, linear: false, modules }
}

fn run(root: &str, others: &[(&str, &str)]) -> pipeline::Output {
    pipeline::run_package(&package(root, others), &OPTIONS, Vec::new())
}

/// What running the package printed, which must check.
fn printed(root: &str, others: &[(&str, &str)]) -> String {
    let out = run(root, others);
    assert!(out.diagnostics.is_empty(), "{root}\n{:#?}", diags(&out));
    out.printed
}

fn value(src: &str) -> String {
    let out = pipeline::run(src, &OPTIONS);
    assert!(out.diagnostics.is_empty(), "{src}\n{:#?}", diags(&out));
    match out.evaluation {
        Some(Evaluation::Value(v)) => v,
        other => panic!("{src}\n{other:?}"),
    }
}

fn rejects(src: &str, expected: &[&str]) {
    assert_eq!(diags(&pipeline::run(src, &OPTIONS)), expected, "{src}");
}

// ---------------------------------------------------------------------------
// Constructors live under their types
// ---------------------------------------------------------------------------

#[test]
fn a_constructor_lives_under_its_type() {
    let nat = "data Nat = Z | S Nat\n\n";
    assert_eq!(value(&format!("{nat}Nat.S (Nat.S Nat.Z)\n")), "S (S Z)");
    rejects(
        &format!("{nat}S Z\n"),
        &["name not in scope: S: a constructor lives under its type: Nat.S", "name not in scope: Z: a constructor lives under its type: Nat.Z"],
    );
    // `use T.*` brings them all, `use T (C)` the ones named
    assert_eq!(value(&format!("{nat}use Nat.*\n\nS (S Z)\n")), "S (S Z)");
    assert_eq!(value(&format!("{nat}use Nat (S)\n\nS (S Nat.Z)\n")), "S (S Z)");
    rejects(&format!("{nat}use Nat (Q)\n\nNat.Z\n"), &["Nat has no constructor Q: used here"]);
}

#[test]
fn two_types_may_share_a_constructor_name() {
    let src = "data Shape = Circle Int | Square Int\n\ndata Tile = Square Int | Hex Int\n\n\
               side : Tile -> Int\nside t = case t of\n  Tile.Square n -> n\n  Tile.Hex n -> n\n\n\
               side (Tile.Square 3)\n";
    assert_eq!(value(src), "3");
    // and a type may have one of its own name
    assert_eq!(value("data Box a = Box a\n\nBox.Box 1\n"), "Box 1");
}

#[test]
fn the_preludes_constructors_are_in_scope() {
    assert_eq!(value("Just (1 :: Nil)\n"), "Just (1 :: Nil)");
    assert_eq!(value("List.Nil : List Int\n").as_str(), "Nil");
    // and a program's own `use` puts its constructors ahead of them
    let maybe = "data Maybe a = Nothing | Just a\n\nuse Maybe.*\n\n";
    let out = pipeline::run(&format!("{maybe}Just 3\n"), &OPTIONS);
    assert_eq!(out.body_type.as_deref(), Some("Maybe Int"));
}

#[test]
fn constructors_are_reported_under_their_types() {
    let out = pipeline::run("data Nat = Z | S Nat\n", &OPTIONS);
    let decls: Vec<String> = out.decls.iter().map(|(x, t)| format!("{x} : {t}")).collect();
    assert_eq!(decls, ["Nat : Type", "Nat.Z : Nat", "Nat.S : (_ : [1] Nat) -> Nat"]);
}

// ---------------------------------------------------------------------------
// Packages
// ---------------------------------------------------------------------------

const SYNTAX: &str = "@pub(pkg)\ndata Expr = Lit Int | Add Expr Expr\n";
const EVAL: &str = "use App.Syntax (Expr)\n\n\
                    @pub(pkg)\neval : Expr -> Int\n\
                    eval (Expr.Lit n) = n\n\
                    eval (Expr.Add a b) = eval a + eval b\n";

#[test]
fn modules_use_each_other() {
    let root = "use App.Syntax (Expr)\nuse Eval (eval)\n\n\
                main () = printInt (eval (Expr.Add (Expr.Lit 40) (Expr.Lit 2)))\n";
    assert_eq!(printed(root, &[("Syntax", SYNTAX), ("Eval", EVAL)]), "42\n");
}

#[test]
fn every_form_of_use() {
    let shapes = "@pub\ndata Shape = Square Int | Rect Int Int\n\nuse Shape.*\n\n\
                  @pub\narea : (s : [] Shape) -> Int\n\
                  area (Square n) = n * n\n\
                  area (Rect w h) = w * h\n";
    let others = [("Geometry.Shapes", shapes)];
    // everything it gives, unqualified; constructors stay under their type
    assert_eq!(printed("use Geometry.Shapes\n\nmain () = printInt (area (Shape.Square 3))\n", &others), "9\n");
    // behind a qualifier, and nothing unqualified
    assert_eq!(printed("use Geometry.Shapes as S\n\nmain () = printInt (S.area (S.Shape.Rect 2 5))\n", &others), "10\n");
    // a type's constructors from another module, bare
    assert_eq!(
        printed("use App.Geometry.Shapes (area)\nuse Geometry.Shapes.Shape.*\n\nmain () = printInt (area (Square 4))\n", &others),
        "16\n"
    );
    // written in full, with no use at all
    assert_eq!(printed("main () = printInt (App.Geometry.Shapes.area (Geometry.Shapes.Shape.Square 5))\n", &others), "25\n");
    let out = run("use Geometry.Shapes as S\n\nmain () = printInt (area (S.Shape.Square 3))\n", &others);
    assert_eq!(diags(&out), ["name not in scope: area: used here"]);
}

#[test]
fn modules_may_be_mutually_recursive() {
    let even = "use Odd (isOdd)\n\n@pub\nisEven : (n : [] Int) -> Bool ! Div\nisEven n = if n == 0 then True else isOdd (n - 1)\n";
    let odd = "use Even (isEven)\n\n@pub\nisOdd : (n : [] Int) -> Bool ! Div\nisOdd n = if n == 0 then False else isEven (n - 1)\n";
    let root = "use Even (isEven)\n\nmain () = if isEven 7 then putStrLn \"even\" else putStrLn \"odd\"\n";
    assert_eq!(printed(root, &[("Even", even), ("Odd", odd)]), "odd\n");
}

#[test]
fn a_modules_names_are_reported_under_it() {
    let out = pipeline::compile_package(&package("x : Int\nx = 1\n", &[("Syntax", SYNTAX), ("Eval", EVAL)]), &OPTIONS);
    let names: Vec<&str> = out.decls.iter().map(|(x, _)| x.as_str()).collect();
    assert_eq!(names, ["x", "Syntax.Expr", "Syntax.Expr.Lit", "Syntax.Expr.Add", "Eval.eval"]);
}

#[test]
fn a_diagnostic_says_which_file() {
    let out = run("use Eval (eval)\n\nmain () = printInt (eval 3)\n", &[("Syntax", SYNTAX), ("Eval", EVAL)]);
    let shown = out.display("src/Main.idl", "", OutputStyle::PLAIN);
    assert!(shown.contains("src/Main.idl:3:"), "{shown}");
    let out = run("main () = ()\n", &[("Broken", "f : Int\nf = True\n")]);
    let shown = out.display("src/Main.idl", "", OutputStyle::PLAIN);
    assert!(shown.contains("src/Broken.idl:2:"), "{shown}");
}

// ---------------------------------------------------------------------------
// Visibility
// ---------------------------------------------------------------------------

#[test]
fn a_name_is_private_unless_it_says_otherwise() {
    let math = "@pub(pkg)\ndouble : Int -> Int\ndouble n = n * 2\n\nfudge : Int -> Int\nfudge n = n + 1\n";
    assert_eq!(printed("use Math (double)\n\nmain () = printInt (double 21)\n", &[("Math", math)]), "42\n");
    let out = run("use Math as M\n\nmain () = printInt (M.fudge 1)\n", &[("Math", math)]);
    assert_eq!(diags(&out), ["fudge is private to Math: used here"]);
}

#[test]
fn a_package_that_never_mentions_visibility_has_none() {
    let math = "double : Int -> Int\ndouble n = n * 2\n";
    assert_eq!(printed("use Math (double)\n\nmain () = printInt (double 4)\n", &[("Math", math)]), "8\n");
}

#[test]
fn pub_super_is_for_the_parent_and_its_subtree() {
    let inner = "@pub(super)\nsecret : Int\nsecret = 7\n";
    let outer = "use App.Outer.Inner (secret)\n\n@pub(pkg)\nreveal : Int\nreveal = secret\n";
    assert_eq!(printed("use Outer (reveal)\n\nmain () = printInt reveal\n", &[("Outer", outer), ("Outer.Inner", inner)]), "7\n");
    let out = run("use Outer.Inner (secret)\n\nmain () = printInt secret\n", &[("Outer", outer), ("Outer.Inner", inner)]);
    assert_eq!(diags(&out), ["secret is private to Outer.Inner: used here"]);
}

#[test]
fn pub_use_gives_names_on() {
    // a library's surface, gathered from its modules
    let lib = "@pub\nuse App.Syntax (Expr)\n@pub\nuse Syntax.Expr.*\n@pub\nuse Eval (eval)\n";
    let root = "use Lib\n\nmain () = printInt (eval (Add (Lit 1) (Lit 2)))\n";
    assert_eq!(printed(root, &[("Syntax", SYNTAX), ("Eval", EVAL), ("Lib", lib)]), "3\n");
    // and only `@pub use` gives on: a plain one keeps its names
    let quiet = "use App.Syntax (Expr)\n";
    let out = run("use Quiet\n\nmain () = printInt (eval (Expr.Lit 1))\n", &[("Syntax", SYNTAX), ("Eval", EVAL), ("Quiet", quiet)]);
    assert!(diags(&out).contains(&"name not in scope: eval: used here".to_string()), "{:#?}", diags(&out));
}

// ---------------------------------------------------------------------------
// Reading a package from disk
// ---------------------------------------------------------------------------

#[test]
fn a_package_is_read_from_its_directory() {
    let dir = std::env::temp_dir().join(format!("idyllic-package-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/Geometry")).unwrap();
    std::fs::write(dir.join("Idyll.toml"), "[package]\nname = \"Demo\"\nversion = \"0.1.0\"\n").unwrap();
    std::fs::write(dir.join("src/Main.idl"), "use Demo.Geometry.Shapes (side)\n\nmain () = printInt side\n").unwrap();
    std::fs::write(dir.join("src/Geometry/Shapes.idl"), "side : Int\nside = 6\n").unwrap();
    let package = pipeline::load_package(&dir).unwrap();
    assert_eq!(package.name, "Demo");
    let paths: Vec<String> = package.modules.iter().map(|m| m.path.join(".")).collect();
    assert_eq!(paths, ["", "Geometry.Shapes"]);
    assert_eq!(pipeline::run_package(&package, &OPTIONS, Vec::new()).printed, "6\n");
    // a module file is PascalCase, since it is a name in a use path
    std::fs::write(dir.join("src/helpers.idl"), "x : Int\nx = 1\n").unwrap();
    let err = pipeline::load_package(&dir).unwrap_err();
    assert!(err.contains("rename it to Helpers"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}
