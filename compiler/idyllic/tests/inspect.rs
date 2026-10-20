//! What a block of code is at each stage of the compiler: the REPL's `:ir`.

use idyllic::inspect::{inspect, STAGES};
use idyllic::pipeline::Options;
use idyllic_diagnostics::OutputStyle;

const OPTS: Options = Options { debug: false, style: OutputStyle::PLAIN, live: false };

fn stage(earlier: &str, block: &str, name: &str) -> String {
    let inspection = inspect(earlier, block, &OPTS, &[name]);
    assert!(inspection.errors.is_none(), "{block}\n{:?}", inspection.errors);
    let (_, text) = inspection.stages.into_iter().find(|(n, _)| *n == name).expect("the stage");
    text.unwrap_or_else(|why| panic!("{name}: {why}"))
}

#[test]
fn every_stage_is_shown_in_order() {
    let inspection = inspect("", "double : Int -> Int\ndouble n = n * 2\n", &OPTS, &[]);
    let names: Vec<&str> = inspection.stages.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, STAGES.iter().map(|(n, _)| *n).collect::<Vec<_>>());
    assert!(inspection.stages.iter().all(|(_, t)| t.is_ok()), "{:?}", inspection.stages);
}

#[test]
fn the_stages_after_cut_are_meadows() {
    // the Cut inlined, what Meadow lowers that to, and what Silo compiles
    // that to
    let block = "double : Int -> Int\ndouble n = n * 2\n\ndouble 21";
    // `double` is small, and its call known: inlined, the entry multiplies
    let inlined = stage("", block, "inlined");
    let entry = &inlined[inlined.find("def idyll:Main/\"#entry\"").expect("the entry")..];
    assert!(entry.contains("prim mul(") && !entry.contains("idyll:Main/double("), "{inlined}");
    let axcut = stage("", block, "axcut");
    assert!(axcut.contains("-- idyll:Main/double") && axcut.contains("extern Mul("), "{axcut}");
    let llvm = stage("", block, "llvm");
    assert!(llvm.contains("define ") && llvm.contains("mul "), "{llvm}");
}

#[test]
fn the_early_stages_show_the_block_alone() {
    let block = "double : Int -> Int\ndouble n = n * 2";
    // layout's braces and semicolons, which nobody wrote
    assert_eq!(stage("", block, "tokens"), "«{» double : Int -> Int «;»\ndouble n = n * 2 «}»");
    // no spans, and a node's wrapper is what it holds
    let ast = stage("", block, "ast");
    assert!(ast.contains("Sig(\"double\", Pi(None, Explicit, None, Var(\"Int\"), Var(\"Int\")))"), "{ast}");
    assert!(!ast.contains("span"), "{ast}");
    // a name is resolved to its unique
    let hir = stage("", block, "hir");
    assert!(hir.contains("name: double#"), "{hir}");
    assert!(hir.contains("Var(Int#"), "{hir}");
}

#[test]
fn core_shows_the_definitions_and_the_expression() {
    let core = stage("", "double : Int -> Int\ndouble n = n * 2\n\ndouble 21", "core");
    assert_eq!(core, "double : (_ : [1] Int) -> Int\ndouble = \\n -> * n 2\n\ndouble 21");
    // a recursive definition is among them, a type former is not run
    let nat = "data Nat = Z | S Nat\n\nuse Nat.*\n\nplus : Nat -> Nat -> Nat\nplus Z m = m\nplus (S n) m = S (plus n m)\n";
    let core = stage("", nat, "core");
    assert!(core.contains("plus = \\x -> \\m -> case x of { Z -> m; S n -> S (plus n m) }"), "{core}");
}

#[test]
fn a_block_of_declarations_is_lowered_with_them_kept() {
    // nothing calls `double`, and it is lowered all the same
    let cut = stage("", "double : Int -> Int\ndouble n = n * 2\n", "cut");
    assert!(cut.contains("def idyll:Main/double (n_") && cut.contains("prim mul(n_"), "{cut}");
}

#[test]
fn the_sessions_declarations_are_in_scope() {
    let earlier = "double : Int -> Int\ndouble n = n * 2\n";
    let core = stage(earlier, "double 21", "core");
    assert_eq!(core, "double 21");
    // what the block uses of the session, and of the prelude, is lowered
    // with it, and nothing else
    let cut = stage(earlier, "showInt (double 21)", "cut");
    assert!(cut.contains("def idyll:Main/double"), "{cut}");
    assert!(cut.contains("def idyll:Prelude/showInt"), "{cut}");
    assert!(!cut.contains("putStrLn"), "{cut}");
}

#[test]
fn a_block_that_does_not_check_says_why_and_shows_what_it_can() {
    let inspection = inspect("", "1 + True", &OPTS, &[]);
    let errors = inspection.errors.expect("errors");
    assert!(errors.contains("Error"), "{errors}");
    let names: Vec<&str> = inspection.stages.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, vec!["tokens", "ast", "hir"]);
}
