//! Records (phase 4 of the roadmap): `record Point = { x : Int, y : Int }`,
//! a data type of one constructor whose fields have names, each of which
//! may mention the fields before it. Built `Point { x = 1, y = 2 }`, read
//! `p.x`, matched `Point { x = a }`, and updated `{ p | x = 3 }`.

use idyllic::pipeline::{self, CompiledPackage, Deps, Evaluation, ModuleFile, Options, PackageSource};
use idyllic_diagnostics::OutputStyle;

const OPTS: Options = Options { debug: false, style: OutputStyle::PLAIN, live: false };

fn diags(out: &pipeline::Output) -> Vec<String> {
    out.diagnostics
        .iter()
        .map(|d| match d.labels.first() {
            Some(l) => format!("{}: {}", d.message, l.message),
            None => d.message.clone(),
        })
        .collect()
}

/// `value : type`, of a program that must check and run.
fn result(src: &str) -> String {
    let out = pipeline::run(src, &OPTS);
    assert!(out.diagnostics.is_empty(), "{src}\n{:#?}", diags(&out));
    match (&out.evaluation, &out.body_type) {
        (Some(Evaluation::Value(v)), Some(ty)) => format!("{v} : {ty}"),
        other => panic!("{src}\nran to {other:?}"),
    }
}

fn decls(src: &str) -> Vec<String> {
    let out = pipeline::run(src, &OPTS);
    assert!(out.diagnostics.is_empty(), "{src}\n{:#?}", diags(&out));
    out.decls.iter().map(|(x, ty)| format!("{x} : {ty}")).collect()
}

fn rejects(src: &str) -> Vec<String> {
    let out = pipeline::run(src, &OPTS);
    diags(&out)
}

const POINT: &str = "record Point = { x : Int, y : Int }\n\n";

#[test]
fn a_record_is_a_type_its_constructor_and_a_projection_for_each_field() {
    assert_eq!(
        decls("record Pair2 (a : Type) = { first : a, second : a }\n"),
        [
            "Pair2 : (a : Type) -> Type",
            "Pair2.Pair2 : {a : Type} -> (first : [1] a) -> (second : [1] a) -> Pair2 a",
            "Pair2.first : {a : Type} -> (self : [0..1] Pair2 a) -> a",
            "Pair2.second : {a : Type} -> (self : [0..1] Pair2 a) -> a",
        ]
    );
}

#[test]
fn built_with_its_fields_in_any_order_and_read_by_name() {
    assert_eq!(result(&format!("{POINT}Point {{ y = 2, x = 1 }}")), "Point 1 2 : Point");
    assert_eq!(result(&format!("{POINT}(Point {{ x = 3, y = 4 }}).y")), "4 : Int");
    // a field is read as `p.x`, or as its projection, which lives under the
    // type as the constructor does
    let norm = "norm : (p : [] Point) -> Int\nnorm p = p.x * p.x + p.y * p.y\n\n";
    assert_eq!(result(&format!("{POINT}{norm}norm (Point {{ x = 3, y = 4 }})")), "25 : Int");
    assert_eq!(result(&format!("{POINT}Point.x (Point {{ x = 5, y = 6 }})")), "5 : Int");
}

#[test]
fn updated_and_matched_by_name() {
    let moved = "moved : (p : [] Point) -> Point\nmoved p = { p | x = p.x + 10 }\n\n";
    assert_eq!(result(&format!("{POINT}{moved}moved (Point {{ x = 1, y = 2 }})")), "Point 11 2 : Point");
    // a field not named is matched by anything
    let first = "first : (p : [0..1] Point) -> Int\nfirst (Point { x = a }) = a\n\n";
    assert_eq!(result(&format!("{POINT}{first}first (Point {{ x = 7, y = 8 }})")), "7 : Int");
}

