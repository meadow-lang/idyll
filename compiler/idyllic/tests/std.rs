//! The standard library, `lib/Std/`, which every program has as `Std`: that
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
    assert_eq!(value("Std.Num.Int.abs (0 - 5)"), "5");
    // nothing is in scope unqualified until a `use` says so, so a program
    // of its own names never clashes with it
    assert_eq!(value("data Nat = Z | S Nat\n\nuse Nat.*\n\nmap : Int -> Int\nmap n = n\n\nmap 3"), "3");
    assert!(rejected("abs (0 - 5)").iter().any(|m| m.contains("not in scope")));
}

#[test]
fn lists() {
    let l = "use Std.Collections.List (map, foldr, foldl, filter, reverse, sum, length, take, drop, zipWith, sortBy, range, maximum)\nuse Std.Num.Nat (fromInt)\n\n";
    // each element of a list is used once: `map` is given a function of a
    // linear argument
    assert_eq!(value(&format!("{l}sum (map (\\(x : Int) -> x + 1) #[1, 2, 3])")), "9");
    assert_eq!(value(&format!("{l}reverse #[1, 2, 3]")), "3 :: 2 :: 1 :: Nil");
    assert_eq!(value(&format!("{l}foldl (\\(acc : Int) (x : Int) -> acc - x) 10 #[1, 2, 3]")), "4");
    assert_eq!(value(&format!("{l}filter (\\(x : [] Int) -> x % 2 == 0) (range 1 (fromInt 6))")), "2 :: 4 :: 6 :: Nil");
    assert_eq!(value(&format!("{l}length (take (fromInt 2) (drop (fromInt 1) #[1, 2, 3, 4]))")), "2");
    assert_eq!(value(&format!("{l}zipWith (\\(a : Int) (b : Int) -> a * b) #[1, 2, 3] #[4, 5]")), "4 :: 10 :: Nil");
    assert_eq!(value(&format!("{l}sortBy Std.Num.Int.compare #[3, 1, 2]")), "1 :: 2 :: 3 :: Nil");
    assert_eq!(value(&format!("{l}maximum #[3, 9, 2]")), "Just 9");
}

#[test]
fn a_list_is_indexed_safely_with_a_fin() {
    // `Fin (size xs)` has an index for each element, and `nth` needs no Maybe
    let src = "use Std.Collections.List (nth)\nuse Std.Num.Fin.Fin.*\n\nnth #[10, 20, 30] (FS FZ)";
    assert_eq!(value(src), "20");
    // an index past the end is a type error, not a run-time one
    let past = "use Std.Collections.List (nth)\nuse Std.Num.Fin.Fin.*\n\nnth #[10] (FS FZ)";
    assert!(rejected(past).iter().any(|m| m.contains("mismatch")), "{:?}", rejected(past));
}

#[test]
fn vectors_carry_their_length() {
    let v = "use Std.Collections.Vec (Vec, head, append, zip, reverse, toList, fromList, index, splitAt)\nuse Std.Collections.Vec.Vec.*\nuse Std.Num.Nat.Nat.*\nuse Std.Num.Fin.Fin.*\nuse Std.Tuple (fst)\n\n";
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
    let n = "use Std.Num.Nat (Nat, plus, plusComm, decEq, fromInt, toInt, times, show)\nuse Std.Num.Nat.Nat.*\nuse Std.Eq (Eq)\nuse Std.Eq.Eq.*\nuse Std.Dec (isYes)\n\n";
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
    let i = "use Std.Num.Int (gcd, pow, max, abs)\n\n";
    assert_eq!(value(&format!("{i}gcd 84 36")), "12");
    assert_eq!(value(&format!("{i}pow 2 (Std.Num.Nat.fromInt 10)")), "1024");
    assert_eq!(value(&format!("{i}max (abs (0 - 7)) 3")), "7");
}

#[test]
fn maybes_results_pairs_and_dependent_pairs() {
    assert_eq!(value("use Std.Maybe (withDefault, map)\n\nwithDefault 0 (map (\\(x : Int) -> x + 1) (Just 41))"), "42");
    assert_eq!(value("use Std.Result (andThen, withDefault)\n\nwithDefault 0 (andThen (\\(x : Int) -> Ok (x * 2)) (Ok 21 : Result String Int))"), "42");
    assert_eq!(value("use Std.Tuple (swap, fst)\n\nfst (swap (1, 2))"), "2");
    let some = "use Std.Collections.Vec (fromListSome, toList)\nuse Std.Sigma (Sigma, first)\nuse Std.Num.Nat (toInt)\n\ntoInt (first (fromListSome #[1, 2, 3]))";
    assert_eq!(value(some), "3");
}

