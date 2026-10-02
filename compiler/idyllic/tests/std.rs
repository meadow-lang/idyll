//! The standard library, `lib/std/`, which every program has as `Std`: that
//! it compiles, and that what it defines does what it says, run on the AxCut
//! machine.

use idyllic::pipeline::{self, Evaluation, Options};
use idyllic_diagnostics::OutputStyle;

const OPTS: Options = Options { debug: false, style: OutputStyle::PLAIN, live: false };

/// The value of a program, which must check and run.
fn value(src: &str) -> String {
    let out = pipeline::run(src, &OPTS);
    let errors: Vec<String> = out.diagnostics.iter().map(|d| format!("{}: {:?}", d.message, d.labels)).collect();
    assert!(errors.is_empty(), "{src}\n{errors:#?}");
    match out.evaluation {
        Some(Evaluation::Value(v)) => v,
        other => panic!("{src}\nran to {other:?}"),
    }
}

/// What a program prints, running its `main`.
fn printed(src: &str) -> String {
    let out = pipeline::run(src, &OPTS);
    assert!(out.diagnostics.is_empty(), "{src}\n{:#?}", out.diagnostics);
    out.printed
}

/// The messages of a program that does not check.
fn rejected(src: &str) -> Vec<String> {
    let out = pipeline::run(src, &OPTS);
    assert!(out.has_errors(), "{src} checked");
    out.diagnostics.iter().map(|d| d.message.clone()).collect()
}

#[test]
fn std_compiles_without_a_diagnostic() {
    let out = pipeline::check_package_with(&idyllic::stdlib::source(), pipeline::Deps::default(), &OPTS, false);
    let all: Vec<String> = out.diagnostics.iter().map(|d| format!("{}: {:?}", d.message, d.labels)).collect();
    assert!(all.is_empty(), "{all:#?}");
}

#[test]
fn std_is_there_without_being_asked_for_and_only_where_used() {
    // no `[dependencies]`, no `use`: written in full
    assert_eq!(value("Std.Int.abs (0 - 5)"), "5");
    // nothing is in scope unqualified until a `use` says so, so a program
    // of its own names never clashes with it
    assert_eq!(value("data Nat = Z | S Nat\n\nuse Nat.*\n\nmap : Int -> Int\nmap n = n\n\nmap 3"), "3");
    assert!(rejected("abs (0 - 5)").iter().any(|m| m.contains("not in scope")));
}

#[test]
fn lists() {
    let l = "use Std.List (map, foldr, foldl, filter, reverse, sum, length, take, drop, zipWith, sortBy, range, maximum)\nuse Std.Nat (fromInt)\n\n";
    // each element of a list is used once: `map` is given a function of a
    // linear argument
    assert_eq!(value(&format!("{l}sum (map (\\(x : Int) -> x + 1) #[1, 2, 3])")), "9");
    assert_eq!(value(&format!("{l}reverse #[1, 2, 3]")), "3 :: 2 :: 1 :: Nil");
    assert_eq!(value(&format!("{l}foldl (\\(acc : Int) (x : Int) -> acc - x) 10 #[1, 2, 3]")), "4");
    assert_eq!(value(&format!("{l}filter (\\(x : [] Int) -> x % 2 == 0) (range 1 (fromInt 6))")), "2 :: 4 :: 6 :: Nil");
    assert_eq!(value(&format!("{l}length (take (fromInt 2) (drop (fromInt 1) #[1, 2, 3, 4]))")), "2");
    assert_eq!(value(&format!("{l}zipWith (\\(a : Int) (b : Int) -> a * b) #[1, 2, 3] #[4, 5]")), "4 :: 10 :: Nil");
    assert_eq!(value(&format!("{l}sortBy Std.Int.compare #[3, 1, 2]")), "1 :: 2 :: 3 :: Nil");
    assert_eq!(value(&format!("{l}maximum #[3, 9, 2]")), "Just 9");
}

#[test]
fn a_list_is_indexed_safely_with_a_fin() {
    // `Fin (size xs)` has an index for each element, and `nth` needs no Maybe
    let src = "use Std.List (nth)\nuse Std.Fin.Fin.*\n\nnth #[10, 20, 30] (FS FZ)";
    assert_eq!(value(src), "20");
    // an index past the end is a type error, not a run-time one
    let past = "use Std.List (nth)\nuse Std.Fin.Fin.*\n\nnth #[10] (FS FZ)";
    assert!(rejected(past).iter().any(|m| m.contains("mismatch")), "{:?}", rejected(past));
}

#[test]
fn vectors_carry_their_length() {
    let v = "use Std.Vec (Vec, head, append, zip, reverse, toList, fromList, index, splitAt)\nuse Std.Vec.Vec.*\nuse Std.Nat.Nat.*\nuse Std.Fin.Fin.*\nuse Std.Pair (Pair, fst)\n\n";
    assert_eq!(value(&format!("{v}head (Cons 1 (Cons 2 Nil))")), "1");
    assert_eq!(value(&format!("{v}toList (reverse (append (Cons 1 Nil) (Cons 2 (Cons 3 Nil))))")), "3 :: 2 :: 1 :: Nil");
    assert_eq!(value(&format!("{v}index (FS (FS FZ)) (fromList #[5, 6, 7])")), "7");
    assert_eq!(value(&format!("{v}toList (fst (splitAt (S Z) (fromList #[5, 6, 7])))")), "5 :: Nil");
    // the head of an empty vector is not a run-time error: there is none
    assert!(!rejected(&format!("{v}head Nil")).is_empty());
    // nor is zipping two of different lengths
    assert!(!rejected(&format!("{v}zip (Cons 1 Nil) (Cons 2 (Cons 3 Nil))")).is_empty());
}