#[test]
fn a_field_may_depend_on_the_fields_before_it() {
    let sized = "data Nat = Z | S Nat\nuse Nat.*\n\n\
                 @seq(nil = VNil, cons = VCons)\ndata Vec : Nat -> Type -> Type where\n  VNil : Vec Z a\n  VCons : a -> Vec n a -> Vec (S n) a\nuse Vec.*\n\n\
                 record Sized (a : Type) = { size : Nat, items : Vec size a }\n\n";
    // the projection's type reads the earlier field off the record (its type
    // argument shown, as an implicit argument is in a type)
    let all = decls(sized);
    assert!(all.contains(&"Sized.items : {a : Type} -> (self : [0..1] Sized a) -> Vec (size a self) a".to_string()), "{all:?}");
    assert_eq!(result(&format!("{sized}(Sized {{ size = S (S Z), items = #[1, 2] }}).items")), "VCons 1 (VCons 2 VNil) : Vec (S (S Z)) Int");
    // and a field of the wrong length for the size is a type error
    assert!(!rejects(&format!("{sized}Sized {{ size = S Z, items = #[1, 2] }}")).is_empty());
}

#[test]
fn what_is_wrong_with_a_record_is_said() {
    assert_eq!(rejects(&format!("{POINT}Point {{ x = 1 }}")), ["Point is missing y: a record is built with every one of its fields"]);
    assert_eq!(rejects(&format!("{POINT}Point {{ x = 1, y = 2, z = 3 }}")), ["Point has no field z: its fields are x, y"]);
    assert_eq!(rejects(&format!("{POINT}Point {{ x = 1, x = 2, y = 3 }}")), ["the field x is given twice: given again here"]);
    assert_eq!(rejects("data T = T Int\n\nT { x = 1 }"), ["T is not a record: fields, `{ x = .. }`, are for a type declared with `record`"]);
    // a field is found by its name, among the records in scope
    let two = "record A = { x : Int }\nrecord B = { x : Int }\n\nf : (a : [] A) -> Int\nf a = a.x\n\n1";
    assert_eq!(rejects(two), ["2 records have x: A, B: say which: write A.x e"]);
    assert_eq!(rejects(&format!("{POINT}f : (p : [] Point) -> Int\nf p = p.z\n\n1")), ["no record in scope has the field z: reads a record's fields, which this does not name"]);
}

/// A package called `App`: the root's text, then each other module's.
fn package(root: &str, others: &[(&str, &str)]) -> PackageSource {
    let mut modules = vec![ModuleFile { path: Vec::new(), file: "src/Main.idl".into(), text: root.into() }];
    for (path, text) in others {
        modules.push(ModuleFile { path: vec![path.to_string()], file: format!("src/{path}.idl"), text: (*text).into() });
    }
    PackageSource { name: "App".into(), version: None, linear: false, modules }
}

#[test]
fn a_record_from_another_module_or_package() {
    let geometry = "@pub\nrecord Point = { x : Int, y : Int }\n";
    let main = "use Geometry (Point)\n\n\
                swap : (p : [] Point) -> Point\nswap p = Point { x = p.y, y = p.x }\n\n\
                main () = printInt (swap (Point { x = 1, y = 2 })).x\n";
    let out = pipeline::run_package(&package(main, &[("Geometry", geometry)]), &OPTS, Vec::new());
    assert!(out.diagnostics.is_empty(), "{:#?}", diags(&out));
    assert_eq!(out.printed, "2\n");
    // and from a package compiled apart: its fields cross with it
    let lib = PackageSource {
        name: "Geo".into(),
        version: None,
        linear: false,
        modules: vec![ModuleFile { path: Vec::new(), file: "Geo/src/Lib.idl".into(), text: geometry.into() }],
    };
    let compiled = pipeline::compile_package_with(&lib, Deps { all: &[], direct: &[], first_unique: 1 << 20 }, &OPTS);
    assert!(compiled.diagnostics.is_empty(), "{:#?}", diags(&compiled));
    let geo: CompiledPackage = compiled.checked.and_then(|c| c.compiled).expect("compiled");
    let app = PackageSource {
        name: "App".into(),
        version: None,
        linear: false,
        modules: vec![ModuleFile {
            path: Vec::new(),
            file: "App/src/Main.idl".into(),
            text: "use Geo (Point)\n\nmain () = printInt (Point { x = 3, y = 4 }).y\n".into(),
        }],
    };
    let out = pipeline::run_package_with(&app, Deps { all: &[&geo], direct: &[("Geo", &geo)], first_unique: 2 << 20 }, &OPTS, Vec::new());
    assert!(out.diagnostics.is_empty(), "{:#?}", diags(&out));
    assert_eq!(out.printed, "4\n");
}