#[test]
fn strings_and_the_console() {
    assert_eq!(value("Std.String.join \", \" #[\"a\", \"b\", \"c\"]"), "\"a, b, c\"");
    assert_eq!(value("Std.String.fromList Std.Num.Int.show #[1, 2]"), "\"#[1, 2]\"");
    assert_eq!(printed("main () = Std.Console.printLines #[\"one\", \"two\"]"), "one\ntwo\n");
}

#[test]
fn exceptions_state_and_generators_are_effects_with_handlers() {
    // `throw` gives up, and `toResult` makes a Result of it
    let throw = "use Std.Exn (Exn, throw, toResult, catch, withDefault, threw, ensure)\n\nsafeDiv : (a : [] Int) -> (b : [] Int) -> Int ! Exn String\nsafeDiv a b = if b == 0 then throw \"divide by zero\" else a / b\n\n";
    assert_eq!(value(&format!("{throw}toResult (\\(u : ()) -> safeDiv 7 2)")), "Ok 3");
    assert_eq!(value(&format!("{throw}toResult (\\(u : ()) -> safeDiv 7 0)")), "Err \"divide by zero\"");
    assert_eq!(value(&format!("{throw}catch (\\(e : String) -> e ++ \"!\") (\\(u : ()) -> throw \"oops\")")), "\"oops!\"");
    assert_eq!(value(&format!("{throw}withDefault 5 (\\(u : ()) -> safeDiv 7 0)")), "5");
    assert_eq!(value(&format!("{throw}threw (\\(u : ()) -> ensure (1 < 0) \"no\")")), "True");
    // a state threaded through, from a first one
    let state = "use Std.State (State, get, put, modify, runState)\n\ncount : () -> Int ! State Int\ncount u = let v = modify (\\(n : [] Int) -> n + 1) in let w = modify (\\(n : [] Int) -> n * 10) in get ()\n\n";
    assert_eq!(value(&format!("{state}runState 4 count")), "(50, 50)");
    // every value yielded, gathered into a list
    let yield_ = "use Std.Yield (Yield, yield, collect, each)\n\n";
    assert_eq!(value(&format!("{yield_}collect (\\(u : ()) -> each #[1, 2, 3])")), "1 :: 2 :: 3 :: Nil");
}

#[test]
fn a_type_and_its_constructor_can_share_a_name() {
    // no `Mk`: the bare name is the type in a type, and the constructor in
    // a term or a pattern
    let t = "data Two a b = Two a b\nuse Two.*\n\nswap2 : Two Int Int -> Two Int Int\nswap2 (Two x y) = Two y x\n\n";
    assert_eq!(value(&format!("{t}swap2 (Two 1 2)")), "Two 2 1");
    // a path says which: through the type, the constructor; the module's,
    // the type
    assert_eq!(value(&format!("{t}Two.Two 3 4")), "Two 3 4");
    // `Std.Sigma.Sigma` is the module's, the type, in a term or a type;
    // `Std.Sigma.Sigma.Sigma`, through the type, the constructor
    let family = "Std.Sigma.Sigma Std.Num.Nat.Nat (\\(n : Std.Num.Nat.Nat) -> Std.Collections.Vec.Vec n Int)";
    let empty = "Std.Sigma.Sigma.Sigma Std.Num.Nat.Nat.Z Std.Collections.Vec.Vec.Nil";
    assert_eq!(value(&format!("Std.Num.Nat.toInt (Std.Sigma.first ({empty} : {family}))")), "0");
}

