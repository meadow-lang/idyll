//! Universe levels and strict positivity: what stands between `Type : Type`
//! and a type theory in which a term with no recursion in it terminates.

use idyllic::pipeline::{self, Options};
use idyllic_diagnostics::OutputStyle;

fn run(src: &str) -> pipeline::Output {
    pipeline::run(src, &Options { debug: false, style: OutputStyle::PLAIN })
}

/// `headline: label`, one per diagnostic.
fn diags(out: &pipeline::Output) -> Vec<String> {
    out.diagnostics
        .iter()
        .map(|d| match d.labels.first() {
            Some(l) => format!("{}: {}", d.message, l.message),
            None => d.message.clone(),
        })
        .collect()
}

/// The type of the trailing expression, which must check cleanly.
fn ty(src: &str) -> String {
    let out = run(src);
    assert!(out.diagnostics.is_empty(), "{src}\nunexpected diagnostics: {:#?}", diags(&out));
    out.body_type.unwrap_or_else(|| panic!("{src}\nno body type"))
}

fn accepts(src: &str) {
    let out = run(src);
    assert!(out.diagnostics.is_empty(), "{src}\nunexpected diagnostics: {:#?}", diags(&out));
}

fn rejects(src: &str, expected: &[&str]) {
    assert_eq!(diags(&run(src)), expected, "{src}");
}

// ---------------------------------------------------------------------------
// The hierarchy
// ---------------------------------------------------------------------------

#[test]
fn each_universe_lives_in_the_next() {
    assert_eq!(ty("Type"), "Type 1");
    assert_eq!(ty("Type 1"), "Type 2");
    assert_eq!(ty("Type 0"), "Type 1");
    assert_eq!(ty("Int"), "Type");
    assert_eq!(ty("Int [2]"), "Type");
}

#[test]
fn type_is_not_its_own_type() {
    rejects("(Type : Type)", &["type mismatch: expected Type, but this has type Type 1"]);
}

#[test]
fn a_function_type_lives_in_the_larger_of_its_two_universes() {
    assert_eq!(ty("Int -> Int"), "Type");
    assert_eq!(ty("Type -> Int"), "Type 1");
    assert_eq!(ty("(a : Type) -> a -> a"), "Type 1");
    assert_eq!(ty("Int -> Type 1"), "Type 2");
    // quantifying over every type is not itself one of those types
    rejects("((a : Type) -> a) : Type", &["type mismatch: expected Type, but this has type Type 1"]);
}

#[test]
fn a_type_fits_in_every_larger_universe() {
    assert_eq!(ty("(Int : Type 1)"), "Type 1");
    assert_eq!(ty("(Type : Type 5)"), "Type 5");
    assert_eq!(ty("(\\(a : Type 1) -> a) Int"), "Type 1");
    assert_eq!(ty("(\\(a : Type 1) -> a) Type"), "Type 1");
    rejects("(\\(a : Type) -> a) Type", &["type mismatch: expected Type, but this has type Type 1"]);
}

#[test]
fn a_level_is_a_number() {
    rejects("\\(n : Int) -> Type n", &["a universe level is a number: level variables are not supported yet"]);
}

