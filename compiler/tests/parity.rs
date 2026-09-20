//! Parity with the original Haskell implementation.
//!
//! `fixtures/expected-*.txt` were produced by running the Haskell compiler
//! over `fixtures/corpus-*.txt` (programs separated by a `%%%` line).  For
//! every program they record the post-layout token stream, the number of
//! layout errors, the inferred type, and the headline of every diagnostic.
//! These tests reproduce that format from the Rust pipeline and demand the
//! two agree exactly.
//!
//! Expectations corrected by hand, where Haskell was wrong; everything else
//! in the fixtures is verbatim Haskell output:
//!
//! * For `"café \x41"` the Haskell lexer produced U+0004 instead of `A`,
//!   because it sliced lexemes by alex's match length (in characters) as if
//!   it were bytes, truncating any lexeme that follows a multi-byte
//!   character.  The Rust lexer slices by byte span.
//!
//! * Handwritten programs 2, 3 and 33 nest a plain `let` inside a top-level
//!   definition.  Haskell's SCC pass never descended into top-level
//!   right-hand sides, so those inner bindings kept the renamer's
//!   conservative recursive group and were wrongly told they "need a type
//!   signature".  The bogus errors are removed (and program 33, no longer
//!   failing, has type `Int`).
//!
//! * Tutorial programs 42, 43 and 46 and exhaustive programs 151, 197, 343,
//!   389, 535 and 581 begin or end with a virtual `;` that separates nothing
//!   (a leading comment or blank line, a source ending in a blank line, a
//!   stray `}` that layout discards).  Haskell's parser took exactly one term
//!   and so called that a parse error.  A program is now a `;`-separated list
//!   of declarations in which stray separators are allowed, and these parse:
//!   the tutorial ones have type `Int`, and the others get as far as `x` not
//!   being in scope.
//!
//! * Handwritten program 15 and tutorial program 46 contain a blank line.
//!   Haskell's layout pass measured the "indentation" of a newline that is
//!   followed by another newline, taking the second newline's column: 1 for
//!   a truly empty line — a spurious `;`, or the end of a `let` block — but
//!   wherever the line ends for a comment or trailing spaces, which glued the
//!   next line onto the previous one.  Blank and comment-only lines now mean
//!   nothing; the stray `;` and newline tokens are removed from those two
//!   expectations.
//!
//! The layout algorithm in particular is subtle and deliberately quirky (see
//! `src/layout.rs`), so the exhaustive corpus enumerates every one- and
//! two-line program over a layout-relevant alphabet at several indents.
//!
//! **Grades.**  The Haskell implementation had no grades; every binder was
//! unrestricted.  Binders are now linear by default, so an ungraded program
//! can newly fail — but *only* with `usage mismatch`.  That is the invariant
//! checked here: with those errors set aside, the tokens, layout, parse
//! result, inferred type and every other diagnostic must still be exactly
//! what Haskell produced.  Grading adds a check; it changes nothing else.

use idyllic::diagnostics::{OutputStyle, Severity};
use idyllic::pipeline::{self, Options};
use idyllic::span::LineIndex;
use idyllic::{layout, lexer};
use std::fmt::Write;

fn render(index: usize, src: &str) -> String {
    let mut out = String::new();
    writeln!(out, "=== {index}").unwrap();

    let li = LineIndex::new(src);
    let laid_out = layout::layout(&li, &lexer::tokenize(src).tokens);
    for t in &laid_out.tokens {
        writeln!(out, "tok {}@{}..{}", t.node, t.span.start, t.span.end).unwrap();
    }
    writeln!(out, "layout-errors {}", laid_out.errors.len()).unwrap();

    let result = pipeline::run(src, &Options { debug: false, style: OutputStyle::PLAIN });
    writeln!(out, "type {}", result.body_type.as_deref().unwrap_or("NONE")).unwrap();
    // see the module docs: usage errors are the one thing grading adds
    for d in result.diagnostics.iter().filter(|d| d.message != "usage mismatch") {
        let kind = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        writeln!(out, "diag [{kind}]: {}", d.message).unwrap();
    }
    out
}

fn check(corpus: &str, expected: &str) {
    let corpus = corpus.replace("\r\n", "\n");
    let expected = expected.replace("\r\n", "\n");
    let expected: Vec<&str> = expected.split("=== ").skip(1).collect();
    let programs: Vec<&str> = corpus.split("\n%%%\n").collect();
    assert_eq!(programs.len(), expected.len(), "corpus and expectations are out of step");

    let mut failures = Vec::new();
    for (i, (src, want)) in programs.iter().zip(&expected).enumerate() {
        let got = render(i, src);
        let want = format!("=== {want}");
        if got != want {
            failures.push(format!("--- program {i} ---\n{src}\n--- haskell ---\n{want}--- rust ---\n{got}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} programs differ from the Haskell implementation:\n\n{}",
        failures.len(),
        programs.len(),
        failures.iter().take(5).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn handwritten_corpus_matches_haskell() {
    check(
        include_str!("fixtures/corpus-handwritten.txt"),
        include_str!("fixtures/expected-handwritten.txt"),
    );
}

#[test]
fn exhaustive_layout_corpus_matches_haskell() {
    check(
        include_str!("fixtures/corpus-exhaustive.txt"),
        include_str!("fixtures/expected-exhaustive.txt"),
    );
}

/// Every example in `TUTORIAL.md`, so the tutorial stays true.
#[test]
fn tutorial_examples_match_haskell() {
    check(
        include_str!("fixtures/corpus-tutorial.txt"),
        include_str!("fixtures/expected-tutorial.txt"),
    );
}
