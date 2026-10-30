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

/// Two functions with different effects, given to one with one row for
/// both: the row is what they do between them. A function with fewer
/// effects fits where more are allowed, and a row variable is not fixed by
/// the first thing it meets.
#[test]
fn functions_with_different_effects_compose() {
    let effects = "effect Ask where\n  ask : () -> Int\n\neffect Log where\n  log : Int -> ()\n\n\
                   compose : {a b c : Type} -> {r s : Grade} -> (f : (y : [s] b) -> c ! e) -> (g : [s] ((x : [r] a) -> b ! e)) -> (x : [s * r] a) -> c ! e\n\
                   compose f g x = f (g x)\n\n";
    let named = "addAsk : Int -> Int ! Ask\naddAsk n = n + ask ()\n\nlogged : Int -> Int ! Log\nlogged n = let u = log n in n\n\n";
    // named, as they are; wrapped in lambdas; and with rows of their own
    accepts(&format!("{effects}{named}both : Int -> Int ! {{Ask, Log}}\nboth n = compose logged addAsk n\n"));
    accepts(&format!("{effects}{named}both : Int -> Int ! {{Ask, Log}}\nboth n = compose (\\y -> logged y) (\\x -> addAsk x) n\n"));
    let open = "addAsk : Int -> Int ! {Ask | e}\naddAsk n = n + ask ()\n\nlogged : Int -> Int ! {Log | e}\nlogged n = let u = log n in n\n\n";
    accepts(&format!("{effects}{open}both : Int -> Int ! {{Ask, Log}}\nboth n = compose logged addAsk n\n"));
    // with no signature, the row found is what they do, and no more
    let out = accepts(&format!("{effects}{named}both n = compose logged addAsk n\n\nboth"));
    assert_eq!(out.body_type.as_deref(), Some("(n : Int) -> Int ! {Ask, Log}"));
    // and what they do is still checked against what is allowed
    rejects(
        &format!("{effects}{named}both : Int -> Int ! Log\nboth n = compose logged addAsk n\n"),
        &["effects not allowed: this has the effects {Ask, Log}, but its type allows Log"],
    );
    // A row variable of kind `Effect` still takes no effect resumed more
    // than once, however it is filled, and says so once; one bound at
    // `Effect []` takes it.
    let choose = "effect Choose where\n  @many flip : () -> Bool\n\npick : Int -> Int ! Choose\npick n = if flip () then n else 0\n\n";
    rejects(
        &format!("{effects}{named}{choose}both : Int -> Int ! {{Ask, Choose}}\nboth n = compose pick addAsk n\n"),
        &["these effects may be resumed more than once: the implicit argument e stands for effects resumed at most once, and Choose has an operation declared @many"],
    );
    accepts(&format!(
        "{effects}{named}{choose}composeM : {{e : Effect []}} -> (f : b -> c ! e) -> (g : a -> b ! e) -> (x : a) -> c ! e\n\
         composeM f g x = f (g x)\n\nboth : Int -> Int ! {{Ask, Choose}}\nboth n = composeM pick addAsk n\n"
    ));
}

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
    let opts = Options { debug: false, style: OutputStyle::PLAIN, live: false };
    let out = pipeline::run_within(&format!("{SPIN}spin 0\n"), &opts, std::time::Duration::from_secs(2));
    assert!(matches!(out.evaluation, Some(Evaluation::TimedOut(_))), "{:?}", out.evaluation);
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
    // and nothing is looked at: no case on the proof, in the program's own
    // code (the prelude's has cases of its own)
    let cut = out.cut.expect("lowered");
    let own = &cut[cut.find("def idyll:Main/transport").expect("transport is lowered")..];
    let own = &own[..own.find("\n\n").unwrap_or(own.len())];
    assert!(!own.contains("case {"), "{own}");
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
    let src = "effect Choose where\n  @many flip : () -> Bool\n\n\
               pick : () -> Int ! Choose\npick u = if flip () then 1 else 10\n\n\
               handle pick () with\n  flip _ k -> k True + k False\n";
    assert_eq!(value(src), "11 : Int");
}

