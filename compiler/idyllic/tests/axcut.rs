//! Lowering to AxCut and running it (Schuster, Müller, Ostermann &
//! Brachthäuser, "Compiling Classical Sequent Calculus to Stock Hardware",
//! OOPSLA 2025).
//!
//! The machine checks AxCut's ordering discipline at every step — that `let`
//! and `new` find their operands at the end of the environment, that `invoke`
//! and `jump` leave nothing behind — so every program that runs here is also
//! a test that the explicit substitutions were inferred correctly.

use idyllic_seq::machine::{self, MachineError};
use idyllic_seq::{Branch, Definition, Extern, Program, Stmt, Symbol};
use idyllic_diagnostics::OutputStyle;
use idyllic::pipeline::{self, Evaluation, Options};

fn run(src: &str) -> pipeline::Output {
    pipeline::run(src, &Options { debug: false, style: OutputStyle::PLAIN })
}

/// The program's value, as the REPL shows it.
fn value(src: &str) -> String {
    let out = run(src);
    assert!(out.diagnostics.is_empty(), "{src}\nunexpected diagnostics: {:#?}", out.diagnostics);
    match out.evaluation {
        Some(Evaluation::Value(v)) => v,
        other => panic!("{src}\nexpected a value, got {other:?}"),
    }
}

