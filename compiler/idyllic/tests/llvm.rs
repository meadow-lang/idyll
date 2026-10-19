//! The LLVM backend: every program here is compiled to a native executable
//! and must print what the AxCut machine computed.  That is a strong test for
//! its size — the two share only the AxCut they are given, so the machine in
//! `idyllic-seq/src/machine.rs` is the model and the compiled code has to
//! agree with it, reference counts and all.
//!
//! Needs `clang` (with `lld`) on the path; without one the native half of
//! each test is skipped, and only the IR is generated.

use idyllic_llvm as llvm;
use idyllic_diagnostics::OutputStyle;
use idyllic::pipeline::{self, Evaluation, Options};
use std::path::PathBuf;
use std::process::Command;

fn clang_is_installed() -> bool {
    Command::new("clang").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// Compile `src` and run it; `None` if there is no clang to do it with.
fn native(name: &str, src: &str) -> Option<String> {
    let out = pipeline::run(src, &Options { debug: false, style: OutputStyle::PLAIN, live: false });
    assert!(out.diagnostics.is_empty(), "{src}\n{:#?}", out.diagnostics);
    let program = out.native.as_ref().expect("it was lowered");
    // inlining must not change what the program means, or its discipline:
    // the machine runs the inlined program too, to the same answer, and
    // finds nothing leaked
    if let Some(Evaluation::Value(expected)) = &out.evaluation {
        let inlined = idyllic_seq::machine::run(program, pipeline::FUEL).unwrap_or_else(|e| panic!("{src}\ninlined: {e}"));
        assert_eq!(inlined.answer.as_ref(), Some(expected), "{src}\n{program}");
        assert_eq!((inlined.memory.leaked, inlined.memory.miscounted), (0, 0), "{src}\n{program}");
    }
    let ir = llvm::emit(program, out.result_kind).expect("it can be compiled");
    if !clang_is_installed() {
        eprintln!("clang is not installed: {name} was not compiled");
        return None;
    }
    let dir: PathBuf = std::env::temp_dir().join(format!("idyllic-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (ll, rt, exe) = (dir.join("p.ll"), dir.join("rt.c"), dir.join("p.exe"));
    std::fs::write(&ll, ir).unwrap();
    std::fs::write(&rt, llvm::RUNTIME).unwrap();
    let built = Command::new("clang")
        .args(llvm::clang_arguments())
        .args([&ll, &rt])
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(built.status.success(), "clang failed:\n{}", String::from_utf8_lossy(&built.stderr));
    let ran = Command::new(&exe).output().unwrap();
    assert!(ran.status.success(), "{name} crashed: {:?}", ran.status);
    let _ = std::fs::remove_dir_all(&dir);
    // (Windows writes a newline as `\r\n`)
    Some(String::from_utf8_lossy(&ran.stdout).replace("\r\n", "\n").trim().to_string())
}

#[test]
fn std_compiles_natively() {
    // what a program uses of the standard library is compiled with it
    agree("std", "use Std.Collections.List (sum, map, reverse)\n\nsum (map (\\(x : Int) -> x * 2) (reverse #[1, 2, 3]))");
    agree("stdnat", "use Std.Num.Nat (fromInt, toInt, times)\n\ntoInt (times (fromInt 6) (fromInt 7))");
    // arrays, mutable arrays, and the vectors made of them
    agree("arrays", "Std.Collections.Array.sum (Std.Collections.Array.map (\\(x : [] Int) -> x * x) (Std.Collections.Array.range 0 100))");
    agree("marrays", "Std.Collections.Array.sum (Std.Collections.MArray.generate 5 0 (\\(i : [] Int) -> i * i))");
    agree(
        "vectors",
        "use Std.Collections.Vector (Vector, pushBack, pushFront, empty, range, foldl, get, sum, popBack, popFront)\n\n\
         grow : (v : [] Vector Int) -> (x : [] Int) -> Vector Int\ngrow v x = pushFront (pushBack v x) x\n\n\
         sum (popFront (popBack (foldl grow empty (range 0 1000))))",
    );
}

/// The machine's answer and the compiled program's must be the same.

fn agree(name: &str, src: &str) {
    let out = pipeline::run(src, &Options { debug: false, style: OutputStyle::PLAIN, live: false });
    let Some(Evaluation::Value(expected)) = out.evaluation else { panic!("{src}\nthe machine gave {:?}", out.evaluation) };
    if let Some(got) = native(name, src) {
        assert_eq!(got, expected, "{src}");
    }
}

#[test]
fn arithmetic_and_literals() {
    agree("arith", "2 + 3 * 4 - 1");
    agree("bool", "1 < 2");
    agree("unit", "()");
    agree("string", "\"hello\"");
}

#[test]
fn characters_and_taking_strings_apart() {
    agree("char", "'a'");
    agree("char-escaped", "'\\''");
    agree("char-code", "charCode 'é' + charCode (charFromCode 955)");
    agree("chars", "charsToString (stringToChars \"héllo, wörld\")");
    agree("char-count", "arrayLen (stringToChars \"héllo\") + stringByteLength \"héllo\" * 10");
    agree("byte", "stringByteAt \"abc\" 2");
    agree("slice", "stringSlice \"hello\" 1 4");
    agree("slice-cut", "stringSlice \"héllo\" 0 2");
    agree("compare", "stringCompare \"apple\" \"apricot\" * 10 + stringIndexOf \"a,b,c\" \",\" 2");
    agree("char-match", "kind : (c : Char) -> Int\nkind 'a' = 1\nkind '\\n' = 2\nkind c = 3\n\nkind 'a' * 100 + kind '\\n' * 10 + kind 'z'");
}

#[test]
fn closures_and_boxes() {
    agree("closure", "(\\(x : [2] Int) -> (\\(y : Int) -> x + y + x)) 10 1");
    agree("hof", "(\\(f : Int -> Int) (x : Int) -> f x) (\\n -> n * 2) 21");
    agree("box", "let [x] = [20] in x + x + 2");
    agree("foo", "foo : Int [3] -> Int [6] -> Int [3]\nfoo [x] [y] = [x + y + y]\n\nlet [r] = foo [1] [20] in r + r + r\n");
}

#[test]
fn sharing_and_dropping() {
    // a box used twice (shared, then released once shared and once unique),
    // and a structure dropped whole
    agree("share", "@linear\n\n(\\(f : Int [2] -> Int [2] -> Int) (b : [2] Int [2]) -> f b b) (\\[x] [y] -> x + x + y + y) [4]");
    agree("drop", "(\\(b : [0..1] Int [1] [1] [1]) -> 5) [[[1]]]");
}

#[test]
fn recursion_data_and_lists() {
    agree("fib", "fib : (n : [] Int) -> Int ! Div\nfib n = if n < 2 then n else fib (n - 1) + fib (n - 2)\n\nfib 20\n");
    let lists = "sum : List Int -> Int\nsum xs = case xs of\n  Nil -> 0\n  y :: ys -> y + sum ys\n\n\
                 upto : (n : [] Int) -> List Int ! Div\nupto n = if n == 0 then Nil else n :: upto (n - 1)\n\n";
    agree("lists", &format!("{lists}sum (upto 1000)\n"));
    agree(
        "maybe",
        "data Maybe a = Nothing | Just a\n\nuse Maybe.*\n\nfromMaybe : {a : Type} -> a [0..1] -> Maybe a -> a\n\
         fromMaybe [d] m = case m of\n  Nothing -> d\n  Just x -> x\n\nfromMaybe [1] (Just 100) + fromMaybe [7] Nothing\n",
    );
}

#[test]
fn more_variables_than_registers() {
    // six arguments and a continuation: the fifth variable on travels through
    // the overflow area
    agree(
        "overflow",
        "six : (a : [] Int) -> Int -> Int -> Int -> Int -> Int -> Int ! Div\n\
         six a b c d e f = if a == 0 then b + c + d + e + f else six (a - 1) (b + 1) (c + 2) (d + 3) (e + 4) (f + 5)\n\nsix 10 0 0 0 0 0\n",
    );
}

#[test]
fn deep_recursion_has_the_stack_for_it() {
    // a million calls each waiting on the next: a frame each, on a stack
    // linked big enough for a good many more
    let src = "upto : (n : [] Int) -> List Int ! Div\nupto n = if n == 0 then Nil else n :: upto (n - 1)\n\n\
               sum : List Int -> Int\nsum xs = case xs of\n  Nil -> 0\n  y :: ys -> y + sum ys\n\nsum (upto 1000000)\n";
    if let Some(got) = native("deep", src) {
        assert_eq!(got, "500000500000");
    }
}

#[test]
fn a_tail_call_takes_no_stack() {
    // a hundred million calls between two functions: were each a frame, no
    // stack would hold them
    let src = "ping : (n : [] Int) -> Int -> Int ! Div\nping n acc = if n == 0 then acc else pong (n - 1) (acc + 1)\n\
               pong : (n : [] Int) -> Int -> Int ! Div\npong n acc = if n == 0 then acc else ping (n - 1) (acc + 2)\n\nping 100000000 0\n";
    if let Some(got) = native("tail", src) {
        assert_eq!(got, "150000000");
    }
}

#[test]
fn a_tail_call_through_a_closure_takes_no_stack() {
    let src = "loop : (f : [] (Int -> Int)) -> (n : [] Int) -> Int -> Int ! Div\n\
               loop f n acc = if n == 0 then acc else loop f (n - 1) (f acc)\n\nloop (\\x -> x + 3) 100000000 0\n";
    if let Some(got) = native("closure-tail", src) {
        assert_eq!(got, "300000000");
    }
}

#[test]
fn main_talks_to_the_console_natively() {
    // the runtime is the console's handler, in native code as in the
    // machine; and `main`'s `()` is not printed
    let src = "count : (n : [] Int) -> () ! Eff\n\
               count n = if n == 0 then putStrLn \"liftoff\" else let u = printInt n in count (n - 1)\n\n\
               main : () -> () ! Eff\nmain () = let u = putStr \"counting: \" in count 3\n";
    let out = pipeline::run(src, &Options { debug: false, style: OutputStyle::PLAIN, live: false });
    assert_eq!(out.printed, "counting: 3\n2\n1\nliftoff\n");
    if let Some(got) = native("main", src) {
        assert_eq!(got, out.printed.trim());
    }
}

#[test]
fn the_worlds_effects_natively() {
    // files, randomness, the environment and exit codes, in native code as
    // in the machine
    let dir = std::env::temp_dir().join(format!("idyllic-native-world-{}", std::process::id()));
    let dir = dir.to_string_lossy().replace('\\', "/");
    let src = format!(
        "main : () -> () ! Eff\n\
         main () =\n  \
           let a = createDirAll \"{dir}\"\n      \
               b = writeString \"{dir}/a.txt\" \"from a file\"\n      \
               c = copy \"{dir}/a.txt\" \"{dir}/b.txt\"\n      \
               d = case readToString \"{dir}/b.txt\" of\n            \
                     Ok s -> putStrLn s\n            \
                     Err e -> putStrLn e\n      \
               e = case readDir \"{dir}\" of\n            \
                     Ok names -> printAll names\n            \
                     Err e -> putStrLn e\n      \
               f = removeDirAll \"{dir}\"\n      \
               g = let n = intBetween 10 20 in if n < 20 then putStrLn \"in range\" else putStrLn \"out of range\"\n      \
               h = setEnv \"IDYLLIC_NATIVE\" \"yes\"\n  \
           in case getEnv \"IDYLLIC_NATIVE\" of\n       \
                Just v -> putStrLn v\n       \
                Nothing -> putStrLn \"unset\"\n\n\
         printAll : List String -> () ! Console\n\
         printAll #[] = ()\n\
         printAll (x :: xs) = let u = putStrLn x in printAll xs\n"
    );
    let out = pipeline::run(&src, &Options { debug: false, style: OutputStyle::PLAIN, live: false });
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
    assert_eq!(out.printed, "from a file\na.txt\nb.txt\nin range\nyes\n");
    if let Some(got) = native("world", &src) {
        assert_eq!(got, out.printed.trim());
    }
}

// ---------------------------------------------------------------------------
// Handlers: a program with one keeps its continuations on the heap, and the
// runtime keeps the handlers in force
// ---------------------------------------------------------------------------

const ASK: &str = "effect Ask where\n  ask : () -> Int\n\n";

#[test]
fn a_handler_answers_its_operations_natively() {
    let sum3 = "sum3 : () -> Int ! Ask\nsum3 u = ask () + ask () + ask ()\n\n";
    agree("handler-ask", &format!("{ASK}{sum3}handle sum3 () with\n  ask _ k -> k 10\n"));
}

#[test]
fn a_handler_that_never_resumes_natively() {
    // an exception: the continuation's grade is 0
    let fail = "effect Fail where\n  fail : () -> Int\n\n\
                safeDiv : (a : [] Int) -> (b : [] Int) -> Int ! Fail\n\
                safeDiv a b = if b == 0 then fail () else a / b\n\n";
    agree("handler-fail", &format!("{fail}handle safeDiv 7 0 with\n  return x -> x + 1\n  fail _ k -> 0\n"));
    agree("handler-ok", &format!("{fail}handle safeDiv 7 2 with\n  return x -> x + 1\n  fail _ k -> 0\n"));
}

#[test]
fn a_handler_resumes_more_than_once_natively() {
    let src = "effect Choose where\n  @many flip : () -> Bool\n\n\
               pick : () -> Int ! Choose\npick u = if flip () then 1 else 10\n\n\
               handle pick () with\n  flip _ k -> k True + k False\n";
    agree("handler-multi", src);
}

#[test]
fn state_as_a_handler_natively() {
    let src = "effect State (s : Type) where\n  get : () -> s\n  put : (x : [] s) -> ()\n\n\
               counter : () -> Int ! State Int\n\
               counter u =\n  let a = get () in\n  let b = put (a + 1) in\n  let c = get () in\n  c * 10\n\n\
               run : (s : [] Int) -> Int\n\
               run = handle counter () with\n  return x -> \\(s : [] Int) -> x\n  \
               get _ k -> \\(s : [] Int) -> k s s\n  put s2 k -> \\(s : [] Int) -> k () s2\n\n\
               run 4\n";
    agree("handler-state", src);
}

#[test]
fn handlers_nest_natively() {
    // an operation passes over a handler that does not take it, which goes
    // into the resumption and comes back when it is resumed
    let src = "effect Log where\n  log : Int -> ()\n\n\
               twice : (f : [] (() -> Int ! e)) -> Int ! e\ntwice f = f () + f ()\n\n\
               logged : () -> Int ! {Ask, Log}\nlogged u = let x = log 1 in ask ()\n\n\
               inner : () -> Int ! Log\ninner u = handle twice logged with\n  ask _ k -> let y = log 100 in k 5\n\n\
               handle inner () with\n  return x -> x\n  log n k -> n + k ()\n";
    agree("handler-nested", &format!("{ASK}{src}"));
}

#[test]
fn a_handler_beside_the_worlds_effects_natively() {
    // the program's own handler, and the console's, which is the runtime
    let src = format!(
        "{ASK}greet : () -> () ! {{Ask, Console}}\ngreet u = let n = ask () in printInt n\n\n\
         main : () -> () ! Eff\nmain () = handle greet () with\n  ask _ k -> let u = putStrLn \"asked\" in k 42\n"
    );
    let out = pipeline::run(&src, &Options { debug: false, style: OutputStyle::PLAIN, live: false });
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
    assert_eq!(out.printed, "asked\n42\n");
    if let Some(got) = native("handler-world", &src) {
        assert_eq!(got, out.printed.trim());
    }
}

#[test]
fn a_long_run_under_a_handler_takes_no_stack() {
    // every call is a tail call once continuations are on the heap, and a
    // resumption's blocks are reused
    let src = format!(
        "{ASK}total : (n : [] Int) -> (acc : [] Int) -> Int ! {{Ask, Div}}\n\
         total n acc = if n == 0 then acc else total (n - 1) (acc + ask ())\n\n\
         handle total 1000000 0 with\n  ask _ k -> k 2\n"
    );
    // (longer than the machine's fuel lasts)
    if let Some(got) = native("handler-long", &src) {
        assert_eq!(got, "2000000");
    }
}

#[test]
fn abort_natively() {
    // the message on standard error, and exit status 1, after what was printed
    let src = "main : () -> () ! Eff\nmain () = let u = putStrLn \"before\" in let v = putErr \"note\\n\" in abort \"gave up\"\n";
    let out = pipeline::run(src, &Options { debug: false, style: OutputStyle::PLAIN, live: false });
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
    assert_eq!((out.printed.as_str(), out.errors.as_str()), ("before\n", "note\ngave up\n"));
    if !clang_is_installed() {
        return;
    }
    let ir = llvm::emit(out.native.as_ref().expect("lowered"), out.result_kind).expect("it can be compiled");
    let dir: PathBuf = std::env::temp_dir().join(format!("idyllic-test-{}-abort", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (ll, rt, exe) = (dir.join("p.ll"), dir.join("rt.c"), dir.join("p.exe"));
    std::fs::write(&ll, ir).unwrap();
    std::fs::write(&rt, llvm::RUNTIME).unwrap();
    let built = Command::new("clang").args(llvm::clang_arguments()).args([&ll, &rt]).arg("-o").arg(&exe).output().unwrap();
    assert!(built.status.success(), "clang failed:\n{}", String::from_utf8_lossy(&built.stderr));
    let ran = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(ran.status.code(), Some(1));
    let text = |b: &[u8]| String::from_utf8_lossy(b).replace("\r\n", "\n");
    assert_eq!((text(&ran.stdout), text(&ran.stderr)), ("before\n".to_string(), "note\ngave up\n".to_string()));
}