#[test]
fn tuples_are_pairs() {
    // in a term, a pattern and a type; a longer one nests to the right
    let t = "swap : (Int, Bool) -> (Bool, Int)\nswap (n, b) = (b, n)\n\nthird : {a b c : Type} -> (t : [0..1] (a, b, c)) -> c\nthird (x, y, z) = z\n\n";
    assert_eq!(value(&format!("{t}swap (1, True)")), "(True, 1)");
    assert_eq!(value(&format!("{t}third (1, \"two\", 3)")), "3");
    assert_eq!(value(&format!("{t}((1, 2), 3)")), "((1, 2), 3)");
    // a tuple is the prelude's `Pair`, so Std.Tuple's functions work on it
    assert_eq!(value("Std.Tuple.snd (1, 2, 3)"), "(2, 3)");
    // a tuple type may begin a binder's type, which grades may also do
    assert_eq!(value("apply : (g : (Int, Int) -> Int) -> Int\napply g = g (40, 2)\n\napply (Std.Tuple.uncurry (\\(x : Int) (y : Int) -> x + y))"), "42");
    // and a program's own `Pair` does not change what one is
    assert_eq!(value("data Pair = Pair Int\n\nlet p = (1, 2) in Std.Tuple.fst p"), "1");
}

#[test]
fn arrays() {
    let a = "use Std.Collections.Array (get, set, map, foldl, filter, reverse, range, sum, zip, checkIndex, at, indices, toList)\n\n";
    assert_eq!(value(&format!("{a}get #[1; 2; 3] 1")), "Just 2");
    assert_eq!(value(&format!("{a}get #[1; 2; 3] 3")), "Nothing");
    // persistent: the old array is still there
    assert_eq!(value(&format!("{a}xs : Array Int\nxs = #[1; 2; 3]\n\nsum (set xs 0 10) + sum xs")), "21");
    assert_eq!(value(&format!("{a}map (\\(x : [] Int) -> x * x) (range 1 5)")), "#[1; 4; 9; 16]");
    assert_eq!(value(&format!("{a}filter (\\(x : [] Int) -> x % 2 == 0) (range 0 7)")), "#[0; 2; 4; 6]");
    assert_eq!(value(&format!("{a}reverse #[1; 2; 3]")), "#[3; 2; 1]");
    assert_eq!(value(&format!("{a}zip #[1; 2] #[\"a\"; \"b\"; \"c\"]")), "#[(1, \"a\"); (2, \"b\")]");
    // an index checked once, then used without failing
    assert_eq!(value(&format!("{a}xs : Array Int\nxs = #[10; 20; 30]\n\ncase checkIndex xs 2 of\n  Just i -> at xs i\n  Nothing -> 0")), "30");
    assert_eq!(value(&format!("{a}xs : Array Int\nxs = #[10; 20; 30]\n\nStd.Collections.List.map (\\(i : Std.Num.Fin.Fin (Std.Collections.Array.size xs)) -> at xs i) (indices xs)")), "10 :: 20 :: 30 :: Nil");
    // an index outside an array stops the program, as dividing by zero does
    let out = pipeline::run("arrayGet #[1; 2] 5", &OPTS);
    assert_eq!(out.evaluation, Some(Evaluation::Stopped("index 5 is out of bounds for an array of length 2".into())));
}

#[test]
fn mutable_arrays() {
    let m = "use Std.Collections.MArray (MArray, new, write, read, freeze, thaw, swap, modify, generate)\n\n";
    assert_eq!(value(&format!("{m}freeze (write (write (new 3 0) 0 7) 2 9)")), "#[7; 0; 9]");
    assert_eq!(value(&format!("{m}freeze (swap (thaw (generate 4 0 (\\(i : [] Int) -> i * 10))) 0 3)")), "#[30; 10; 20; 0]");
    assert_eq!(value(&format!("{m}freeze (modify (new 2 5) 1 (\\(x : [] Int) -> x + x))")), "#[5; 10]");
    // a read gives back the array, which is linear and must be used, and
    // the element boxed, to use as often as wanted
    assert_eq!(value(&format!("{m}case read (new 2 21) 1 of\n  (b, m2) -> let [x] = b in (x + x, freeze m2)")), "(42, #[21; 21])");
    assert!(!rejected(&format!("{m}case read (new 2 21) 1 of\n  (b, m2) -> let [x] = b in x + x")).is_empty());
}

