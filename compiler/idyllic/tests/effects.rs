//! Effects (phase 2 of the roadmap): rows on arrows, `Div` and the
//! termination checker behind it, the rule that nothing with effects gets
//! into a type, and matching on erased values their types decide.

use idyllic::pipeline::{self, Evaluation, Options};
use idyllic_diagnostics::OutputStyle;

fn run(src: &str) -> pipeline::Output {
    pipeline::run(src, &Options { debug: false, style: OutputStyle::PLAIN, live: false })
}

fn diags(out: &pipeline::Output) -> Vec<String> {
    out.diagnostics
        .iter()
        .map(|d| match d.labels.first() {
            Some(l) => format!("{}: {}", d.message, l.message),
            None => d.message.clone(),
        })
        .collect()
}

fn accepts(src: &str) -> pipeline::Output {
    let out = run(src);
    assert!(out.diagnostics.is_empty(), "{src}\nunexpected diagnostics: {:#?}", diags(&out));
    out
}

fn rejects(src: &str, expected: &[&str]) {
    assert_eq!(diags(&run(src)), expected, "{src}");
}

fn ty(src: &str) -> String {
    accepts(src).body_type.unwrap_or_else(|| panic!("{src}\nno body type"))
}

const NAT: &str = "data Nat = Z | S Nat\n\nuse Nat.*\n\n";
const SPIN: &str = "spin : Int -> Int ! Div\nspin n = spin n\n\n";

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[test]
fn a_row_goes_after_an_arrow() {
    assert_eq!(ty("Int -> Int ! Div"), "Type");
    assert_eq!(ty("Int -> Int -> Int ! Div"), "Type");
    // `Effect` is a type like `Grade`
    assert_eq!(ty("{e : Effect} -> (Int -> Int ! e) -> Int ! e"), "Type");
    assert_eq!(ty("Int -> Int ! {Div}"), "Type");
    // an empty row is no row at all
    assert_eq!(ty("(Int -> Int ! {}) : Type"), "Type");
    // a recursive definition's type is a function's
    rejects("xs : Int ! Div\nxs = xs\n", &["effects belong to a function type: write this after an arrow, as in a -> b ! e"]);
}

#[test]
fn a_row_is_a_set() {
    accepts("f : Int -> Int ! {Div, Div}\nf = \\(n : Int) -> n\n\ng : Int -> Int ! Div\ng = f\n");
}

#[test]
fn a_lambdas_row_is_what_its_body_does() {
    assert_eq!(ty(&format!("{SPIN}\\(n : Int) -> spin n")), "(n : Int) -> Int ! Div");
    assert_eq!(ty(&format!("{SPIN}\\(n : Int) -> n")), "(n : Int) -> Int");
}

#[test]
fn a_signature_with_no_row_allows_none() {
    rejects(
        &format!("{SPIN}f : Int -> Int\nf n = spin n\n"),
        &["effects not allowed: this has the effects Div, but its type allows none"],
    );
    accepts(&format!("{SPIN}f : Int -> Int ! Div\nf n = spin n\n"));
    // a definition that is not a function says it with its type
    rejects(&format!("{SPIN}x : Int\nx = spin 0\n"), &["effects not allowed: this has the effects Div, but its type allows none"]);
    accepts(&format!("{SPIN}x : Int ! Div\nx = spin 0\n"));
}

#[test]
fn a_row_variable_takes_what_it_is_given() {
    let apply = "apply : (f : a -> b ! e) -> a -> b ! e\napply f x = f x\n\n";
    // pure where the function is pure
    accepts(&format!("{apply}pure : Int -> Int\npure n = apply (\\(x : Int) -> x) n\n"));
    // and with the function's effects where it has some
    rejects(
        &format!("{SPIN}{apply}f : Int -> Int\nf n = apply spin n\n"),
        &["effects not allowed: this has the effects Div, but its type allows none"],
    );
    accepts(&format!("{SPIN}{apply}f : Int -> Int ! Div\nf n = apply spin n\n"));
}

// ---------------------------------------------------------------------------
// Div
// ---------------------------------------------------------------------------

