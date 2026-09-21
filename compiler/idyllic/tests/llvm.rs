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
    let out = pipeline::run(src, &Options { debug: false, style: OutputStyle::PLAIN });
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
    Some(String::from_utf8_lossy(&ran.stdout).trim().to_string())
}

/// The machine's answer and the compiled program's must be the same.
fn agree(name: &str, src: &str) {
    let out = pipeline::run(src, &Options { debug: false, style: OutputStyle::PLAIN });
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
    agree("share", "(\\(f : Int [2] -> Int [2] -> Int) (b : [2] Int [2]) -> f b b) (\\[x] [y] -> x + x + y + y) [4]");
    agree("drop", "(\\(b : [0..1] Int [1] [1] [1]) -> 5) [[[1]]]");
}

#[test]
fn recursion_data_and_lists() {
    agree("fib", "fib : (n : [] Int) -> Int\nfib n = if n < 2 then n else fib (n - 1) + fib (n - 2)\n\nfib 20\n");
    let lists = "sum : List Int -> Int\nsum xs = case xs of\n  Nil -> 0\n  y :: ys -> y + sum ys\n\n\
                 upto : (n : [] Int) -> List Int\nupto n = if n == 0 then Nil else n :: upto (n - 1)\n\n";
    agree("lists", &format!("{lists}sum (upto 1000)\n"));
    agree(
        "maybe",
        "data Maybe a = Nothing | Just a\n\nfromMaybe : {a : Type} -> a [0..1] -> Maybe a -> a\n\
         fromMaybe [d] m = case m of\n  Nothing -> d\n  Just x -> x\n\nfromMaybe [1] (Just 100) + fromMaybe [7] Nothing\n",
    );
}

#[test]
fn more_variables_than_registers() {
    // six arguments and a continuation: the fifth variable on travels through
    // the overflow area
    agree(
        "overflow",
        "six : (a : [] Int) -> Int -> Int -> Int -> Int -> Int -> Int\n\
         six a b c d e f = if a == 0 then b + c + d + e + f else six (a - 1) (b + 1) (c + 2) (d + 3) (e + 4) (f + 5)\n\nsix 10 0 0 0 0 0\n",
    );
}

#[test]
fn deep_recursion_has_the_stack_for_it() {
    // a million calls each waiting on the next: a frame each, on a stack
    // linked big enough for a good many more
    let src = "upto : (n : [] Int) -> List Int\nupto n = if n == 0 then Nil else n :: upto (n - 1)\n\n\
               sum : List Int -> Int\nsum xs = case xs of\n  Nil -> 0\n  y :: ys -> y + sum ys\n\nsum (upto 1000000)\n";
    if let Some(got) = native("deep", src) {
        assert_eq!(got, "500000500000");
    }
}

#[test]
fn a_tail_call_takes_no_stack() {
    // a hundred million calls between two functions: were each a frame, no
    // stack would hold them
    let src = "ping : (n : [] Int) -> Int -> Int\nping n acc = if n == 0 then acc else pong (n - 1) (acc + 1)\n\
               pong : (n : [] Int) -> Int -> Int\npong n acc = if n == 0 then acc else ping (n - 1) (acc + 2)\n\nping 100000000 0\n";
    if let Some(got) = native("tail", src) {
        assert_eq!(got, "150000000");
    }
}

#[test]
fn a_tail_call_through_a_closure_takes_no_stack() {
    let src = "loop : (f : [] (Int -> Int)) -> (n : [] Int) -> Int -> Int\n\
               loop f n acc = if n == 0 then acc else loop f (n - 1) (f acc)\n\nloop (\\x -> x + 3) 100000000 0\n";
    if let Some(got) = native("closure-tail", src) {
        assert_eq!(got, "300000000");
    }
}