#[test]
fn naturals_compute_in_types_and_their_proofs_check() {
    let n = "use Std.Nat (Nat, plus, plusComm, decEq, fromInt, toInt, times, show)\nuse Std.Nat.Nat.*\nuse Std.Eq (Eq)\nuse Std.Eq.Eq.*\nuse Std.Dec (isYes)\n\n";
    assert_eq!(value(&format!("{n}toInt (times (fromInt 6) (fromInt 7))")), "42");
    assert_eq!(value(&format!("{n}show (fromInt 1000)")), "\"1000\"");
    // `plus (S Z) (S Z)` computes to `S (S Z)` in a type
    assert_eq!(value(&format!("{n}two : Eq (plus (S Z) (S Z)) (S (S Z))\ntwo = Refl\n\n1")), "1");
    // a proof by induction, used at particular numbers
    assert_eq!(value(&format!("{n}swapped : Eq (plus (S Z) (S (S Z))) (plus (S (S Z)) (S Z))\nswapped = plusComm (S Z) (S (S Z))\n\n1")), "1");
    // a decision, with its proof either way
    assert_eq!(value(&format!("{n}isYes (decEq (fromInt 3) (fromInt 3))")), "True");
    assert_eq!(value(&format!("{n}isYes (decEq (fromInt 3) (fromInt 4))")), "False");
    // and a false equation has no proof
    assert!(!rejected(&format!("{n}wrong : Eq (plus (S Z) (S Z)) (S Z)\nwrong = Refl\n\n1")).is_empty());
}

#[test]
fn integers() {
    let i = "use Std.Int (gcd, pow, max, abs)\n\n";
    assert_eq!(value(&format!("{i}gcd 84 36")), "12");
    assert_eq!(value(&format!("{i}pow 2 (Std.Nat.fromInt 10)")), "1024");
    assert_eq!(value(&format!("{i}max (abs (0 - 7)) 3")), "7");
}

#[test]
fn maybes_results_pairs_and_dependent_pairs() {
    assert_eq!(value("use Std.Maybe (withDefault, map)\n\nwithDefault 0 (map (\\(x : Int) -> x + 1) (Just 41))"), "42");
    assert_eq!(value("use Std.Result (andThen, withDefault)\n\nwithDefault 0 (andThen (\\(x : Int) -> Ok (x * 2)) (Ok 21 : Result String Int))"), "42");
    assert_eq!(value("use Std.Pair (Pair, swap, fst)\nuse Std.Pair.Pair.*\n\nfst (swap (MkPair 1 2))"), "2");
    let some = "use Std.Vec (fromListSome, toList)\nuse Std.Sigma (Sigma, first)\nuse Std.Nat (toInt)\n\ntoInt (first (fromListSome #[1, 2, 3]))";
    assert_eq!(value(some), "3");
}

#[test]
fn strings_and_the_console() {
    assert_eq!(value("Std.String.join \", \" #[\"a\", \"b\", \"c\"]"), "\"a, b, c\"");
    assert_eq!(value("Std.String.fromList Std.Int.show #[1, 2]"), "\"#[1, 2]\"");
    assert_eq!(printed("main () = Std.Console.printLines #[\"one\", \"two\"]"), "one\ntwo\n");
}

#[test]
fn exceptions_state_and_generators_are_effects_with_handlers() {
    // `throw` gives up, and `catch` makes a Result of it
    let throw = "use Std.Throw (Throw, throw, catch)\n\nsafeDiv : (a : [] Int) -> (b : [] Int) -> Int ! Throw String\nsafeDiv a b = if b == 0 then throw \"divide by zero\" else a / b\n\n";
    assert_eq!(value(&format!("{throw}catch (\\(u : ()) -> safeDiv 7 2)")), "Ok 3");
    assert_eq!(value(&format!("{throw}catch (\\(u : ()) -> safeDiv 7 0)")), "Err \"divide by zero\"");
    // a state threaded through, from a first one
    let state = "use Std.State (State, get, put, modify, runState)\nuse Std.Pair (Pair)\n\ncount : () -> Int ! State Int\ncount u = let v = modify (\\(n : [] Int) -> n + 1) in let w = modify (\\(n : [] Int) -> n * 10) in get ()\n\n";
    assert_eq!(value(&format!("{state}runState 4 count")), "MkPair 50 50");
    // every value yielded, gathered into a list
    let yield_ = "use Std.Yield (Yield, yield, collect, each)\n\n";
    assert_eq!(value(&format!("{yield_}collect (\\(u : ()) -> each #[1, 2, 3])")), "1 :: 2 :: 3 :: Nil");
}