#[test]
fn a_recursion_the_checker_cannot_see_end_needs_div() {
    rejects(
        "count : (n : [] Int) -> Int\ncount n = if n == 0 then 0 else count (n - 1)\n",
        &["count may not terminate: no argument gets smaller on every round of its recursion, so it has the effect Div"],
    );
    let src = "count : (n : [] Int) -> Int ! Div\ncount n = if n == 0 then 0 else count (n - 1)\n\ncount 10\n";
    assert_eq!(accepts(src).evaluation, Some(Evaluation::Value("0".into())));
}

#[test]
fn structural_recursion_is_total() {
    accepts(&format!("{NAT}double : Nat -> Nat\ndouble Z = Z\ndouble (S n) = S (S (double n))\n"));
}

#[test]
fn a_program_may_diverge() {
    let out = accepts(&format!("{SPIN}spin 0\n"));
    assert_eq!(out.evaluation, Some(Evaluation::OutOfFuel));
}

// ---------------------------------------------------------------------------
// Nothing with effects gets into a type
// ---------------------------------------------------------------------------

#[test]
fn a_type_is_pure() {
    let vec = "data Vec : Nat -> Type -> Type where\n  Nil : Vec Z a\n  Cons : a -> Vec n a -> Vec (S n) a\n\nuse Vec.*\n\n";
    let spin = "spinN : Nat -> Nat ! Div\nspinN n = spinN n\n\n";
    rejects(
        &format!("{NAT}{vec}{spin}f : Vec (spinN Z) Int -> Int\nf v = 0\n"),
        &["a type must be pure: computing this type has the effects Div"],
    );
}

#[test]
fn a_type_cannot_depend_on_an_argument_with_effects() {
    let vec = "data Vec : Nat -> Type -> Type where\n  Nil : Vec Z a\n  Cons : a -> Vec n a -> Vec (S n) a\n\nuse Vec.*\n\n";
    let spin = "spinN : Nat -> Nat ! Div\nspinN n = spinN n\n\n";
    let len = "len : (n : Nat) -> (v : [0] Vec n Int) -> Nat\nlen n v = n\n\n";
    rejects(
        &format!("{NAT}{vec}{spin}{len}g : Nat ! Div\ng = len (spinN Z) Nil\n"),
        &["a type cannot depend on this: the result type mentions this argument, which has the effects Div"],
    );
    // bound to a variable first, it is fine: the type sees the variable
    accepts(&format!(
        "{NAT}{vec}{spin}{len}g : Nat ! Div\ng = let m = spinN Z in m\n"
    ));
}

#[test]
fn an_erased_argument_is_pure() {
    rejects(
        &format!("{SPIN}(\\(n : [0] Int) -> 5) (spin 0)"),
        &["an erased argument must be pure: this has the effects Div, but it is never run, so they could not happen"],
    );
}

#[test]
fn a_let_with_effects_is_opaque_to_the_types() {
    let vec = "data Vec : Nat -> Type -> Type where\n  Nil : Vec Z a\n  Cons : a -> Vec n a -> Vec (S n) a\n\nuse Vec.*\n\n";
    let spin = "spinN : Nat -> Nat ! Div\nspinN n = spinN n\n\n";
    let rep = "rep : (n : Nat) -> Vec n Int\nrep Z = Nil\nrep (S k) = Cons 0 (rep k)\n\n";
    rejects(
        &format!("{NAT}{vec}{spin}{rep}let m = spinN Z in rep m"),
        &["a type cannot depend on this: the type of this depends on m, whose definition has effects"],
    );
}

// ---------------------------------------------------------------------------
// Matching on what is erased
// ---------------------------------------------------------------------------

const EQ: &str = "data Eq : Nat -> Nat -> Type where\n  Refl : Eq x x\n\nuse Eq.*\n\n";

#[test]
fn an_erased_proof_can_be_matched_on() {
    let vec = "data Vec : Nat -> Type -> Type where\n  Nil : Vec Z a\n  Cons : a -> Vec n a -> Vec (S n) a\n\nuse Vec.*\n\n";
    let src = format!(
        "{NAT}{vec}{EQ}transport : (p : [0] Eq n m) -> Vec n a -> Vec m a\ntransport Refl v = v\n\n\
         transport Refl (Cons 1 Nil)\n"
    );
    let out = accepts(&src);
    assert_eq!(out.evaluation, Some(Evaluation::Value("Cons 1 Nil".into())));
    // and nothing is looked at: no switch on the proof
    assert!(!out.axcut.expect("lowered").to_string().contains("switch"));
}