#[test]
fn a_signature_can_be_in_any_universe() {
    accepts("pair : Type 1\npair = Type -> Type -> Type\n\npair");
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[test]
fn a_data_type_cannot_hold_a_type() {
    // the shape of every "set of all sets" paradox
    rejects(
        "data U = MkU Type",
        &["field too large for its data type: this field's type lives in Type 1, but U lives in Type"],
    );
    rejects(
        "data Poly = MkPoly ((a : Type) -> a -> a)",
        &["field too large for its data type: this field's type lives in Type 1, but Poly lives in Type"],
    );
}

#[test]
fn small_data_types_are_unaffected() {
    accepts("data Pair a b = MkPair a b\n\nMkPair 1 True");
    accepts("data Proxy (a : Type 1) = MkProxy\n\n(MkProxy : Proxy Type)");
}

// ---------------------------------------------------------------------------
// Strict positivity
// ---------------------------------------------------------------------------

#[test]
fn a_type_may_not_appear_to_the_left_of_an_arrow_in_its_own_fields() {
    rejects(
        "data Bad = MkBad (Bad -> Int)",
        &["Bad is not strictly positive: it appears to the left of an arrow in this field of MkBad"],
    );
    rejects(
        "data Bad = MkBad ((Int -> Bad) -> Int)",
        &["Bad is not strictly positive: it appears to the left of an arrow in this field of MkBad"],
    );
}

#[test]
fn to_the_right_of_every_arrow_is_fine() {
    accepts("data Stream = MkStream (Int -> Stream)");
    accepts("data Tree = Leaf | Node Tree Int Tree");
    accepts("data Inf = MkInf (Int -> Bool -> Inf) Int");
}

#[test]
fn a_type_may_not_appear_in_its_own_arguments() {
    rejects(
        "data Bad a = MkBad (Bad (Bad a))",
        &["Bad is not strictly positive: it appears in an argument of itself in this field of MkBad"],
    );
    // a different argument at the recursive occurrence is fine, though
    accepts("data Nest a = NilN | ConsN a (Nest (Maybe a))\ndata Maybe a = Nothing | Just a");
}

#[test]
fn nested_occurrences_follow_the_parameters_polarity() {
    // List's parameter is strictly positive, so a rose tree is fine
    accepts("data Rose a = Node a (List (Rose a))");
    accepts("data Box a = MkBox a\ndata T = MkT (Box T)");
    // but a type whose parameter is used negatively passes the problem on
    rejects(
        "data Neg a = MkNeg (a -> Int)\ndata Bad = MkBad (Neg Bad)",
        &["Bad is not strictly positive: it appears inside Neg, whose parameter is not strictly positive, in this field of MkBad"],
    );
    rejects(
        "data Bad = MkBad (List (Bad -> Int))",
        &["Bad is not strictly positive: it appears to the left of an arrow in this field of MkBad"],
    );
}

#[test]
fn mutually_recursive_types_are_checked_together() {
    accepts("data Even = Zero | ESucc Odd\ndata Odd = OSucc Even");
    // B is what appears to the left of the arrow, and it is only a problem
    // because B in turn contains an A
    rejects(
        "data A = MkA (B -> Int)\ndata B = MkB A",
        &["B is not strictly positive: it appears to the left of an arrow in this field of MkA"],
    );
    // without that, a type from an earlier group is just a type
    accepts("data B = MkB Int\ndata A = MkA (B -> Int)");
}

#[test]
fn an_unknown_type_former_cannot_be_trusted() {
    rejects(
        "data Bad (f : Type -> Type) = MkBad (f (Bad f))",
        &["Bad is not strictly positive: it appears under a type it is not known to be positive in, in this field of MkBad"],
    );
}

// ---------------------------------------------------------------------------
// The paradox
// ---------------------------------------------------------------------------

/// Hurkens' simplification of Girard's paradox: a closed term of
/// `(a : Type) -> a`, which with `Type : Type` type-checks and loops.  Every
/// arrow is unrestricted, since the proof uses its hypotheses freely.  It
/// fails at `u`, which quantifies over `Type` and so cannot itself be in
/// `Type`.
const HURKENS: &str = "\
bot : Type
bot = (a : [] Type) -> a

neg : (z : [] Type) -> Type
neg phi = (h : [] phi) -> bot

pow : (z : [] Type) -> Type
pow a = (h : [] a) -> Type

u : Type
u = (x : [] Type) -> (f : [] ((h : [] pow (pow x)) -> x)) -> pow (pow x)

tau : (t : [] pow (pow u)) -> u
tau t = \\x f q -> t (\\y -> q (f (y x f)))

sigma : (s : [] u) -> pow (pow u)
sigma s = s u (\\t -> tau t)

delta : pow u
delta y = neg ((q : [] pow u) -> (h : [] sigma y q) -> q (tau (sigma y)))

omega : u
omega = tau (\\q -> (x : [] u) -> (h : [] sigma x q) -> q x)

d : Type
d = (q : [] pow u) -> (h : [] sigma omega q) -> q (tau (sigma omega))

lem1 : (q : [] pow u) -> (h1 : [] ((x : [] u) -> (h : [] sigma x q) -> q x)) -> q omega
lem1 q h1 = h1 omega (\\x -> h1 (tau (sigma x)))

lem2 : neg d
lem2 = lem1 delta (\\x h2 h3 -> h3 delta h2 (\\q -> h3 (\\y -> q (tau (sigma y)))))

lem3 : d
lem3 q = lem1 (\\y -> q (tau (sigma y)))

loop : bot
loop = lem2 lem3
";

#[test]
fn girards_paradox_no_longer_type_checks() {
    let out = run(HURKENS);
    let got = diags(&out);
    assert_eq!(got.first().map(String::as_str), Some("type mismatch: expected Type, but this has type Type 1"), "{got:#?}");
}

#[test]
fn moving_the_paradox_up_a_universe_only_moves_the_error() {
    // giving `u` the universe it asks for makes `pow u` the problem instead
    let out = run(&HURKENS.replace("u : Type\n", "u : Type 1\n"));
    let got = diags(&out);
    assert_eq!(got.first().map(String::as_str), Some("type mismatch: expected Type, but this has type Type 1"), "{got:#?}");
}
