//! Sized integers and floats, as Meadow has them: `Int8` to `UInt64`,
//! `Float` and `Float32`. A number literal is read at the type wanted, with
//! an `Int` or a `Float` the default; each type's arithmetic, comparisons
//! and text are the prelude's instances, made of natives; mixing two types
//! is an error a conversion fixes.

use idyllic::pipeline::{self, Evaluation, Options};
use idyllic_diagnostics::OutputStyle;

const OPTIONS: Options = Options { debug: false, style: OutputStyle::PLAIN, live: false };

/// What a program's body comes to, and its type.
fn value(src: &str) -> (String, String) {
    let out = pipeline::run(src, &OPTIONS);
    let said: Vec<String> = out.diagnostics.iter().map(|d| format!("{}: {:?}", d.message, d.labels.first().map(|l| &l.message))).collect();
    assert!(said.is_empty(), "{src}\n{said:#?}");
    match out.evaluation {
        Some(Evaluation::Value(v)) => (v, out.body_type.unwrap_or_default()),
        other => panic!("{src}\n{other:?}"),
    }
}

/// The headlines and first labels of a program that does not check.
fn rejected(src: &str) -> Vec<String> {
    let out = pipeline::run(src, &OPTIONS);
    assert!(out.has_errors(), "{src} checked");
    out.diagnostics
        .iter()
        .map(|d| match d.labels.first() {
            Some(l) => format!("{}: {}", d.message, l.message),
            None => d.message.clone(),
        })
        .collect()
}

#[test]
fn a_literal_is_read_at_the_type_wanted() {
    assert_eq!(value("((127 : Int8), (255 : UInt8), (1.5 : Float32), (70000 : Int32))\n").0, "(127, 255, 1.5, 70000)");
    // with nothing to say, an integer is an Int and a number with a point a Float
    assert_eq!(value("1.5 * 2.0\n"), ("3.0".into(), "Float".into()));
    assert_eq!(value("let n = 3 in n\n").1, "Int");
}

#[test]
fn a_literal_waits_for_what_comes_after_it() {
    // `1` is checked before `x` says what `+` is at
    let src = "x : Int8\nx = 100\n\n(1 + x, x + 1, 0 - x)\n";
    assert_eq!(value(src).0, "(101, 101, -100)");
    assert_eq!(value("let f : Float32 -> Float32 = \\y -> 2.0 * y in f 1.25\n").0, "2.5");
    // and under a binder, where what it waits for is a meta applied to it
    assert_eq!(value("g : Int8 -> Int8\ng y = 2 * y + 1\n\ng 3\n").0, "7");
}

#[test]
fn sized_integers_wrap_at_their_width() {
    assert_eq!(value("((127 : Int8) + 1, (0 : UInt8) - 1, (200 : UInt8) * 2, (0 - 7 : Int16) / 2, (0 - 7 : Int16) % 2)\n").0, "(-128, 255, 144, -3, -1)");
    // a UInt64 past an Int's largest, and its unsigned division
    assert_eq!(value("(toUInt64 (0 - 1), toUInt64 (0 - 1) / 2, toUInt64 (0 - 1) > 0)\n").0, "(18446744073709551615, 9223372036854775807, True)");
}

#[test]
fn conversions_say_which_type_is_meant() {
    let src = "(toInt8 300, toUInt8 (0 - 1), toInt (toInt8 200), toFloat 3, floor (0.0 - 2.5), toFloat32 (toFloat 7), toFloat64 (1.5 : Float32), toInt64 (5 : Int16))\n";
    assert_eq!(value(src).0, "(44, 255, -56, 3.0, -3, 7.0, 1.5, 5)");
    // `Int64` is `Int`, as `Float64` is `Float`
    assert_eq!(value("(5 : Int64) + toInt (3 : UInt8)\n"), ("8".into(), "Int".into()));
}

#[test]
fn numbers_compare_and_show_themselves() {
    let src = "((1 : Int8) < 2, (3 : UInt32) == 3, 1.5 <= 2.5, (2.0 : Float32) != 2.0, display (2.0 : Float), \"${(5 : UInt16)} ${0.25:?}\", display (0 - 3 : Int32))\n";
    assert_eq!(value(src).0, "(True, True, True, False, \"2.0\", \"5 0.25\", \"-3\")");
}

#[test]
fn a_literal_pattern_is_read_at_the_type_matched() {
    let src = "f : UInt8 -> String\nf 0 = \"zero\"\nf 255 = \"max\"\nf n = \"other\"\n\n\
               g : Float -> Int\ng 0.5 = 1\ng x = 0\n\n\
               (f 0, f 255, f 7, g 0.5, g 1.0)\n";
    assert_eq!(value(src).0, "(\"zero\", \"max\", \"other\", 1, 0)");
}

#[test]
fn a_literal_too_big_for_its_type_is_an_error() {
    assert_eq!(rejected("(300 : Int8)\n"), ["300 does not fit in an Int8: an Int8 is from -128 to 127"]);
    assert_eq!(rejected("(256 : UInt8)\n"), ["256 does not fit in an UInt8: an UInt8 is from 0 to 255"]);
}

#[test]
fn two_number_types_do_not_mix() {
    assert_eq!(rejected("(1 : Int8) + (2 : Int16)\n"), ["type mismatch: expected Int8, but this has type Int16"]);
    assert_eq!(rejected("(1.5 : Int)\n"), ["type mismatch: expected Int, but this has type Float"]);
    let said = rejected("let n : Int = 2 in n * 1.5\n");
    assert_eq!(said[0], "type mismatch: expected Int, but this has type Float");
}

#[test]
fn dividing_a_sized_integer_by_zero_stops_the_program() {
    let out = pipeline::run("(1 : Int8) / 0\n", &OPTIONS);
    assert!(!out.has_errors(), "{:#?}", out.diagnostics);
    match out.evaluation {
        Some(Evaluation::Stopped(why)) => assert!(why.contains("division by zero"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_literal_at_a_type_variable_is_read_through_its_instance() {
    // `fromInt` and `fromFloat`, `Integer`'s and `Floating`'s, at whichever
    // type the code is used at: generic arithmetic says `0` and `1`
    let fact = "@terminating\nfactorial : {n : Type} -> Integer n => PartialEq n => Sub n => Mul n => (k : [] n) -> n\nfactorial k = if k == 0 then 1 else k * factorial (k - 1)\n\n\
                half : {f : Type} -> Floating f => Divide f => (x : [] f) -> f\nhalf x = x / 2.0\n\n";
    assert_eq!(value(&format!("{fact}(factorial 20, factorial (toUInt8 5), half 3.0, half (toFloat32 1.0))\n")).0, "(2432902008176640000, 120, 1.5, 0.5)");
    // with no instance to read it through, it says so
    assert!(rejected("zero : {n : Type} -> (k : [] n) -> n\nzero k = 0\n\n1\n").iter().any(|e| e.contains("Integer n")));
    // and only at a type variable: anything else is a mismatch, as it was
    assert!(rejected("(5 : List Int)\n").iter().any(|e| e.starts_with("type mismatch")));
}