#[test]
fn rrb_vectors() {
    let v = "use Std.Collections.Vector (Vector, fromList, toList, len, get, set, pushBack, pushFront, popBack, popFront, append, take, drop, map, filter, foldl, reverse, range, sum, maximum, empty, isEmpty, splitAt, zip, concat, takeWhile)\n\n\
             big : (n : [] Int) -> Vector Int\nbig n = foldl (\\(v : [] Vector Int) (x : [] Int) -> pushBack v x) empty (range 0 n)\n\n\
             front : (n : [] Int) -> Vector Int\nfront n = foldl (\\(v : [] Vector Int) (x : [] Int) -> pushFront v x) empty (range 0 n)\n\n";
    assert_eq!(value(&format!("{v}toList (fromList #[1, 2, 3])")), "1 :: 2 :: 3 :: Nil");
    assert_eq!(value(&format!("{v}get (fromList #[1, 2, 3]) 3")), "Nothing");
    // past one chunk, so into the tree, at both ends
    assert_eq!(value(&format!("{v}len (big 200)")), "200");
    assert_eq!(value(&format!("{v}get (big 200) 137")), "Just 137");
    assert_eq!(value(&format!("{v}get (front 200) 0")), "Just 199");
    assert_eq!(value(&format!("{v}get (front 200) 199")), "Just 0");
    assert_eq!(value(&format!("{v}get (set (big 200) 100 7) 100")), "Just 7");
    assert_eq!(value(&format!("{v}len (popBack (big 100))")), "99");
    assert_eq!(value(&format!("{v}get (popFront (big 100)) 0")), "Just 1");
    assert_eq!(value(&format!("{v}sum (append (big 50) (big 50))")), "2450");
    assert_eq!(value(&format!("{v}sum (take (big 100) 10) + sum (drop (big 100) 10)")), "4950");
    assert_eq!(value(&format!("{v}toList (map (\\(x : [] Int) -> x * 2) (fromList #[1, 2, 3]))")), "2 :: 4 :: 6 :: Nil");
    assert_eq!(value(&format!("{v}len (filter (\\(x : [] Int) -> x % 2 == 0) (big 100))")), "50");
    assert_eq!(value(&format!("{v}get (reverse (big 100)) 0")), "Just 99");
    assert_eq!(value(&format!("{v}maximum (fromList #[1, 3, 2])")), "Just 3");
    // emptied, a vector is the empty vector, whichever way
    assert_eq!(value(&format!("{v}isEmpty (popBack (fromList #[1]))")), "True");
    assert_eq!(value(&format!("{v}isEmpty (drop (fromList #[1, 2]) 2)")), "True");
    assert_eq!(value(&format!("{v}toList (takeWhile (\\(x : [] Int) -> x < 3) (fromList #[1, 2, 3, 1]))")), "1 :: 2 :: Nil");
    assert_eq!(value(&format!("{v}toList (concat (fromList #[fromList #[1, 2], fromList #[3]]))")), "1 :: 2 :: 3 :: Nil");
}

#[test]
fn either_sorting_and_comparing() {
    let e = "use Std.Either (Either, either, partitionEithers, lefts)\nuse Std.Either.Either.*\n\n";
    assert_eq!(value(&format!("{e}either (\\(n : Int) -> n + 1) (\\(m : Int) -> m * 2) (Left 41 : Either Int Int)")), "42");
    assert_eq!(value(&format!("{e}lefts #[Left 1, Right \"a\", Left 2]")), "1 :: 2 :: Nil");
    let s = "use Std.Sort (sortBy, sort, sortDesc, isSorted, sortArray)\n\n";
    assert_eq!(value(&format!("{s}sort #[5, 3, 9, 1, 3]")), "1 :: 3 :: 3 :: 5 :: 9 :: Nil");
    assert_eq!(value(&format!("{s}sortDesc #[5, 3, 9]")), "9 :: 5 :: 3 :: Nil");
    assert_eq!(value(&format!("{s}isSorted (sort #[4, 2, 8, 6])")), "True");
    assert_eq!(value(&format!("{s}sortArray #[3; 1; 2]")), "#[1; 2; 3]");
    assert_eq!(value("Std.Cmp.lexicographic Std.Num.Int.compare #[1, 2] #[1, 3]"), "Less");
}

#[test]
fn files_and_the_process() {
    assert_eq!(printed("main : () -> () ! {Fs, Console}\nmain () = putStrLn (Std.Fs.readToStringOr \"none\" \"/no/such/file\")"), "none\n");
    assert_eq!(printed("main : () -> () ! {Process, Console}\nmain () = printInt (Std.Process.argCount ())"), "0\n");
}
