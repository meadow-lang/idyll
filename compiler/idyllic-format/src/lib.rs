//! How Idyll is set out: `idyll fmt`, within 100 columns, a level two spaces.
//!
//! Idyll's layout is part of its meaning: a block's items start at its
//! column, a line further right goes on with the line before. So a block's
//! items each start a line of their own, after what opens the block or under
//! it a level in; a body that does not fit after its `=` or `->` goes under it
//! a level in, and guards a line each. What formatting changes is the space
//! between tokens, and nothing else: the tokens, comments among them, are the
//! source's, in its order, and one blank line is kept where there were any. A
//! line that goes on with the one before keeps how far further in it was.
//!
//! A signature that fits on one line stays on one; one that does not is set
//! out an entry a line, its name alone, the colon three spaces in and each
//! arrow two, so that every type starts in one column (ROADMAP's principles).
//!
//! front/'s `Idyll fmt` is the same formatter, written with Lingua's `format!`
//! over front/'s grammar. This is that, in Rust: a lossless tree ([`tree`]),
//! the rules and what lays them out ([`style`]), and Wadler's printer
//! ([`doc`]). Each says what it follows, and the two are compared on every
//! program of the corpus.

mod doc;
mod style;
mod tree;

use idyllic_diagnostics::Diagnostic;
use idyllic_span::{LineIndex, Span};

/// `src` set out, or what is wrong with it: a program with errors is not set
/// out, as what it means may not be what it says.
pub fn format(src: &str) -> Result<String, Vec<Diagnostic>> {
    let lexed = idyllic_lexer::tokenize(src);
    let laid = idyllic_layout::layout(&LineIndex::new(src), &lexed.tokens);
    let mut problems = lexed.diagnostics.clone();
    problems.extend(laid.errors.iter().map(|e| e.to_diagnostic()));
    if !problems.is_empty() {
        return Err(problems);
    }
    idyllic_parser::parse(src.len(), &laid.tokens)?;
    match tree::read(src, &lexed, &laid) {
        Ok(tree) => Ok(style::layout(src, &tree)),
        Err(at) => Err(vec![Diagnostic::error("this cannot be set out").label(
            Span::at(at),
            "the parser reads this, but the formatter's grammar has no rule for it",
        )]),
    }
}

#[cfg(test)]
mod tests {
    use super::format;

    fn set(text: &str) -> String {
        format(text).unwrap_or_else(|_| "(error)".to_string())
    }

    /// Set out, and set out again the same.
    fn once(text: &str) -> String {
        let out = set(text);
        assert_eq!(set(&out), out, "formatting twice is formatting once");
        out
    }

    #[test]
    fn a_blocks_items_are_each_a_line_at_its_column() {
        assert_eq!(once("x =\n  let a = 1\n      b = 2\n  in a + b\n"), "x =\n  let\n    a = 1\n    b = 2\n  in a + b\n");
    }

    #[test]
    fn a_block_that_fits_stays_on_a_line() {
        assert_eq!(once("x = let a = 1 in a\n"), "x = let a = 1 in a\n");
    }

    #[test]
    fn guards_are_a_line_each_a_level_in() {
        assert_eq!(once("abs n | n < 0 = 0 - n\n  | otherwise = n\n"), "abs n\n  | n < 0 = 0 - n\n  | otherwise = n\n");
    }

    #[test]
    fn comments_and_one_blank_line_are_kept() {
        assert_eq!(
            once("-- a\nx = 1\n\n\n\n-- b\ny = case x of\n  1 -> 2\n  _ -> 3  -- c\n"),
            "-- a\nx = 1\n\n-- b\ny =\n  case x of\n    1 -> 2\n    _ -> 3 -- c\n"
        );
    }

    #[test]
    fn a_declaration_block_is_under_what_opens_it() {
        assert_eq!(once("effect Ask where ask : Int\n"), "effect Ask where\n  ask : Int\n");
    }

    #[test]
    fn a_long_signature_is_an_entry_a_line() {
        let long = "compose : {a b c : Type} -> {r s : Grade} -> (f : (y : [s] b) -> c ! e) -> (g : [s] ((x : [r] a) -> b ! e)) -> (x : [s * r] a) -> c ! e\ncompose f g x = f (g x)\n";
        assert_eq!(
            once(long),
            "compose\n   : {a b c : Type}\n  -> {r s : Grade}\n  -> (f : (y : [s] b) -> c ! e)\n  -> (g : [s] ((x : [r] a) -> b ! e))\n  -> (x : [s * r] a)\n  -> c ! e\ncompose f g x = f (g x)\n"
        );
        assert_eq!(once("id : a -> a\nid x = x\n"), "id : a -> a\nid x = x\n");
    }

    #[test]
    fn a_continuation_line_keeps_its_place() {
        assert_eq!(once("x = foo 1\n  2\n"), "x =\n  foo 1\n    2\n");
    }

    #[test]
    fn a_written_semicolon_keeps_its_line() {
        assert_eq!(once("x = let a = 1; b = 2 in b\n"), "x = let a = 1; b = 2 in b\n");
    }

    #[test]
    fn an_attribute_is_a_line_of_its_own() {
        assert_eq!(once("@pub\nx = 1\n"), "@pub\nx = 1\n");
        assert_eq!(once("@pub x = 1\n"), "@pub\nx = 1\n");
        assert_eq!(once("@pub(pkg)\n@terminating\nx : Int\nx = 1\n"), "@pub(pkg)\n@terminating\nx : Int\nx = 1\n");
    }

    #[test]
    fn a_sequence_fits_or_is_an_item_a_line() {
        assert_eq!(once("x = #[\n  1,\n  2\n]\n"), "x = #[1, 2]\n");
        assert_eq!(once("x = #[]\n"), "x = #[]\n");
        let a = "a".repeat(40);
        let long = format!("x = #[\"{a}\", \"{a}\", \"{a}\"]\n");
        assert_eq!(once(&long), format!("x =\n  #[\n    \"{a}\",\n    \"{a}\",\n    \"{a}\"\n  ]\n"));
    }

    #[test]
    fn a_program_with_errors_is_left_alone() {
        assert_eq!(set("x = (1 +\n"), "(error)");
    }
}
