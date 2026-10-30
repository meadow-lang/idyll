//! Lexer behaviour worth pinning down on its own: tie-breaking between
//! keywords / punctuation and the generic patterns, and CRLF line endings.

use idyllic_lexer::tokenize;
use idyllic_span::Span;
use idyllic_lexer::token::Token::{self, *};

fn kinds(src: &str) -> Vec<Token> {
    tokenize(src).tokens.into_iter().map(|t| t.node).collect()
}

fn lower(s: &str) -> Token {
    LowerIdent(s.to_string())
}

#[test]
fn keywords_win_ties_but_longer_identifiers_win_on_length() {
    assert_eq!(kinds("let letter in inner"), [Let, lower("letter"), In, lower("inner")]);
    assert_eq!(kinds("Type Types type"), [Type, UpperIdent("Types".into()), Type]);
    assert_eq!(kinds("_ _x x'"), [Underscore, lower("_x"), lower("x'")]);
}

#[test]
fn punctuation_wins_ties_but_longer_operators_win_on_length() {
    assert_eq!(kinds("-> ->> = == | ||"), [
        RArrow,
        OpIdent("->>".into()),
        Eq,
        OpIdent("==".into()),
        Bar,
        OpIdent("||".into()),
    ]);
    assert_eq!(kinds(r"\x \>"), [BackSlash, lower("x"), OpIdent(r"\>".into())]);
}

#[test]
fn colon_headed_operators_are_constructor_operators() {
    assert_eq!(kinds(": :: :+: +:"), [
        Colon,
        ConOpIdent("::".into()),
        ConOpIdent(":+:".into()),
        OpIdent("+:".into()),
    ]);
}

#[test]
fn integer_radixes() {
    assert_eq!(kinds("0 42 0b1010 0o17 0xFf"), [0, 42, 10, 15, 255].map(|n| Int(n.into())));
}

#[test]
fn integers_are_arbitrary_precision() {
    let src = "999999999999999999999999999999999999999999";
    let out = tokenize(src);
    assert_eq!(out.tokens[0].node, Int(src.parse().unwrap()));
    assert!(out.diagnostics.is_empty());
}

#[test]
fn spans_are_byte_offsets() {
    let out = tokenize("let  x");
    let spans: Vec<Span> = out.tokens.iter().map(|t| t.span).collect();
    assert_eq!(spans, [Span::new(0, 3), Span::new(5, 6)]);
}

#[test]
fn escapes_resolve_and_bad_ones_point_at_themselves() {
    let out = tokenize(r#""a\n\x41\q""#);
    assert_eq!(out.tokens[0].node, Str("a\nAq".into()));
    assert_eq!(out.diagnostics.len(), 1);
    // the `\q`, not the whole literal
    assert_eq!(out.diagnostics[0].labels[0].span, Span::new(8, 10));
}

#[test]
fn malformed_literals_recover() {
    for (src, msg) in [
        ("\"abc", "unterminated string literal"),
        ("'a", "unterminated character literal"),
        ("'", "unterminated character literal"),
        ("''", "empty character literal"),
    ] {
        let out = tokenize(src);
        assert_eq!(out.tokens[0].node, Error, "{src}");
        assert_eq!(out.diagnostics[0].message, msg, "{src}");
    }
    assert_eq!(kinds(r"'a' '\n' '\''"), [Char('a'), Char('\n'), Char('\'')]);
}

#[test]
fn unexpected_characters_become_error_tokens_and_lexing_continues() {
    let out = tokenize("x \u{2714} y");
    assert_eq!(kinds("x \u{2714} y"), [lower("x"), Error, lower("y")]);
    assert_eq!(out.diagnostics[0].message, "unexpected character '\u{2714}'");
}

#[test]
fn crlf_is_a_single_newline() {
    assert_eq!(kinds("a\r\nb\nc\rd"), [
        lower("a"),
        Newline,
        lower("b"),
        Newline,
        lower("c"),
        Newline,
        lower("d"),
    ]);
}

#[test]
fn comments_and_whitespace_are_skipped() {
    assert_eq!(kinds("x -- comment ( { let\ny"), [lower("x"), Newline, lower("y")]);
}

#[test]
fn a_string_with_holes_is_its_pieces_and_the_holes_tokens() {
    // as Meadow's lexer reads one: the text before each hole, between them
    // and after the last, each unescaped, and each hole lexed as code
    assert_eq!(kinds(r#""a ${x + 1} b ${f "c"} d""#), [
        InterpStart("a ".into()),
        lower("x"),
        OpIdent("+".into()),
        Int(1.into()),
        InterpMid(" b ".into()),
        lower("f"),
        Str("c".into()),
        InterpEnd(" d".into()),
    ]);
    // `:?` after a hole is a token of its own, which the parser reads
    assert_eq!(kinds(r#""${x:?}""#), [InterpStart(String::new()), lower("x"), ConOpIdent(":?".into()), InterpEnd(String::new())]);
    // the pieces' spans cover their quote, `${` and `}`
    let spans: Vec<Span> = tokenize(r#""a ${x} b""#).tokens.into_iter().map(|t| t.span).collect();
    assert_eq!(spans, [Span::new(0, 5), Span::new(5, 6), Span::new(6, 10)]);
    // `\$` is a dollar; a string with no hole is a plain one
    assert_eq!(kinds(r#""\${x}""#), [Str("${x}".into())]);
    // a hole with nothing in it, or never closed, says so
    assert!(!tokenize(r#""${}""#).diagnostics.is_empty());
    assert!(!tokenize(r#""a ${x"#).diagnostics.is_empty());
}
