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
fn a_length_is_written_with_plus() {
    // `+` at `Nat` is `plus`, the prelude's `Add`'s instance for it, and
    // computes in types: so `append` says its length as it is said
    let src = "use Std.Num.Nat (Nat, plusComm)\nuse Std.Num.Nat.Nat.*\nuse Std.Eq (Eq)\nuse Std.Eq.Eq.*\n\n\
               data Vec : Nat -> Type -> Type where\n  Nil : {a : Type} -> Vec Z a\n  Cons : {n : Nat} -> {a : Type} -> a -> Vec n a -> Vec (S n) a\n\n\
               use Vec.*\n\n\
               append : {n m : Nat} -> {a : Type} -> Vec n a -> Vec m a -> Vec (n + m) a\nappend Nil ys = ys\nappend (Cons x xs) ys = Cons x (append xs ys)\n\n\
               three : Vec (S (S (S Z))) Int\nthree = append (Cons 1 Nil) (Cons 2 (Cons 3 Nil))\n\n\
               swapped : Eq (S Z + S (S Z)) (S (S Z) + S Z)\nswapped = plusComm (S Z) (S (S Z))\n\n";
    assert_eq!(value(&format!("{src}three")), "Cons 1 (Cons 2 (Cons 3 Nil))");
    // a length that is not the sum is a mismatch
    assert!(!rejected(&format!("{src}two : Vec (S (S Z)) Int\ntwo = append (Cons 1 Nil) (Cons 2 (Cons 3 Nil))\n\ntwo")).is_empty());
    // and `+` at `Int` is the primitive still
    assert_eq!(value("(2 + 3) * 4 - 6 / 2 % 2"), "19");
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
    assert_eq!(out.evaluation, Some(Evaluation::Stopped("arrayGet: index 5 out of bounds (len 2)".into())));
}

#[test]
fn a_function_from_a_module_that_is_not_linear_fits_the_library() {
    // the library is linear, and its functions take another's at whatever
    // grades its arguments have: `map` and `all` generic in them, `foldr`
    // taking any
    let src = "use Std.Collections.List (map, foldr, all)\n\n\
               double : Int -> Int\ndouble n = n + n\n\n\
               (map double #[1, 2], foldr (\\x acc -> if x < acc then acc else x) 0 #[3, 1], all (\\x -> 0 < x + x) #[1])";
    assert_eq!(value(src), "(2 :: 4 :: Nil, 3, True)");
    let state = "use Std.State (State, get, put, runState)\n\n\
                 bump : () -> Int ! State Int\nbump u = let n = get () in let v = put (n + n) in n\n\n\
                 runState 4 bump";
    assert_eq!(value(state), "(4, 8)");
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
    // and, in a linear module, dropping the array is an error
    assert!(!rejected(&format!("@linear\n\n{m}case read (new 2 21) 1 of\n  (b, m2) -> let [x] = b in x + x")).is_empty());
}