fn evaluation(src: &str) -> Option<Evaluation> {
    run(src).evaluation
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

#[test]
fn literals() {
    assert_eq!(value("42"), "42");
    assert_eq!(value("\"hi\\n\""), "\"hi\\n\"");
    assert_eq!(value("True"), "True");
    assert_eq!(value("()"), "()");
}

#[test]
fn arithmetic() {
    assert_eq!(value("1 + 2 * 3"), "7");
    assert_eq!(value("(1 + 2) * 3"), "9");
    assert_eq!(value("99999999999999999999 * 99999999999999999999"), "9999999999999999999800000000000000000001");
}

#[test]
fn functions_are_called_by_value() {
    assert_eq!(value("(\\(x : Int) -> x + 1) 41"), "42");
    assert_eq!(value("(\\(f : Int -> Int) (x : Int) -> f x) (\\n -> n * 2) 21"), "42");
    // closures capture their environment
    assert_eq!(value("(\\(x : [2] Int) -> (\\(y : Int) -> x + y + x)) 10 1"), "21");
}

#[test]
fn let_bindings_and_definitions() {
    assert_eq!(value("let x = 2; y = x * x in y + 1"), "5");
    assert_eq!(value("double : Int [2] -> Int\ndouble [n] = n + n\n\ndouble [21]\n"), "42");
    // used before it is defined
    assert_eq!(value("two = one + one\none = 1\n\ntwo\n"), "2");
}

#[test]
fn boxes() {
    assert_eq!(value("[3] : Int [7]"), "[3]");
    assert_eq!(value("let [x] = [20] in x + x + 2"), "42");
    assert_eq!(value("foo : Int [3] -> Int [6] -> Int [3]\nfoo [x] [y] = [x + y + y]\n\nfoo [1] [20]\n"), "[41]");
    assert_eq!(value("[[1]] : Int [2] [3]"), "[[1]]");
}

#[test]
fn implicit_and_erased_arguments_are_passed_and_ignored() {
    let id = "id : {a : Type} -> a -> a\nid x = x\n\n";
    assert_eq!(value(&format!("{id}id (id 7)\n")), "7");
    assert_eq!(value("(\\(a : [0] Type) (x : a) -> x) Int 3"), "3");
    // a grade is erased too
    let app = "app : {a b : Type} -> {r : Grade} -> (a [r] -> b) -> a [r] -> b\napp f [x] = f [x]\n\n\
               copy : Int [2] -> Int\ncopy [x] = x + x\n\n";
    assert_eq!(value(&format!("{app}app copy [4]\n")), "8");
}

#[test]
fn a_hole_the_checker_filled_in_is_run_as_what_it_was_filled_with() {
    let out = run("let id (a : [0] Type) (x : a) : a = x in id _ 3");
    assert_eq!(out.evaluation, Some(Evaluation::Value("3".into())));
}

#[test]
fn functions_and_types_have_nothing_to_show() {
    assert_eq!(evaluation("\\(x : Int) -> x"), Some(Evaluation::Opaque));
    assert_eq!(evaluation("Int [3]"), Some(Evaluation::Opaque));
    assert_eq!(evaluation("[\\(x : Int) -> x] : (Int -> Int) [2]"), Some(Evaluation::Opaque));
}

#[test]
fn display_puts_the_value_before_the_type() {
    let show = |src: &str| run(src).display("test", src, OutputStyle::PLAIN);
    assert_eq!(show("1 + 2"), "3 : Int");
    assert_eq!(show("\\(x : Int) -> x"), ": (x : Int) -> Int");
}

// ---------------------------------------------------------------------------
// What is not run
// ---------------------------------------------------------------------------

#[test]
fn a_program_with_errors_is_not_run() {
    assert_eq!(evaluation("1 + True"), None);
    assert_eq!(evaluation("\\(x : Int) -> x + x"), None);
    assert_eq!(evaluation("1 +"), None);
}

#[test]
fn declarations_alone_are_not_run() {
    assert_eq!(evaluation("one = 1\n"), None);
}

// ---------------------------------------------------------------------------
// Recursion
// ---------------------------------------------------------------------------

#[test]
fn recursion_becomes_a_label_and_runs_out_of_fuel() {
    // there are no conditionals yet, so a recursive call never ends
    let src = "loop : Int -> Int\nloop n = loop (n + 1)\n\nloop 0\n";
    let out = run(src);
    assert_eq!(out.evaluation, Some(Evaluation::OutOfFuel));
    let program = out.axcut.expect("it was lowered");
    let labels: Vec<&str> = program.definitions.iter().map(|d| d.label.as_str()).collect();
    assert_eq!(labels.len(), 2, "{labels:?}");
    assert!(labels[1].starts_with("loop_"), "{labels:?}");
    assert!(program.to_string().contains(&format!("jump {}", labels[1])));
}

#[test]
fn a_recursive_definition_that_is_not_called_does_not_run() {
    assert_eq!(value("loop : Int -> Int\nloop n = loop n\n\n7\n"), "7");
    // nor one that is only mentioned
    assert_eq!(value("let loop : Int -> Int = \\n -> loop n in let f = loop in 7"), "7");
}

#[test]
fn a_nested_recursive_definition_is_lifted_over_what_it_captures() {
    let src = "\\(x : [] Int) -> let loop : Int -> Int = \\n -> loop (n + x) in loop 1";
    let out = run(src);
    assert_eq!(out.evaluation, Some(Evaluation::Opaque));
    let program = out.axcut.expect("it was lowered").to_string();
    // the label takes the captured `x` (and no continuation: it never returns)
    assert!(program.contains("define loop_"), "{program}");
    let header = program.lines().find(|l| l.starts_with("define loop_")).unwrap();
    assert!(header.contains("(x_") && !header.contains("k_"), "{header}");
    // and applied, it loops
    assert_eq!(evaluation(&format!("({src}) 5")), Some(Evaluation::OutOfFuel));
}

#[test]
fn mutual_recursion() {
    let src = "ping : Int -> Int\nping n = pong (n + 1)\npong : Int -> Int\npong n = ping n\n\nping 0\n";
    let out = run(src);
    assert_eq!(out.evaluation, Some(Evaluation::OutOfFuel));
    assert_eq!(out.axcut.unwrap().definitions.len(), 3);
}

// ---------------------------------------------------------------------------
// The shape of the output
// ---------------------------------------------------------------------------

#[test]
fn a_small_program_in_full() {
    let out = run("(\\(x : Int) -> ([x] : Int [1])) 5");
    assert_eq!(out.evaluation, Some(Evaluation::Value("[5]".into())));
    let expected = "\
define main : () =
  new f_0 = (){
    apply(x_1, k_2) ⇒
      substitute [k_2 ↦ k_2, x_1 ↦ x_1];
      let box_3 = box(x_1);
      substitute [box_3 ↦ box_3, k_2 ↦ k_2];
      invoke k_2 ret };
  extern lit 5 { (n_4) ⇒
  new k_5 = (){
    ret(r_6) ⇒
      extern halt(r_6) {} };
  substitute [n_4 ↦ n_4, k_5 ↦ k_5, f_0 ↦ f_0];
  invoke f_0 apply }";
    assert_eq!(out.axcut.unwrap().to_string(), expected);
}

#[test]
fn a_variable_used_again_after_being_consumed_is_duplicated() {
    // `b` goes into a box and is then needed again, so the box gets a copy
    let program = run("(\\(f : Int [1] -> Int [1] -> Int) (b : [2] Int) -> f [b] [b]) (\\[x] [y] -> x + y) 4");
    assert_eq!(program.evaluation, Some(Evaluation::Value("8".into())));
    let text = program.axcut.unwrap().to_string();
    assert!(text.contains("' ↦ b_"), "expected a copy of b:\n{text}");
}

#[test]
fn a_tail_call_passes_its_continuation_along() {
    // `f x` is in tail position: no new continuation is made for it
    let out = run("(\\(f : Int -> Int) (x : Int) -> f x) (\\n -> n) 1");
    let text = out.axcut.unwrap().to_string();
    let news = text.matches("ret(").count();
    // one continuation for each of the two outer applications, none inside
    assert_eq!(news, 2, "{text}");
}

// ---------------------------------------------------------------------------
// The machine polices the ordering discipline
// ---------------------------------------------------------------------------

fn halt(v: &str) -> Stmt {
    Stmt::Extern(Extern::Halt, vec![v.into()], Vec::new())
}

fn lit(n: i32, v: &str, rest: Stmt) -> Stmt {
    Stmt::Extern(Extern::LitInt(n.into()), Vec::new(), vec![Branch { params: vec![v.into()], body: rest }])
}

fn main_only(body: Stmt) -> Program {
    Program::new(vec![Definition { label: "main".into(), params: Vec::new(), body }])
}

#[test]
fn let_takes_its_fields_from_the_end_of_the_environment() {
    let boxed = |field: &str| {
        let rest = Stmt::Switch("b".into(), vec![(Symbol::Box, Branch { params: vec!["x".into()], body: halt("x") })]);
        main_only(lit(1, "a", lit(2, "c", Stmt::Let("b".into(), Symbol::Box, vec![field.into()], Box::new(rest)))))
    };
    // `c` is last, so it can be boxed
    assert_eq!(machine::run(&boxed("c"), 100).unwrap().answer.as_deref(), Some("2"));
    // `a` is not, and needs a substitution first
    let Err(MachineError::Malformed(why)) = machine::run(&boxed("a"), 100) else { panic!("it ran") };
    assert_eq!(why, "let needs (a) at the end, not (c)");
}

#[test]
fn substitute_reorders_duplicates_and_drops() {
    let pairs = vec![("c".to_string(), "c".to_string()), ("a2".to_string(), "a".to_string()), ("a".to_string(), "a".to_string())];
    let add = Stmt::Extern(Extern::Add, vec!["a".into(), "a2".into()], vec![Branch { params: vec!["r".into()], body: halt("r") }]);
    let program = main_only(lit(20, "a", lit(1, "b", lit(2, "c", Stmt::Substitute(pairs, Box::new(add))))));
    assert_eq!(machine::run(&program, 100).unwrap().answer.as_deref(), Some("40"));
}

#[test]
fn the_machine_stops_when_the_fuel_does() {
    let program = main_only(Stmt::Jump("main".into(), Vec::new()));
    assert_eq!(machine::run(&program, 1000).err(), Some(MachineError::OutOfFuel));
}

// ---------------------------------------------------------------------------
// Everything else we have
// ---------------------------------------------------------------------------

/// Every program in the corpora (`tests/corpus/`) that type checks must lower to AxCut
/// that obeys the ordering discipline, and run to an end (or out of fuel).
#[test]
fn every_well_typed_corpus_program_runs() {
    let corpora = [
        include_str!("corpus/handwritten.txt"),
        include_str!("corpus/exhaustive.txt"),
        include_str!("corpus/tutorial.txt"),
    ];
    let (mut ran, mut failures) = (0, Vec::new());
    for corpus in corpora {
        for src in corpus.replace("\r\n", "\n").split("\n%%%\n") {
            match evaluation(src) {
                None => {}
                Some(Evaluation::Failed(why)) => failures.push(format!("{src}\n  {why}")),
                Some(_) => ran += 1,
            }
        }
    }
    assert!(failures.is_empty(), "{} programs could not be run:\n\n{}", failures.len(), failures.join("\n\n"));
    // (most of the corpus predates linearity, and no longer type checks)
    assert!(ran > 50, "only {ran} programs ran; is the corpus being read?");
}

// ---------------------------------------------------------------------------
// Erasure: what has grade 0 is not computed
// ---------------------------------------------------------------------------

const LOOP: &str = "loop : Int -> Int\nloop n = loop (n + 1)\n\n";

#[test]
fn an_argument_to_an_erased_parameter_is_not_computed() {
    // `loop 0` never returns, and is never asked to
    assert_eq!(value(&format!("{LOOP}(\\(n : [0] Int) -> 5) (loop 0)\n")), "5");
    // at grade 1 it is
    assert_eq!(evaluation(&format!("{LOOP}(\\(n : Int) -> n) (loop 0)\n")), Some(Evaluation::OutOfFuel));
}

#[test]
fn a_box_nobody_may_open_is_not_filled() {
    assert_eq!(value(&format!("{LOOP}let [x] = ([loop 0] : Int [0]) in 5\n")), "5");
}

#[test]
fn implicit_arguments_are_erased_not_passed_as_computed_types() {
    let out = run("id : {a : Type} -> a -> a\nid x = x\n\nid 7\n");
    assert_eq!(out.evaluation, Some(Evaluation::Value("7".into())));
    // the type argument is a token with no fields, so it costs no memory
    assert!(out.axcut.unwrap().to_string().contains("= erased()"));
}

// ---------------------------------------------------------------------------
// Grades, held against the lowered code
// ---------------------------------------------------------------------------

#[test]
fn the_lowered_program_carries_the_binders_grades() {
    use idyllic_typecheck::grade::Grade;
    let program = run("\\(x : [2] Int) (y : Int) (b : Int [3]) -> let [z] = b in x + x + y + z + z + z").axcut.unwrap();
    let grade_of = |hint: &str| {
        let found: Vec<Grade> =
            program.grades.iter().filter(|(v, _)| v.starts_with(hint)).map(|(_, g)| *g).collect();
        assert_eq!(found.len(), 1, "{hint}: {found:?}");
        found[0]
    };
    assert_eq!(grade_of("x_"), Grade::exact(2));
    assert_eq!(grade_of("y_"), Grade::ONE);
    assert_eq!(grade_of("z_"), Grade::exact(3));
    assert!(idyllic_seq::grades::check(&program).is_empty());
}

#[test]
fn using_a_variable_more_than_its_grade_allows_is_caught() {
    use idyllic_typecheck::grade::Grade;
    // what the type checker would have to get wrong: x is linear, and used twice
    let add = Stmt::Extern(Extern::Add, vec!["x".into(), "x".into()], vec![Branch { params: vec!["r".into()], body: halt("r") }]);
    let mut program = main_only(lit(1, "x", add));
    program.grades.insert("x".into(), Grade::ONE);
    let violations = idyllic_seq::grades::check(&program);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].to_string(), "x has grade 1, but the lowered program uses it 2 times");
    // at grade 2 it is fine
    program.grades.insert("x".into(), Grade::exact(2));
    assert!(idyllic_seq::grades::check(&program).is_empty());
}