#[test]
fn only_an_operation_marked_many_is_resumed_more_than_once() {
    // unmarked, an operation is resumed at most once: once, or never
    let coin = "effect Coin where\n  toss : () -> Bool\n\n";
    rejects(
        &format!("{coin}handle (if toss () then 1 else 2) with\n  toss _ k -> k True + k False\n"),
        &["toss is resumed more than once: this clause may resume toss more than once, and Coin declares it to be resumed at most once"],
    );
    assert_eq!(value(&format!("{coin}handle (if toss () then 1 else 2) with\n  toss _ k -> k True\n")), "1 : Int");
    assert_eq!(value(&format!("{coin}handle (if toss () then 1 else 2) with\n  toss _ k -> 0\n")), "0 : Int");
    // a resumption whose use depends on the argument is at most once too
    assert_eq!(
        value(&format!("{coin}handle (if toss () then 1 else 2) with\n  toss _ k -> if False then 0 else k False\n")),
        "2 : Int"
    );
    // the mark is part of what a compiled package says of its operations
    let out = accepts("effect Choose where\n  @many flip : () -> Bool\n  pick : () -> Int\n\n");
    let ops = &out.checked.as_ref().expect("checked").ops;
    let many: Vec<(&str, bool)> = {
        let mut v: Vec<(&str, bool)> =
            ops.values().map(|o| (o.name.as_str(), o.many)).filter(|(n, _)| ["flip", "pick"].contains(n)).collect();
        v.sort();
        v
    };
    assert_eq!(many, [("flip", true), ("pick", false)]);
}