#[test]
fn an_erased_value_with_a_choice_cannot_be_matched_on() {
    rejects(
        &format!("{NAT}f : (m : [0] Nat) -> Int\nf Z = 0\nf (S n) = 1\n"),
        &["usage mismatch: m has grade 0, but is used once"],
    );
}

#[test]
fn an_erased_value_of_an_empty_type_is_impossible() {
    let fin = "data Fin : Nat -> Type where\n  FZ : Fin (S n)\n  FS : Fin n -> Fin (S n)\n\nuse Fin.*\n\n";
    accepts(&format!("{NAT}{fin}noFin : (i : [0] Fin Z) -> Int\nnoFin i impossible\n"));
}

// ---------------------------------------------------------------------------
// The program's own effects, and handlers
// ---------------------------------------------------------------------------

fn value(src: &str) -> String {
    let out = accepts(src);
    match (out.evaluation, out.body_type) {
        (Some(Evaluation::Value(v)), Some(ty)) => format!("{v} : {ty}"),
        other => panic!("{src}\nexpected a value, got {other:?}"),
    }
}

const ASK: &str = "effect Ask where\n  ask : () -> Int\n\n";
const SUM3: &str = "sum3 : () -> Int ! Ask\nsum3 u = ask () + ask () + ask ()\n\n";

#[test]
fn an_operation_is_a_function_with_its_effect() {
    let out = accepts(ASK);
    assert!(out.decls.contains(&("ask".into(), "() -> Int ! Ask".into())), "{:?}", out.decls);
    assert!(out.decls.contains(&("Ask".into(), "Effect".into())), "{:?}", out.decls);
    let state = "effect State (s : Type) where\n  get : () -> s\n  put : s -> ()\n\n";
    let out = accepts(state);
    assert!(out.decls.contains(&("put".into(), "{s : Type} -> s -> () ! State s".into())), "{:?}", out.decls);
}

#[test]
fn a_handler_answers_its_operations() {
    assert_eq!(value(&format!("{ASK}{SUM3}handle sum3 () with\n  ask _ k -> k 10\n")), "30 : Int");
}

#[test]
fn a_handler_need_not_resume() {
    let fail = "effect Fail where\n  fail : () -> Int\n\n\
                safeDiv : (a : [] Int) -> (b : [] Int) -> Int ! Fail\n\
                safeDiv a b = if b == 0 then fail () else a / b\n\n";
    let src = |a: u32, b: u32| format!("{fail}handle safeDiv {a} {b} with\n  return x -> x + 1\n  fail _ k -> 0\n");
    assert_eq!(value(&src(7, 0)), "0 : Int");
    assert_eq!(value(&src(7, 2)), "4 : Int");
}

#[test]
fn a_handler_can_resume_more_than_once() {
    let src = "effect Choose where\n  flip : () -> Bool\n\n\
               pick : () -> Int ! Choose\npick u = if flip () then 1 else 10\n\n\
               handle pick () with\n  flip _ k -> k True + k False\n";
    assert_eq!(value(src), "11 : Int");
}

#[test]
fn state_is_a_handler_returning_a_function() {
    let src = "effect State (s : Type) where\n  get : () -> s\n  put : (x : [] s) -> ()\n\n\
               counter : () -> Int ! State Int\n\
               counter u =\n  let a = get () in\n  let b = put (a + 1) in\n  let c = get () in\n  c * 10\n\n\
               run : (s : [] Int) -> Int\n\
               run = handle counter () with\n  return x -> \\(s : [] Int) -> x\n  \
               get _ k -> \\(s : [] Int) -> k s s\n  put s2 k -> \\(s : [] Int) -> k () s2\n\n\
               run 4\n";
    assert_eq!(value(src), "50 : Int");
}