#[test]
fn a_recursive_definition_captures_only_what_it_uses() {
    // `y` is linear and in scope where `loop` is defined, but `loop` does not
    // mention it: passing it along would copy it at every reference
    let out = run("\\(y : Int) (x : [] Int) -> let loop : Int -> Int = \\n -> loop (n + x) in loop y");
    assert_eq!(out.evaluation, Some(Evaluation::Opaque));
    let program = out.axcut.unwrap().to_string();
    let header = program.lines().find(|l| l.starts_with("define loop_")).unwrap();
    assert!(header.contains("x_") && !header.contains("y_"), "{header}");
}

// ---------------------------------------------------------------------------
// Memory: reference counting as in the paper, and no collector
// ---------------------------------------------------------------------------

fn memory(src: &str) -> machine::Memory {
    let out = run(src);
    assert!(out.diagnostics.is_empty(), "{src}\n{:#?}", out.diagnostics);
    assert!(!matches!(out.evaluation, Some(Evaluation::Failed(_))), "{src}\n{:?}", out.evaluation);
    out.memory.unwrap_or_else(|| panic!("{src}\ndid not run to the end"))
}

#[test]
fn a_linear_program_never_touches_a_reference_count() {
    for src in [
        "foo : Int [1] -> Int [1] -> Int\nfoo [x] [y] = x + y\n\nfoo [1] [20]\n",
        "(\\(f : Int -> Int) (x : Int) -> f x) (\\n -> n + 1) 1",
        "flip : {a b c : Type} -> (a -> b -> c) -> b -> a -> c\nflip f y x = f x y\n\nflip (\\(n : Int) (m : Int) -> n * m) 2 3\n",
    ] {
        let m = memory(src);
        assert!(m.was_linear(), "{src}\n{m}");
        assert!(m.acquired() > 0 && m.unique_releases > 0, "{src}\n{m}");
    }
}