#[test]
fn a_row_variable_takes_effects_resumed_at_most_once_unless_it_says_otherwise() {
    let choose = "effect Choose where\n  @many flip : () -> Bool\n\n";
    let ask = "effect Ask where\n  ask : () -> Int\n\n";
    // an effect with an operation resumed more than once has the other kind
    let out = accepts(&format!("{choose}{ask}"));
    assert!(out.decls.contains(&("Choose".into(), "Effect []".into())), "{:?}", out.decls);
    assert!(out.decls.contains(&("Ask".into(), "Effect".into())), "{:?}", out.decls);
    // a row variable a signature binds for you is `Effect`
    let twice = "twice : (f : [] (() -> Int ! e)) -> Int ! e\ntwice f = f () + f ()\n\n";
    let out = accepts(twice);
    assert!(out.decls.contains(&("twice".into(), "{e : Effect} -> (f : () -> Int ! e) -> Int ! e".into())), "{:?}", out.decls);
    // so it takes `Ask`, and not `Choose`
    let pick_ask = "pickA : () -> Int ! Ask\npickA u = ask ()\n\n";
    assert_eq!(value(&format!("{ask}{twice}{pick_ask}handle twice pickA with\n  ask _ k -> k 5\n")), "10 : Int");
    let pick = "pick : () -> Int ! Choose\npick u = if flip () then 1 else 10\n\n";
    rejects(
        &format!("{choose}{twice}{pick}handle twice pick with\n  flip _ k -> k True + k False\n"),
        &["these effects may be resumed more than once: the implicit argument e stands for effects resumed at most once, and Choose has an operation declared @many"],
    );
    // unless it says it takes them
    let twice_many = "twiceM : {e : Effect []} -> (f : [] (() -> Int ! e)) -> Int ! e\ntwiceM f = f () + f ()\n\n";
    assert_eq!(value(&format!("{choose}{twice_many}{pick}handle twiceM pick with\n  flip _ k -> k True + k False\n")), "44 : Int");
    // and one that takes them takes the others too
    assert_eq!(value(&format!("{ask}{twice_many}{pick_ask}handle twiceM pickA with\n  ask _ k -> k 5\n")), "10 : Int");
    // an alias has its row's kind
    let out = accepts(&format!("{choose}{ask}effect Both = {{Choose, Ask}}\neffect JustAsk = {{Ask}}\n"));
    assert!(out.decls.contains(&("Both".into(), "Effect []".into())), "{:?}", out.decls);
    assert!(out.decls.contains(&("JustAsk".into(), "Effect".into())), "{:?}", out.decls);
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
    assert!(out.decls.contains(&("Stateful".into(), "(s : Type) -> Effect".into())), "{:?}", out.decls);
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
fn a_program_gives_up_with_abort() {
    // unhandled, `abort` writes its message as an error and exits with 1,
    // after whatever was printed before
    let src = "main : () -> () ! Eff\nmain () = let u = putStrLn \"before\" in abort \"gave up\"\n";
    let out = accepts(src);
    assert_eq!(out.evaluation, Some(Evaluation::Exited(1)));
    assert_eq!((out.printed.as_str(), out.errors.as_str()), ("before\n", "gave up\n"));
    // it gives any type, and a function that may give up says so
    let half = "half : (n : [] Int) -> Int ! Abort\nhalf n = if n % 2 == 0 then n / 2 else abort \"odd\"\n\n";
    let out = accepts(half);
    assert!(out.decls.contains(&("half".into(), "(n : Int) -> Int ! Abort".into())), "{:?}", out.decls);
    // and like any effect it can be handled
    assert_eq!(value(&format!("{half}handle half 7 with\n  abort m k -> let u = putErr m in 0 - 1\n")), "-1 : Int");
    assert_eq!(value(&format!("{half}handle half 8 with\n  abort m k -> let u = putErr m in 0 - 1\n")), "4 : Int");
    // `putErr` writes to standard error
    let out = accepts("main () = putErr \"careful\\n\"\n");
    assert_eq!((out.printed.as_str(), out.errors.as_str()), ("", "careful\n"));
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
    // counting what is written (`putStrLn` is the prelude's, over
    // `putStr`), and passing each on to the runtime
    let src = "greet : () -> () ! Console\ngreet () = let u = putStrLn \"hello\" in putStrLn \"world\"\n\n\
               handle greet () with\n  \
                 return _ -> 0\n  \
                 putStr s k -> let u = putStr s in 1 + k ()\n  \
                 putErr s k -> let u = putErr s in k ()\n  \
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
    // with the message, as the runtime words it
    assert!(matches!(&out.evaluation, Some(Evaluation::Stopped(why)) if why.contains("two is not three")), "{:?}", out.evaluation);
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

#[test]
fn a_constructor_does_not_hide_an_effect_of_its_name() {
    // `Div` the constructor is in scope, but in a row a name is an effect's
    let op = "data Op = Add | Div\nuse Op.*\n\n";
    assert_eq!(ty(&format!("{op}count : (n : Int) -> Int ! Div\ncount n = if n == 0 then 0 else count (n - 1)\n\ncount 3")), "Int");
    assert_eq!(ty(&format!("{op}count : (n : Int) -> Int ! {{Div, Console}}\ncount n = if n == 0 then 0 else count (n - 1)\n\ncount 3")), "Int");
    // and outside one, the constructor is what it is
    assert_eq!(ty(&format!("{op}Div")), "Op");
}

#[test]
fn a_row_variable_made_inside_a_lambda_meets_one_made_outside() {
    // the inner `map`'s row is a metavariable over `x`, the outer's is not:
    // the inner one is pruned of `x`, and the two are one
    let src = "use Std.Collections.List (map)\n\n\
               table : (xs : List Int) -> (ys : List Int) -> List (List Int)\n\
               table xs ys = map (\\(x : Int) -> map (\\(y : Int) -> x * y) ys) xs\n\n\
               table #[1, 2] #[3, 4]";
    assert_eq!(accepts(src).evaluation, Some(Evaluation::Value("(3 :: 4 :: Nil) :: (6 :: 8 :: Nil) :: Nil".into())));
    // and a resumption handed to a function polymorphic in its row: that
    // row's variable, made in the clause, is pruned of the clause's binders
    let search = "effect Search where\n  @many choose : Int -> Int -> Int\n\n\
                  each : (lo : [] Int) -> (hi : [] Int) -> (k : [] (Int -> List a ! {Div | r})) -> List a ! {Div | r}\n\
                  each lo hi k = if hi < lo then Nil else Std.Collections.List.append (k lo) (each (lo + 1) hi k)\n\n\
                  all : (body : () -> a ! {Search, Div | r}) -> List a ! {Div | r}\n\
                  all body = handle body () with\n  return x -> x :: Nil\n  choose lo hi k -> each lo hi k\n\n\
                  all (\\() -> let a = choose 1 2 in let b = choose a 2 in (a, b))";
    assert_eq!(accepts(search).evaluation, Some(Evaluation::Value("(1, 1) :: (1, 2) :: (2, 2) :: Nil".into())));
    // which also lets an argument's type be found from how it is used
    assert_eq!(ty("(\\f -> f 1) (\\n -> True)"), "Bool");
}