#[test]
fn handlers_nest_and_effects_stay_polymorphic() {
    let src = "effect Log where\n  log : Int -> ()\n\n\
               twice : (f : [] (() -> Int ! e)) -> Int ! e\ntwice f = f () + f ()\n\n\
               logged : () -> Int ! {Ask, Log}\nlogged u = let x = log 1 in ask ()\n\n\
               inner : () -> Int ! Log\ninner u = handle twice logged with\n  ask _ k -> let y = log 100 in k 5\n\n\
               handle inner () with\n  return x -> x\n  log n k -> n + k ()\n";
    assert_eq!(value(&format!("{ASK}{src}")), "212 : Int");
}

#[test]
fn a_handled_effect_leaves_the_row() {
    // `inner` would not check if its handler left `Ask` behind
    accepts(&format!("{ASK}{SUM3}f : () -> Int\nf u = handle sum3 () with\n  ask _ k -> k 1\n"));
}

#[test]
fn an_effect_nothing_handles_is_an_error() {
    rejects(&format!("{ASK}ask ()\n"), &["unhandled effects: the program has the effects Ask, and nothing handles them"]);
    rejects(
        &format!("{ASK}f : () -> Int\nf u = ask ()\n"),
        &["effects not allowed: this has the effects Ask, but its type allows none"],
    );
}

#[test]
fn a_handler_handles_one_effect_and_all_of_it() {
    let two = "effect Two where\n  one : () -> Int\n  two : () -> Int\n\n";
    rejects(
        &format!("{two}handle one () with\n  one _ k -> k 1\n"),
        &["the handler does not handle every operation: missing: two"],
    );
    rejects(
        &format!("{ASK}{two}handle ask () with\n  ask _ k -> k 1\n  one _ k -> k 2\n  two _ k -> k 3\n"),
        &["one handler, one effect: this operation belongs to another effect than the clauses before it"],
    );
    rejects(
        &format!("{ASK}handle ask () with\n  ask _ k -> k 1\n  ask _ k -> k 2\n"),
        &["ask is handled twice: this clause can never be taken"],
    );
    rejects(
        &format!("{ASK}f : Int -> Int\nf x = x\n\nhandle ask () with\n  f _ k -> k 1\n"),
        &["f is not an operation: a handler's clauses are for the operations of an effect, and return"],
    );
    rejects(
        &format!("{ASK}handle ask () with\n  ask k -> k 1\n"),
        &["ask's clause names 2 things: ask x k -> .., for its argument and the resumption"],
    );
}

#[test]
fn an_operation_takes_arguments() {
    rejects("effect Bad where\n  bad : Int\n", &["bad is not an operation's type: an operation takes arguments and gives a result, as in a -> b; use () for none"]);
    // as many as its type has arrows; it is performed once it has them all,
    // and can be passed as a function until then
    let calc = "effect Calc where\n  add2 : Int -> Int -> Int\n  neg : Int -> Int\n\n";
    let src = format!(
        "{calc}apply2 : (f : Int -> Int -> Int ! e) -> Int ! e\napply2 f = f 30 4\n\n\
         handle add2 1 (neg 2) + apply2 add2 with\n  add2 a b k -> k (a + b)\n  neg n k -> k (0 - n)\n"
    );
    assert_eq!(accepts(&src).evaluation, Some(Evaluation::Value("33".into())));
    rejects(
        &format!("{calc}handle add2 1 2 with\n  add2 a k -> k a\n  neg n k -> k n\n"),
        &["add2's clause names 3 things: add2 x y k -> .., for its arguments and the resumption"],
    );
}

#[test]
fn the_native_back_end_does_not_run_handlers_yet() {
    let out = run(&format!("{ASK}{SUM3}handle sum3 () with\n  ask _ k -> k 10\n"));
    let program = out.native.expect("lowered");
    let err = idyllic_llvm::emit(&program, idyllic_llvm::ResultKind::Int).unwrap_err();
    assert!(err.contains("not supported by the native back end"), "{err}");
}