#[test]
fn the_memory_reports_the_tutorial_quotes() {
    let linear = memory("foo : Int [1] -> Int [1] -> Int\nfoo [x] [y] = x + y\n\nfoo [1] [20]\n");
    assert_eq!(
        linear.to_string(),
        "memory: 3 blocks acquired (2 fresh, 1 reused), at most 2 at once; 0 shares, 0 erases, 3 of 3 releases unique"
    );
    let shared = memory("(\\(f : Int [2] -> Int [2] -> Int) (b : [2] Int [2]) -> f b b) (\\[x] [y] -> x + x + y + y) [4]");
    assert_eq!(
        shared.to_string(),
        "memory: 4 blocks acquired (3 fresh, 1 reused), at most 3 at once; 1 shares, 0 erases, 4 of 5 releases unique"
    );
}

#[test]
fn linear_code_reuses_its_blocks() {
    // each call's continuation is freed by the return, in time for the next
    let m = memory("id : {a : Type} -> a -> a\nid x = x\n\nid (id (id (id (id 7))))\n");
    assert_eq!(m.fresh, 1, "{m}");
    assert_eq!(m.peak, 1, "{m}");
    assert!(m.reused >= 5, "{m}");
}

#[test]
fn a_shared_value_is_counted_and_released_once_shared_once_unique() {
    // `b` is used twice, so its box is shared once; the first `[x]` finds it
    // shared, the second finds it unique
    let m = memory("(\\(f : Int [2] -> Int [2] -> Int) (b : [2] Int [2]) -> f b b) (\\[x] [y] -> x + x + y + y) [4]");
    assert_eq!(m.shares, 1, "{m}");
    assert_eq!(m.shared_releases, 1, "{m}");
    assert!(!m.was_linear());
}

#[test]
fn dropping_a_structure_is_one_erase_however_big() {
    // three nested blocks die with one decrement; the rest is left for when
    // the memory is wanted
    let m = memory("(\\(b : [0..1] Int [1] [1] [1]) -> 5) [[[1]]]");
    assert_eq!(m.erases, 1, "{m}");
    assert_eq!(m.shares, 0, "{m}");
}

#[test]
fn dead_blocks_are_reused_lazily() {
    // the dropped structure's blocks are what the later boxes are made from
    let m = memory("(\\(b : [0..1] Int [1] [1] [1]) (c : Int) -> ([[[c]]] : Int [1] [1] [1])) [[[1]]] 2");
    assert!(m.reused_lazily > 0, "{m}");
}

#[test]
fn the_answer_may_be_a_structure_and_nothing_else_survives() {
    // the audit at the end of every run: no leaks, no wrong counts
    let m = memory("(\\(b : Int [1]) -> ([b] : Int [1] [1])) [3]");
    assert_eq!((m.leaked, m.miscounted), (0, 0), "{m}");
}
