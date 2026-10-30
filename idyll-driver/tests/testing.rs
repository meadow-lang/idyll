//! `idyll test`: a package's `@test` definitions, run by `Std.Test`'s
//! runner, each in turn, one failing not stopping the rest.

mod common;

use common::said;
use idyll::{Profile, pipeline};
use idyllic::pipeline::{Evaluation, TestSelection};
use std::path::PathBuf;

/// A package of this test's own: a root module with two tests, one of which
/// fails, and a module with one of its own.
fn package(who: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idyll-test-{}-{who}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    common::write(&dir.join("Idyll.toml"), "[package]\nname = \"Tested\"\nversion = \"0.1.0\"\n");
    common::write(
        &dir.join("src/Main.idl"),
        "use Std.Test (assertEq)\nuse Maths (double)\n\n\
         @test\naddsUp : () -> () ! Test\naddsUp u = assertEq (1 + 2) 3 \"sums\"\n\n\
         @test\nbreaks : () -> () ! Test\nbreaks u = assertEq (double 2) 5 \"doubling\"\n\n\
         main : () -> () ! Eff\nmain u = putStrLn \"not a test\"\n",
    );
    common::write(
        &dir.join("src/Maths.idl"),
        "use Std.Test (assertEq)\n\n@pub\ndouble : (n : Int) -> Int\ndouble n = n * 2\n\n\
         @test\ndoubles : () -> () ! Test\ndoubles u = assertEq (double 21) 42 \"forty-two\"\n",
    );
    dir
}

/// What running the tests `selection` picks printed, and how it ended.
fn tested(dir: &PathBuf, selection: &TestSelection) -> (String, Option<Evaluation>) {
    common::on_big_stack(|| {
        let out = pipeline::build_tests(dir, Profile::Debug, &common::OPTS, selection);
        assert!(said(&out).is_empty(), "{:?}", said(&out));
        let ran = idyllic::pipeline::run_compiled(&out.packages(), &common::OPTS, Vec::new());
        (ran.printed.replace("\r\n", "\n"), ran.evaluation)
    })
}

#[test]
fn every_test_runs_and_a_failure_says_why() {
    let (printed, ended) = tested(&package("all"), &TestSelection::default());
    assert_eq!(
        printed,
        "running 3 tests\ntest addsUp ... ok\ntest breaks ... FAILED\n    doubling: expected 5, got 4\ntest Maths.doubles ... ok\n\n\
         test result: FAILED. 2 passed; 1 failed\n"
    );
    assert_eq!(ended, Some(Evaluation::Exited(1)));
}

#[test]
fn tests_are_picked_by_their_names() {
    let dir = package("filter");
    // a part of the name, `Module.test`
    let some = TestSelection { filter: Some("doubl".into()), exact: false };
    let (printed, ended) = tested(&dir, &some);
    assert_eq!(printed, "running 1 test\ntest Maths.doubles ... ok\n\ntest result: ok. 1 passed; 0 failed\n");
    assert_ne!(ended, Some(Evaluation::Exited(1)));
    // or the whole of it
    let exact = TestSelection { filter: Some("addsUp".into()), exact: true };
    assert!(tested(&dir, &exact).0.contains("running 1 test\ntest addsUp ... ok"));
    let none = TestSelection { filter: Some("adds".into()), exact: true };
    assert!(tested(&dir, &none).0.starts_with("running 0 tests"));
}

#[test]
fn a_build_runs_main_and_not_the_tests() {
    let dir = package("main");
    let printed = common::on_big_stack(|| common::printed(&pipeline::build(&dir, Profile::Debug, &common::OPTS)));
    assert_eq!(printed, "not a test\n");
}

#[test]
fn a_test_is_a_unit_function_as_main_is() {
    let dir = package("type");
    common::write(&dir.join("src/Maths.idl"), "@test\nwrong : (n : Int) -> Int\nwrong n = n\n");
    let said = common::on_big_stack(|| said(&pipeline::build_tests(&dir, Profile::Debug, &common::OPTS, &TestSelection::default())));
    assert!(said.iter().any(|m| m.contains("a test has the wrong type")), "{said:?}");
}