#[test]
fn a_mutable_array_is_written_in_place() {
    // a million writes to an array of a million: a write that copied the
    // array would take hours, and the run would time out
    let m = "use Std.Collections.MArray (MArray, new, write, freeze)\n\n\
             fill : (m : MArray Int) -> (i : Int) -> MArray Int ! Div\n\
             fill m i = if i == 1000000 then m else fill (write m i i) (i + 1)\n\n";
    assert_eq!(value(&format!("{m}arrayGet (freeze (fill (new 1000000 0) 0)) 999999")), "999999");
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
fn characters_are_their_own_type() {
    assert_eq!(value("'a'"), "'a'");
    assert_eq!(value("#['\\'', '\"', '\\n', 'é']"), "'\\'' :: '\"' :: '\\n' :: 'é' :: Nil");
    assert_eq!(value("(charCode 'A', charFromCode 955)"), "(65, 'λ')");
    // matched as literals are, by their code points
    let kind = "kind : (c : Char) -> String\nkind 'a' = \"a\"\nkind '\\n' = \"newline\"\nkind c = \"other\"\n\n";
    assert_eq!(value(&format!("{kind}(kind 'a', kind '\\n', kind 'z')")), "(\"a\", \"newline\", \"other\")");
    // and a code point that is no character stops the program
    let out = pipeline::run("charFromCode 55296", &OPTS);
    assert_eq!(out.evaluation, Some(Evaluation::Stopped("charFromCode: 55296 is not a Unicode scalar value".into())));
}

#[test]
fn strings_are_taken_apart_by_bytes_and_characters() {
    let s = "use Std.String (byteLength, slice, byteAt, indexOf, split, words, lines, trim, toInt, toUpper, startsWith, endsWith, contains, chars, fromChars, charLength, charAt, equal, compare, join, repeat)\n\n";
    assert_eq!(value(&format!("{s}(byteLength \"héllo\", charLength \"héllo\")")), "(6, 5)");
    assert_eq!(value(&format!("{s}(slice \"hello\" 1 3, slice \"héllo\" 0 2, byteAt \"é\" 1, byteAt \"a\" 3)")), "(\"el\", \"h\u{fffd}\", Just 169, Nothing)");
    assert_eq!(value(&format!("{s}(indexOf \"l\" \"hello\", indexOf \"z\" \"hello\", contains \"ell\" \"hello\")")), "(Just 2, Nothing, True)");
    assert_eq!(value(&format!("{s}(startsWith \"he\" \"hello\", endsWith \"lo\" \"hello\", startsWith \"hello!\" \"hello\")")), "(True, True, False)");
    assert_eq!(value(&format!("{s}split \",\" \"a,b,,c\"")), "\"a\" :: \"b\" :: \"\" :: \"c\" :: Nil");
    assert_eq!(value(&format!("{s}words \"  hello   world\\tagain \"")), "\"hello\" :: \"world\" :: \"again\" :: Nil");
    assert_eq!(value(&format!("{s}lines \"one\\ntwo\\n\"")), "\"one\" :: \"two\" :: Nil");
    assert_eq!(value(&format!("{s}(trim \"  x  \", toUpper \"Hello, é!\")")), "(\"x\", \"HELLO, é!\")");
    assert_eq!(value(&format!("{s}(toInt \"-42\", toInt \"4x\", toInt \"-\")")), "(Just -42, Nothing, Nothing)");
    assert_eq!(value(&format!("{s}(charAt \"héllo\" 1, fromChars (chars \"round trip é\"))")), "(Just 'é', \"round trip é\")");
    assert_eq!(value(&format!("{s}(equal \"a\" \"a\", compare \"apple\" \"apricot\", join \"-\" #[\"a\", \"b\"], repeat \"ab\" 3)")), "(True, Less, \"a-b\", \"ababab\")");
    let c = "use Std.Char (isDigit, isSpace, isAlpha, toUpper, digitToInt, hexDigitToInt, fromCodeSafe)\n\n";
    assert_eq!(value(&format!("{c}(isDigit '7', isSpace '\\t', isAlpha 'é', toUpper 'q', digitToInt '9', hexDigitToInt 'F', fromCodeSafe 55296)")), "(True, True, False, 'Q', Just 9, Just 15, Nothing)");
}

#[test]
fn files_and_the_process() {
    assert_eq!(printed("main : () -> () ! {Fs, Console}\nmain () = putStrLn (Std.Fs.readToStringOr \"none\" \"/no/such/file\")"), "none\n");
    assert_eq!(printed("main : () -> () ! {Process, Console}\nmain () = printInt (Std.Process.argCount ())"), "0\n");
}

#[test]
fn ord_compares_and_min_and_max_choose() {
    let c = "use Std.Cmp (Ord, compare, min, max, clamp)\nuse Std.Num.Nat (Nat, fromInt, toInt)\nuse Std.Ordering.Ordering.*\n\n";
    // `compare` from `<`, at the prelude's types, and Nat's own
    assert_eq!(value(&format!("{c}(compare 1 2, compare \"b\" \"a\", compare 'c' 'c', compare (fromInt 3) (fromInt 2))\n")), "(Less, Greater, Equal, Greater)");
    assert_eq!(value(&format!("{c}(min 3 9, max \"a\" \"b\", clamp 0 10 42, toInt (max (fromInt 2) (fromInt 5)))\n")), "(3, \"b\", 10, 5)");
    // and Nat's `==` and `<`
    assert_eq!(value(&format!("{c}(fromInt 4 == fromInt 4, fromInt 1 < fromInt 2)\n")), "(True, True)");
}

#[test]
fn a_std_value_only_a_solved_argument_carries_still_links_std() {
    // `n` is used at run time, and is `S Z` only by what unification solved
    // it with: the program mentions Std nowhere else (it was linked without
    // Std, and its Cut named a constructor that was not there)
    let src = "use Std.Num.Nat (Nat)\nuse Std.Num.Nat.Nat.*\n\n\
               data Is : Nat -> Type where\n  It : Is (S Z)\n\n\
               pick : {n : [] Nat} -> Is n -> Nat\npick {n} _ = n\n\npick Is.It\n";
    assert_eq!(value(src), "S Z");
}

/// Maps and sets, and an order other than the usual one, for `Int`.
const ORDERED: &str = "use Std.Collections.Map (Map, MapBy)\nuse Std.Collections.Map as Map\n\
                       use Std.Collections.Set (Set, SetBy)\nuse Std.Collections.Set as Set\n\
                       use Std.Cmp (Ord, compare)\nuse Std.Collections.Vector as V\n\n\
                       instance down : Ord Int where\n  compare x y = compare y x\n\n";

#[test]
fn maps_keep_their_keys_in_order() {
    let m = "m : Map String Int\nm = Map.fromList ((\"b\", 2) :: (\"a\", 1) :: (\"c\", 3) :: Nil)\n\n";
    assert_eq!(
        value(&format!("{ORDERED}{m}(Map.toList m, Map.lookup \"b\" m, Map.lookup \"z\" m, Map.size m, Map.member \"c\" m)\n")),
        "((\"a\", 1) :: (\"b\", 2) :: (\"c\", 3) :: Nil, Just 2, Nothing, 3, True)"
    );
    // replaced, not duplicated; deleted; changed in place
    assert_eq!(
        value(&format!(
            "{ORDERED}{m}(Map.toList (Map.insert \"a\" 9 m), Map.toList (Map.delete \"b\" m), \
             Map.toList (Map.adjust \"c\" (\\v -> v * 10) m), Map.toList (Map.insertWith (\\new old -> new + old) \"a\" 5 m))\n"
        )),
        "((\"a\", 9) :: (\"b\", 2) :: (\"c\", 3) :: Nil, (\"a\", 1) :: (\"c\", 3) :: Nil, \
         (\"a\", 1) :: (\"b\", 2) :: (\"c\", 30) :: Nil, (\"a\", 6) :: (\"b\", 2) :: (\"c\", 3) :: Nil)"
    );
    assert_eq!(
        value(&format!("{ORDERED}{m}(V.toList (Map.keys m), V.toList (Map.values m), Map.minEntry m, Map.maxEntry m, Map.getOr 0 \"q\" m)\n")),
        "(\"a\" :: \"b\" :: \"c\" :: Nil, 1 :: 2 :: 3 :: Nil, Just (\"a\", 1), Just (\"c\", 3), 0)"
    );
}

#[test]
fn a_map_keeps_the_order_it_was_built_with_in_its_type() {
    let d = "d : MapBy Int down String\nd = Map.insert @{down} 1 \"a\" (Map.insert @{down} 2 \"b\" (Map.empty @{down}))\n\n";
    // its keys in its order, and its own order used to look in it: what it
    // is is said by its type, whether or not the key's type is known first
    assert_eq!(value(&format!("{ORDERED}{d}(Map.toList d, Map.lookup 1 d, Map.lookup (1 : Int) d)\n")), "((2, \"b\") :: (1, \"a\") :: Nil, Just \"a\", Just \"a\")");
    // and what wants a map in the usual order does not take it
    let usual = "count : Map Int String -> Int\ncount m = Map.size m\n\n";
    assert!(rejected(&format!("{ORDERED}{d}{usual}count d\n")).iter().any(|e| e == "type mismatch"));
}

#[test]
fn sets_hold_each_value_once_in_order() {
    let s = "a : Set Int\na = Set.fromList (3 :: 1 :: 2 :: 1 :: Nil)\n\nb : Set Int\nb = Set.fromList (3 :: 4 :: Nil)\n\n";
    assert_eq!(
        value(&format!(
            "{ORDERED}{s}(Set.toList a, Set.size a, Set.member 2 a, Set.toList (Set.union a b), Set.toList (Set.diff a b), \
             Set.toList (Set.intersect a b), Set.subset (Set.fromList (1 :: Nil)) a, Set.minElem a, Set.maxElem (Set.delete 3 a))\n"
        )),
        "(1 :: 2 :: 3 :: Nil, 3, True, 1 :: 2 :: 3 :: 4 :: Nil, 1 :: 2 :: Nil, 3 :: Nil, True, Just 1, Just 2)"
    );
    assert_eq!(value(&format!("{ORDERED}Set.toList (Set.fromList @{{down}} (1 :: 3 :: 2 :: Nil))\n")), "3 :: 2 :: 1 :: Nil");
}

const HASHED: &str = "use Std.Collections.HashMap (HashMap, HashMapBy)\nuse Std.Collections.HashMap as HashMap\n\
                      use Std.Collections.List as L\nuse Std.Collections.Vector as V\nuse Std.Sort (sort)\n\
                      use Std.Hash (Hash, hash)\n\n\
                      instance same : Hash Int where\n  hash x = 12345\n\n\
                      sample : HashMap String Int\nsample = HashMap.fromList ((\"one\", 1) :: (\"two\", 2) :: (\"three\", 3) :: Nil)\n\n";

#[test]
fn hash_maps_find_what_was_put_in() {
    assert_eq!(
        value(&format!(
            "{HASHED}(HashMap.lookup \"two\" sample, HashMap.lookup \"four\" sample, HashMap.member \"one\" sample, \
             HashMap.getOr 0 \"four\" sample, HashMap.size sample)\n"
        )),
        "(Just 2, Nothing, True, 0, 3)"
    );
    // replaced rather than duplicated, the old map untouched; deleted
    assert_eq!(
        value(&format!(
            "{HASHED}(HashMap.lookup \"one\" (HashMap.insert \"one\" 100 sample), HashMap.size (HashMap.insert \"one\" 100 sample), \
             HashMap.lookup \"one\" sample, HashMap.lookup \"two\" (HashMap.delete \"two\" sample), HashMap.size (HashMap.delete \"two\" sample), \
             HashMap.size (HashMap.delete \"nine\" sample), HashMap.isEmpty (HashMap.delete \"one\" (HashMap.singleton \"one\" 1)))\n"
        )),
        "(Just 100, 3, Just 1, Nothing, 2, 3, True)"
    );
    // changed in place, and whole maps
    assert_eq!(
        value(&format!(
            "{HASHED}(HashMap.lookup \"one\" (HashMap.adjust \"one\" (\\v -> v + 1) sample), \
             HashMap.lookup \"one\" (HashMap.insertWith (\\new old -> new + old) \"one\" 10 sample), \
             HashMap.lookup \"nine\" (HashMap.alter (\\o -> Just 9) \"nine\" sample), HashMap.member \"one\" (HashMap.alter (\\o -> Nothing) \"one\" sample), \
             sort (V.toList (HashMap.values sample)), HashMap.foldl (\\acc k v -> acc + v) 0 sample, HashMap.lookup \"two\" (HashMap.map (\\v -> v * 10) sample), \
             HashMap.size (HashMap.filter (\\k v -> v > 1) sample), V.len (HashMap.toVec sample))\n"
        )),
        "(Just 2, Just 11, Just 9, False, 1 :: 2 :: 3 :: Nil, 6, Just 20, 2, 3)"
    );
    let u = "u : HashMap String Int\nu = HashMap.union sample (HashMap.fromList ((\"two\", 22) :: (\"four\", 4) :: Nil))\n\n";
    assert_eq!(value(&format!("{HASHED}{u}(HashMap.size u, HashMap.lookup \"two\" u)\n")), "(4, Just 22)");
}

#[test]
fn a_hash_map_grows_and_shrinks_through_its_levels() {
    // two thousand keys is several levels deep
    let full = "full : HashMap Int Int\nfull = V.foldl (\\m i -> HashMap.insert i (i * i) m) HashMap.empty (V.range 0 2000)\n\n\
                half : HashMap Int Int\nhalf = V.foldl (\\m i -> HashMap.delete (i * 2) m) full (V.range 0 1000)\n\n";
    assert_eq!(
        value(&format!(
            "{HASHED}{full}(HashMap.size full, V.all (\\i -> HashMap.lookup i full == Just (i * i)) (V.range 0 2000), HashMap.lookup 2000 full, \
             HashMap.size half, V.all (\\i -> HashMap.member i half == (i % 2 == 1)) (V.range 0 2000))\n"
        )),
        "(2000, True, Nothing, 1000, True)"
    );
    // the same entries, the same shape: built forwards, backwards, or with
    // keys added and taken away again, the fold's order is the same
    let shapes = "forwards : HashMap Int Int\nforwards = V.foldl (\\m i -> HashMap.insert i i m) HashMap.empty (V.range 0 300)\n\n\
                  backwards : HashMap Int Int\nbackwards = V.foldl (\\m i -> HashMap.insert (299 - i) (299 - i) m) HashMap.empty (V.range 0 300)\n\n\
                  detour : HashMap Int Int\ndetour = V.foldl (\\m i -> HashMap.delete (i + 1000) m) (V.foldl (\\m i -> HashMap.insert (i + 1000) 0 m) forwards (V.range 0 300)) (V.range 0 300)\n\n";
    assert_eq!(
        value(&format!("{HASHED}{shapes}(HashMap.toList forwards == HashMap.toList backwards, HashMap.toList forwards == HashMap.toList detour)\n")),
        "(True, True)"
    );
}

#[test]
fn keys_whose_hashes_collide_are_told_apart_and_the_hash_is_in_the_type() {
    // every key hashed to one number by an instance of its own: a collision
    // node, searched with `==`, collapsed again as keys go
    let m = "m : HashMapBy Int same Int\nm = V.foldl (\\acc i -> HashMap.insert @{same} i (i * 10) acc) (HashMap.empty @{same}) (V.range 0 5)\n\n\
             few : HashMapBy Int same Int\nfew = V.foldl (\\acc i -> HashMap.delete @{same} i acc) m (V.range 1 5)\n\n";
    assert_eq!(
        value(&format!(
            "{HASHED}{m}(HashMap.size m, V.toList (V.map (\\i -> HashMap.lookup i m) (V.range 0 5)), HashMap.lookup 9 m, \
             HashMap.lookup 3 (HashMap.insert @{{same}} 3 99 m), HashMap.size (HashMap.insert @{{same}} 3 99 m), HashMap.size few, HashMap.toList few)\n"
        )),
        "(5, (Just 0) :: (Just 10) :: (Just 20) :: (Just 30) :: (Just 40) :: Nil, Nothing, Just 99, 5, 1, (0, 0) :: Nil)"
    );
    // and what wants a map hashed the usual way does not take it
    let usual = "count : HashMap Int Int -> Int\ncount h = HashMap.size h\n\n";
    assert!(rejected(&format!("{HASHED}{m}{usual}count m\n")).iter().any(|e| e == "type mismatch"));
}

const RANDOM: &str = "use Std.Random (withSeed, withSeedFrom, between, step, seed, shuffle, choose, nextFloat)\n\
                      use Std.Collections.Vector as V\nuse Std.Tuple (snd)\n\n";

#[test]
fn a_seed_gives_the_same_numbers_as_meadows() {
    // what Meadow's `Std.Random` gives for the same seeds, the same
    // SplitMix64 on every engine
    assert_eq!(
        value(&format!(
            "{RANDOM}(withSeed 42 (\\u -> between 1 7), step (seed 42), V.toList (withSeed 11 (\\u -> shuffle (V.range 1 6))), \
             withSeed 9 (\\u -> nextFloat ()))\n"
        )),
        "(1, (-5014216602933006456, -7046029254386353088), 5 :: 1 :: 3 :: 2 :: 4 :: Nil, 0.6823627349789958)"
    );
}

#[test]
fn random_numbers_keep_to_their_ranges_and_runs_repeat() {
    assert_eq!(
        value(&format!(
            "{RANDOM}(withSeed 7 (\\u -> V.toList (V.map (\\i -> between 0 1000) (V.range 0 3))) == withSeed 7 (\\u -> V.toList (V.map (\\i -> between 0 1000) (V.range 0 3))), \
             withSeed 1 (\\u -> between 0 1000000) == withSeed 2 (\\u -> between 0 1000000), \
             withSeed 4 (\\u -> let n = between (0 - 10) 0 in if n < 0 then (0 - 10) <= n else False), \
             withSeed 1 (\\u -> between 5 5), withSeed 1 (\\u -> between 5 1), \
             withSeed 3 (\\u -> choose (V.range 0 0)), V.sum (withSeed 11 (\\u -> shuffle (V.range 1 6))), \
             snd (withSeedFrom 42 (\\u -> between 0 9)) == seed 42)\n"
        )),
        "(True, False, True, 5, 5, Nothing, 15, False)"
    );
}

#[test]
fn durations_and_dates_read_as_meadows_do() {
    // what Meadow's `Std.Time` writes for the same numbers
    let src = "use Std.Time (formatNanos, formatMillis, formatTimestamp, formatReadable, formatAgo, formatDuration, utc, DateTime)\n\n\
               (formatNanos 742, formatNanos 1234, formatNanos 999600, formatNanos 125300000000, formatNanos 3723000000000, \
               formatMillis 273600000, formatTimestamp 1789405445123, formatReadable 1789405445123, formatTimestamp (0 - 86400001), \
               formatAgo 100000 40000, formatAgo 0 300000, formatDuration 3723004, (utc 0).weekday)\n";
    assert_eq!(
        value(src),
        "(\"742ns\", \"1.23µs\", \"1.00ms\", \"2m 05.3s\", \"1h 02m 03s\", \"3d 04h 00m\", \"2026-09-14T17:04:05.123Z\", \
         \"Mon 14 Sep 2026 17:04:05 UTC\", \"1969-12-30T23:59:59.999Z\", \"1m ago\", \"in 5m\", \"01:02:03.004\", 4)"
    );
}

#[test]
fn a_clock_of_ones_own_answers_the_time() {
    // stopped, or moving on by a tick at each reading and by a sleep
    let src = "use Std.Time (withClock, withTickingClock, elapsed)\n\n\
               (withClock 500 (\\u -> now () > 100), withTickingClock 10 5 (\\u -> let a = now () in let b = now () in b - a), \
               withTickingClock 0 1 (\\u -> let s = sleep 1000 in now ()), withClock 7 (\\u -> elapsed (\\v -> now ())))\n";
    assert_eq!(value(src), "(True, 5, 1000, 0)");
}

#[test]
fn bytes_are_read_and_written_as_meadows_are() {
    let src = "use Std.Bytes\n\nb : Array UInt8\nb = #[0x34; 0x12; 0xff]\n\n\
               (bytesToHex (stringToBytes \"hé!\"), bytesToString (stringToBytes \"hé\"), bytesFromHex \"0aFf\", bytesFromHex \"abc\", \
               bytesGetU16LE b 0, bytesGetU16BE b 0, bytesToHex (bytesSetU32BE (stringToBytes \"abcd\") 0 305419896), \
               bytesGetOr (toUInt8 0) (bytesSet b 0 511) 0, bytesGet b 5, \
               bytesGetU64LE (bytesSetU64LE (stringToBytes \"12345678\") 0 (toUInt64 (0 - 1))) 0, bytesToHex (bytesSetU16LE (stringToBytes \"ab\") 0 4660), \
               bytesLength (bytesPush b 4), bytesIsEmpty bytesEmpty)\n";
    assert_eq!(
        value(src),
        "(\"68c3a921\", \"hé\", Just #[10; 255], Nothing, 4660, 13330, \"12345678\", 255, Nothing, 18446744073709551615, \"3412\", 4, True)"
    );
}

#[test]
fn streams_are_yielded_and_consumed() {
    let src = "use Std.Stream as St\nuse Std.Yield (Yield, yield)\nuse Std.Collections.Vector as V\n\n\
               @terminating\ncountdown : (n : Int) -> () ! Yield Int\ncountdown n = if n <= 0 then () else let u = yield n in countdown (n - 1)\n\n\
               (V.toList (St.toVec (\\u -> countdown 3)), St.toList (\\u -> countdown 3), St.sum (\\u -> St.range 1 5), St.count (\\u -> countdown 4), \
               V.toList (St.take 2 (\\u -> St.repeat 1000000 7)), V.toList (St.takeWhile (\\x -> x > 1) (\\u -> countdown 5)), \
               St.first (\\u -> countdown 0), St.find (\\x -> x % 2 == 0) (\\u -> St.ofList (1 :: 3 :: 4 :: 5 :: Nil)), \
               St.any (\\x -> x > 2) (\\u -> countdown 3), St.all (\\x -> x > 2) (\\u -> countdown 3), \
               St.toList (\\u -> St.map (\\x -> x * 10) (\\v -> St.filter (\\x -> x % 2 == 1) (\\w -> St.range 0 6))), \
               St.toList (\\u -> St.iterate 4 (\\x -> x * 2) 1))\n";
    assert_eq!(
        value(src),
        "(3 :: 2 :: 1 :: Nil, 3 :: 2 :: 1 :: Nil, 10, 4, 7 :: 7 :: Nil, 5 :: 4 :: 3 :: 2 :: Nil, Nothing, Just 4, True, False, \
         10 :: 30 :: 50 :: Nil, 1 :: 2 :: 4 :: 8 :: Nil)"
    );
}

#[test]
fn assertions_fail_when_they_should_and_say_what_failed() {
    let src = "use Std.Test (assert, assertEq, assertNeq, assertTrue, assertFalse, refuteThat, failWith, didFail, failure, assertFails)\n\n\
               inner : () -> Int ! Test\ninner u = let ignored = assertEq 1 2 \"inner\" in 0\n\n\
               (didFail (\\u -> assertEq 1 1 \"fine\"), didFail (\\u -> assert False \"x\"), didFail (\\u -> assertNeq 1 1 \"x\"), \
               didFail (\\u -> assertTrue False \"x\"), didFail (\\u -> assertFalse False \"x\"), didFail (\\u -> refuteThat True \"x\"), \
               didFail (\\u -> failWith \"x\"), didFail (\\u -> 1 + 2 * inner ()), didFail (\\u -> assertFails (\\v -> failWith \"e\") \"f\"), \
               failure (\\u -> assertEq (1 :: 2 :: Nil) (1 :: 3 :: Nil) \"lists\"), failure (\\u -> assertEq (Just \"a\") Nothing \"maybes\"))\n";
    assert_eq!(
        value(src),
        "(False, True, True, True, False, True, True, True, False, (True, \"lists: expected #[1, 3], got #[1, 2]\"), \
         True, \"maybes: expected Nothing, got Just(\\\"a\\\")\")"
    );
}
