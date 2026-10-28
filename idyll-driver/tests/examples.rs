//! The packages in `examples/`: each builds, and each that runs on the
//! machine prints what it should. (`euler`'s sieve runs past the machine's
//! fuel, so only its building is checked here; its Problem 5 runs in
//! `compiler/idyllic/tests/matching.rs`.)

mod common;

use common::said;
use idyll::{Profile, pipeline};
use std::path::{Path, PathBuf};

/// A copy of the example `name` in a directory of this test's own, so that
/// building it leaves nothing in the source tree.
fn example(name: &str, who: &str) -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples").join(name);
    let dir = std::env::temp_dir().join(format!("idyll-example-{}-{who}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(&from, &dir);
    dir
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let path = e.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "target") {
                copy(&path, &to.join(e.file_name()));
            }
        } else {
            std::fs::copy(&path, to.join(e.file_name())).unwrap();
        }
    }
}

/// What the example `name` printed, run on the machine.
fn printed(name: &str) -> String {
    let dir = example(name, "run");
    common::on_big_stack(|| {
        let out = pipeline::build(&dir, Profile::Debug, &common::OPTS);
        assert!(said(&out).is_empty(), "{name}: {:?}", said(&out));
        common::printed(&out).replace("\r\n", "\n")
    })
}

#[test]
fn every_example_builds() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples");
    let mut names: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .filter(|e| e.path().join("Idyll.toml").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(names.len() >= 6, "only found {names:?}");
    for name in names {
        let dir = example(&name, "build");
        let (errors, runnable) = common::on_big_stack(|| {
            let out = pipeline::build(&dir, Profile::Debug, &common::OPTS);
            (said(&out), out.runnable())
        });
        assert!(errors.is_empty(), "{name}: {errors:?}");
        assert!(runnable, "{name} has nothing to run");
    }
}

#[test]
fn hello_greets_and_counts_down() {
    assert_eq!(printed("hello"), "Hello, world!\n3\n2\n1\nliftoff!\n");
}

#[test]
fn effects_are_handled_each_their_own_way() {
    let out = printed("effects");
    for line in [
        "  log: 6 is even: halve it",
        "  8 steps",
        "From 3, the log kept: 7 steps, 7 messages",
        "From 27, the log thrown away: 111 steps",
        "1. apples\n2. pears\n3. plums",
        "mean of 1 2 3 4: Ok 2",
        "mean of nothing: Err (the mean of no numbers)",
        "Fibonacci below 100: 0 1 1 2 3 5 8 13 21 34 55 89",
        "Right triangles with sides up to 20: 3-4-5 5-12-13 6-8-10 8-15-17 9-12-15 12-16-20",
    ] {
        assert!(out.contains(line), "no {line:?} in:\n{out}");
    }
}

#[test]
fn vectors_multiply_matrices_of_the_right_sizes() {
    assert_eq!(
        printed("vectors"),
        "a times b:\n58 64\n139 154\nb transposed:\n7 9 11\n8 10 12\n\
         primes at 3: 7\nprimes at 9: out of range\ndouble 21: 42\n"
    );
}

#[test]
fn linear_sorts_in_place_and_redeems_its_tickets() {
    assert_eq!(
        printed("linear"),
        "square 7: 49\nidentity: 42\nfirstOr: 2\nsorted in place: 3 4 5 9 15 26 31 58\nseats: 42\n"
    );
}

#[test]
fn the_interpreter_runs_its_programs() {
    let out = printed("interpreter");
    for line in [
        "  = 7, in 5 steps",
        "  = 3628800, in 96 steps",
        "  = 55, in 1986 steps",
        "  = 21, in 19 steps",
        "  = error: division by zero",
        "  = error: 3 is not a function",
        "  (let x0 = (\\x0 -> (\\x1 -> (x0 (x0 x1)))) in ((x0 (\\x1 -> (x1 + 10))) 1))",
        "  does not read: unexpected end",
        "  does not read: y is bound nowhere",
    ] {
        assert!(out.contains(line), "no {line:?} in:\n{out}");
    }
}

#[test]
fn the_tour_runs_as_meadows_does() {
    let out = printed("tour");
    for line in [
        "a triangle with area 6.0",
        "degenerate: #[Just(\"a rectangle\")]",
        "distance from (0, 0) to (3, 4): 5.0",
        "factorial 30 as an Int, wrapping: -8764578968847253504",
        "factorial 5 as a UInt8: 120",
        "200 + 100 as a UInt8: 44",
        "primes below 50: #[2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47]",
        "\"the\" appears 3 times, \"cat\" 1",
    ] {
        assert!(out.contains(line), "no {line:?} in:\n{out}");
    }
}

/// What an example prints, run with these arguments.
fn printed_with(name: &str, args: &[&str]) -> String {
    let dir = example(name, "run");
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    common::on_big_stack(|| {
        let out = pipeline::build(&dir, Profile::Debug, &common::OPTS);
        assert!(said(&out).is_empty(), "{name}: {:?}", said(&out));
        let ran = idyllic::pipeline::run_compiled(&out.packages(), &common::OPTS, args);
        assert!(ran.diagnostics.is_empty(), "{:?}", ran.diagnostics);
        ran.printed.replace("\r\n", "\n")
    })
}

#[test]
fn rock_paper_scissors_plays_a_game_fed_its_moves() {
    // `--demo`: the moves given by `withInput`, the machine's seeded, so the
    // game is the same every time, with nothing typed
    let out = printed_with("rps", &["--demo"]);
    for line in [
        "your move (rock / paper / scissors, or quit): I played rock. It's a tie.",
        "I do not know what that is: nonsense",
        "I played rock. You win!",
        "You won 1, lost 0, drew 1.",
    ] {
        assert!(out.contains(line), "no {line:?} in:\n{out}");
    }
}

#[test]
fn miniml_types_and_runs_its_programs() {
    let out = printed_with("miniml", &["--demo"]);
    for line in [
        "> 7 : Int",
        "> 3628800 : Int",
        "> 1 : Int",
        "> 42 : Int",
        "> <function> : (t0 -> t0)",
        "> type error: cannot unify Bool with Int",
        "> type error: occurs check: a type would contain itself",
        "> type error: unbound variable: x",
        "> run-time error: division by zero",
        "> parse error: at offset 3: unexpected end of input",
    ] {
        assert!(out.contains(line), "no {line:?} in:\n{out}");
    }
}