#[test]
fn a_producer_can_go_on_for_ever_if_its_consumer_stops() {
    // Meadow's streams, in small: `from` yields for ever, and the handler
    // stops resuming it after five
    let src = "effect Yield where\n  \
               yield : (x : [] Int) -> ()\n\
               \n\
               from : (n : [] Int) -> () ! {Yield, Div}\n\
               from n = let u = yield n in from (n + 1)\n\
               \n\
               first : () -> ((n : [] Int) -> List Int ! Div) ! Div\n\
               first u = handle from 0 with\n  \
               return v -> \\(n : [] Int) -> Nil\n  \
               yield x k -> \\(n : [] Int) -> if n == 0 then Nil else x :: k () (n - 1)\n\
               \n\
               first () 5\n";
    assert_eq!(value(src), "0 :: 1 :: 2 :: 3 :: 4 :: Nil : List Int");
}

// ---------------------------------------------------------------------------
// Aliases, the console and main
// ---------------------------------------------------------------------------

const LOG: &str = "effect Log where\n  log : () -> ()\n\n";

#[test]
fn an_effect_alias_names_a_row() {
    let src = format!(
        "{ASK}{LOG}effect App = {{Log, Ask}}\n\n\
         work : () -> Int ! App\nwork u = let v = log () in ask () * 2\n\n\
         twice : (f : [] (() -> Int ! {{App | e}})) -> Int ! {{App | e}}\ntwice f = f () + f ()\n\n\
         handle (handle twice work with {{ ask _ k -> k 21 }}) with\n  log _ k -> k ()\n"
    );
    let out = accepts(&src);
    assert_eq!(out.evaluation, Some(Evaluation::Value("84".into())));
    // it is a definition of a row, and what it names is what is shown
    assert!(out.decls.contains(&("App".into(), "Effect".into())), "{:?}", out.decls);
    assert!(out.decls.contains(&("work".into(), "() -> Int ! {Ask, Log}".into())), "{:?}", out.decls);
}

#[test]
fn an_effect_alias_can_have_parameters() {
    let state = "effect Read (s : Type) where\n  get : () -> s\n\n";
    let src = format!(
        "{state}effect Stateful s = {{Read s, Div}}\n\n\
         count : Int -> Int ! Stateful Int\ncount n = n + get ()\n\n\
         handle count 1 with\n  get _ k -> k 41\n"
    );
    let out = accepts(&src);
    assert!(out.decls.contains(&("Stateful".into(), "(s : [] Type) -> Effect".into())), "{:?}", out.decls);
    assert_eq!(out.evaluation, Some(Evaluation::Value("42".into())));
}

#[test]
fn the_runtime_handles_the_console() {
    let out = accepts("let u = putStrLn \"hello\" in printInt 42\n");
    assert_eq!(out.printed, "hello\n42\n");
    assert_eq!(out.evaluation, Some(Evaluation::Value("()".into())));
    // and only the console, and divergence: the effects in `Eff`
    rejects(
        &format!("{ASK}ask ()\n"),
        &["unhandled effects: the program has the effects Ask, and nothing handles them"],
    );
}

#[test]
fn a_program_with_a_main_runs_it() {
    let src = "count : (n : [] Int) -> () ! Eff\n\
               count n = if n == 0 then putStrLn \"liftoff\" else let u = printInt n in count (n - 1)\n\n\
               main : () -> () ! Eff\nmain () = let u = putStr \"counting: \" in count 2\n";
    let out = accepts(src);
    assert!(out.ran_main);
    assert_eq!(out.printed, "counting: 2\n1\nliftoff\n");
    // what is shown is what it printed, and nothing of its `()`
    assert_eq!(out.display("m", src, OutputStyle::PLAIN), "counting: 2\n1\nliftoff");
    // with no signature, its effects are inferred, and they are the console's
    let out = accepts("main () = putStrLn \"hi\"\n");
    assert_eq!((out.ran_main, out.printed.as_str()), (true, "hi\n"));
}

#[test]
fn main_is_a_function_of_unit_with_the_runtimes_effects() {
    let wrong = |ty: &str| format!("main has the wrong type: main is {ty}");
    rejects("main : Int\nmain = 3\n", &[&wrong("Int")]);
    rejects("main : () -> Int\nmain () = 3\n", &[&wrong("() -> Int")]);
    rejects(&format!("{ASK}main () = printInt (ask ())\n"), &[&wrong("() -> () ! {Console, Ask}")]);
}

