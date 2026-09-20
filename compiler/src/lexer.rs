//! Lexing, via [logos].  The lexical grammar itself is the `#[token]` /
//! `#[regex]` attributes on [`Token`]; this module is the driver and the
//! callbacks that decode literal payloads.
//!
//! The lexer is *infallible*: malformed input never aborts.  Anything logos
//! cannot match becomes a [`Token::Error`] with a diagnostic, and literals
//! whose bodies are malformed (a bad escape, an unterminated string) are
//! reported with a diagnostic pointing at the exact offending bytes while
//! lexing carries on.  Callbacks report through the lexer's [`Extras`].

use crate::diagnostics::Diagnostic;
use crate::span::{Span, Spanned};
use crate::token::Token;
use logos::{Lexer, Logos};
use num_bigint::BigInt;

/// Diagnostics accumulated by callbacks while lexing.
pub type Extras = Vec<Diagnostic>;

/// Tokens plus whatever went wrong.  Never fails.
pub struct LexOutput {
    pub tokens: Vec<Spanned<Token>>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn tokenize(src: &str) -> LexOutput {
    let mut lex = Token::lexer(src);
    let mut tokens = Vec::new();
    while let Some(result) = lex.next() {
        let span = Span::from(lex.span());
        // the only way logos fails is on input no pattern matches
        let token = result.unwrap_or_else(|()| {
            lex.extras.push(
                Diagnostic::error(format!("unexpected character '{}'", lex.slice()))
                    .label(span, "not valid at the start of a token"),
            );
            Token::Error
        });
        tokens.push(Spanned::new(token, span));
    }
    LexOutput { tokens, diagnostics: lex.extras }
}

// ---------------------------------------------------------------------------
// Callbacks
// ---------------------------------------------------------------------------

type Lex<'s> = Lexer<'s, Token>;

pub(crate) fn text(lex: &mut Lex) -> String {
    lex.slice().to_string()
}

/// Integers are arbitrary precision, so this cannot overflow; the regex has
/// already guaranteed every digit is valid for the radix.
fn int(lex: &mut Lex, prefix: usize, radix: u32) -> BigInt {
    BigInt::parse_bytes(&lex.slice().as_bytes()[prefix..], radix)
        .expect("the lexical grammar only admits valid digits")
}

pub(crate) fn binary(lex: &mut Lex) -> BigInt {
    int(lex, 2, 2)
}

pub(crate) fn octal(lex: &mut Lex) -> BigInt {
    int(lex, 2, 8)
}

pub(crate) fn hexadecimal(lex: &mut Lex) -> BigInt {
    int(lex, 2, 16)
}

pub(crate) fn decimal(lex: &mut Lex) -> BigInt {
    int(lex, 0, 10)
}

/// The literal's content with the surrounding quotes stripped and escapes
/// resolved.
fn literal_body(lex: &mut Lex) -> String {
    let text = lex.slice();
    let inner = &text[1..text.len() - 1];
    resolve_escapes(inner, lex.span().start + 1, &mut lex.extras)
}

pub(crate) fn string(lex: &mut Lex) -> String {
    literal_body(lex)
}

pub(crate) fn character(lex: &mut Lex) -> char {
    let span = Span::from(lex.span());
    let resolved = literal_body(lex);
    let mut chars = resolved.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => c,
        (None, _) => {
            lex.extras.push(
                Diagnostic::error("empty character literal")
                    .label(span, "a character literal must contain exactly one codepoint"),
            );
            '\u{FFFD}'
        }
        (Some(c), Some(_)) => {
            lex.extras.push(
                Diagnostic::error("character literal contains more than one codepoint")
                    .label(span, "expected exactly one codepoint between the quotes")
                    .help("use a string literal (\"...\") for multi-character text"),
            );
            c
        }
    }
}

pub(crate) fn unterminated_string(lex: &mut Lex) {
    let span = Span::from(lex.span());
    lex.extras.push(
        Diagnostic::error("unterminated string literal").label(span, "missing the closing double quote"),
    );
}

pub(crate) fn unterminated_char(lex: &mut Lex) {
    let span = Span::from(lex.span());
    lex.extras.push(
        Diagnostic::error("unterminated character literal")
            .label(span, "missing the closing single quote"),
    );
}

pub(crate) fn empty_char(lex: &mut Lex) {
    let span = Span::from(lex.span());
    lex.extras.push(
        Diagnostic::error("empty character literal").label(span, "expected a codepoint between the quotes"),
    );
}

/// Resolve backslash escapes in literal content, reporting every malformed
/// escape at its exact position and recovering by keeping the offending text.
///
/// `base` is the byte offset of `content` in the source, so diagnostics can
/// point at the escape rather than the whole literal.
fn resolve_escapes(content: &str, base: usize, diags: &mut Vec<Diagnostic>) -> String {
    let mut out = String::with_capacity(content.len());
    let mut iter = content.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some((_, e)) = iter.next() else {
            diags.push(Diagnostic::error("trailing backslash in literal").label(
                Span::new(base + i, base + i + 1),
                "expected an escape character after this backslash",
            ));
            break;
        };
        match e {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            '\\' => out.push('\\'),
            '\'' => out.push('\''),
            '"' => out.push('"'),
            '0' => out.push('\0'),
            'a' => out.push('\x07'),
            'b' => out.push('\x08'),
            'f' => out.push('\x0c'),
            'v' => out.push('\x0b'),
            'x' | 'u' | 'U' => {
                let max_digits = match e {
                    'x' => 2,
                    'u' => 4,
                    _ => 8,
                };
                let mut value: u32 = 0;
                let mut n = 0;
                while n < max_digits {
                    match iter.peek() {
                        Some(&(_, d)) if d.is_ascii_hexdigit() => {
                            value = value.wrapping_mul(16).wrapping_add(d.to_digit(16).unwrap());
                            n += 1;
                            iter.next();
                        }
                        _ => break,
                    }
                }
                let esc_start = base + i;
                if n == 0 {
                    diags.push(
                        Diagnostic::error(format!(
                            "'\\{e}' escape needs at least one hexadecimal digit"
                        ))
                        .label(
                            Span::new(esc_start, esc_start + 2),
                            "expected hexadecimal digits here",
                        ),
                    );
                } else {
                    match char::from_u32(value) {
                        Some(ch) => out.push(ch),
                        None => {
                            diags.push(
                                Diagnostic::error(format!("invalid Unicode scalar value U+{value:X}"))
                                    .label(
                                        Span::new(esc_start, esc_start + 2 + n),
                                        "not a valid Unicode codepoint",
                                    )
                                    .help("Unicode scalar values are U+0000..U+D7FF and U+E000..U+10FFFF"),
                            );
                            out.push('\u{FFFD}');
                        }
                    }
                }
            }
            other => {
                diags.push(
                    Diagnostic::error(format!("unknown escape sequence '\\{other}'"))
                        .label(
                            Span::new(base + i, base + i + 1 + other.len_utf8()),
                            "this escape sequence is not recognized",
                        )
                        .help("supported escapes: \\n \\r \\t \\0 \\a \\b \\f \\v \\\\ \\' \\\" \\xNN \\uNNNN \\UNNNNNNNN"),
                );
                out.push(other);
            }
        }
    }
    out
}