#[test]
fn a_handler_of_ones_own_can_take_the_console() {
    // counting the lines, and passing each on to the runtime
    let src = "greet : () -> () ! Console\ngreet () = let u = putStrLn \"hello\" in putStrLn \"world\"\n\n\
               handle greet () with\n  \
                 return _ -> 0\n  \
                 putStr s k -> let u = putStr s in k ()\n  \
                 putStrLn s k -> let u = putStrLn s in 1 + k ()\n  \
                 printInt n k -> let u = printInt n in k ()\n  \
                 readLine _ k -> k Nothing\n";
    let out = accepts(src);
    assert_eq!(out.printed, "hello\nworld\n");
    assert_eq!(out.evaluation, Some(Evaluation::Value("2".into())));
}

// ---------------------------------------------------------------------------
// The rest of the world
// ---------------------------------------------------------------------------

/// A path in a directory of this test's own, written as Idyll would.
fn scratch(test: &str) -> String {
    let dir = std::env::temp_dir().join(format!("idyllic-{test}-{}", std::process::id()));
    dir.to_string_lossy().replace('\\', "/")
}

#[test]
fn the_runtime_does_the_worlds_operations() {
    let value = |src: &str| match accepts(src).evaluation {
        Some(Evaluation::Value(v)) => v,
        other => panic!("{src}\n{other:?}"),
    };
    assert_eq!(value("let n = intBetween 1 7 in n < 7\n"), "True");
    assert_eq!(value("exists \"no/such/file\"\n"), "False");
    // a test's world has no input, and no arguments
    assert_eq!(value("readLine ()\n"), "Nothing");
    assert_eq!(value("args ()\n"), "Nil");
    assert_eq!(value("let u = setEnv \"IDYLLIC_TEST_VAR\" \"set\" in getEnv \"IDYLLIC_TEST_VAR\"\n"), "Just \"set\"");
    let t = "let a = monotonic () in let u = sleep 5 in let b = monotonic () in b - a < 5000000\n";
    assert_eq!(value(t), "False");
}

#[test]
fn files_are_written_read_and_listed() {
    let dir = scratch("fs");
    let src = format!(
        "main : () -> () ! Eff\n\
         main () =\n  \
           let a = createDirAll \"{dir}\"\n      \
               b = writeString \"{dir}/a.txt\" \"first\"\n      \
               c = appendString \"{dir}/a.txt\" \" line\"\n  \
           in case readToString \"{dir}/a.txt\" of\n       \
                Ok s -> putStrLn s\n       \
                Err e -> fail e\n"
    );
    let out = accepts(&src);
    assert_eq!(out.printed, "first line\n");
    let listed = format!(
        "case readDir \"{dir}\" of\n  Ok names -> names\n  Err e -> #[e]\n"
    );
    assert_eq!(accepts(&listed).evaluation, Some(Evaluation::Value("\"a.txt\" :: Nil".into())));
    let gone = format!("let r = removeDirAll \"{dir}\" in exists \"{dir}\"\n");
    assert_eq!(accepts(&gone).evaluation, Some(Evaluation::Value("False".into())));
}

#[test]
fn exit_and_a_failed_test_end_the_program() {
    let out = accepts("main () = let u = putStrLn \"bye\" in exit 3\n");
    assert_eq!((out.printed.as_str(), &out.evaluation), ("bye\n", &Some(Evaluation::Exited(3))));
    let out = accepts("main () = fail \"two is not three\"\n");
    assert_eq!(out.evaluation, Some(Evaluation::Stopped("test failed: two is not three".into())));
}

#[test]
fn a_handler_of_ones_own_can_take_one_of_the_worlds_effects() {
    let src = "dice : () -> Int ! Random\ndice () = intBetween 1 7\n\n\
               handle dice () + dice () with\n  \
                 nextInt _ k -> k 4\n  \
                 intBetween lo hi k -> k ((lo + hi) / 2)\n  \
                 nextSeed _ k -> k 0\n";
    assert_eq!(accepts(src).evaluation, Some(Evaluation::Value("8".into())));
}
