//! Lexing, via [logos].  The lexical grammar itself is the `#[token]` /
//! `#[regex]` attributes on [`Token`]; this module is the driver and the
//! callbacks that decode literal payloads.
//!
//! The lexer is *infallible*: malformed input never aborts.  Anything logos
//! cannot match becomes a [`Token::Error`] with a diagnostic, and literals
//! whose bodies are malformed (a bad escape, an unterminated string) are
//! reported with a diagnostic pointing at the exact offending bytes while
//! lexing carries on.  Callbacks report through the lexer's [`Extras`].

pub mod token;

pub use token::Token;

use idyllic_diagnostics::Diagnostic;
use idyllic_span::{Span, Spanned};
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
    let mut out = LexOutput { tokens: Vec::new(), diagnostics: Vec::new() };
    lex_into(src, 0, &mut out);
    out
}

/// Lex `text`, whose first byte is at `base` in the source: the whole
/// source, and again what is inside each `${..}` of a string, so that a
/// hole holds any expression the language has (as Meadow's lexer does).
fn lex_into(text: &str, base: usize, out: &mut LexOutput) {
    let mut lex = Token::lexer(text);
    loop {
        let before = lex.extras.len();
        let Some(result) = lex.next() else { break };
        let local = lex.span();
        let span = Span::new(base + local.start, base + local.end);
        // a string with a hole in it is read by hand, from its quote: the
        // pattern stops at a quote inside a hole, and knows nothing of holes
        if text[local.start..].starts_with('"') {
            if let Some(read) = interpolated(text, local.start) {
                lex.extras.truncate(before);
                lex.bump(read.end - local.end.min(read.end));
                pieces(text, base, read, out);
                continue;
            }
        }
        // the only way logos fails is on input no pattern matches
        let token = result.unwrap_or_else(|()| {
            lex.extras.push(
                Diagnostic::error(format!("unexpected character '{}'", lex.slice()))
                    .label(span, "not valid at the start of a token"),
            );
            Token::Error
        });
        out.tokens.push(Spanned::new(token, span));
    }
    out.diagnostics.extend(lex.extras.into_iter().map(|d| d.shifted(base)));
}

/// A string read by hand: where it ends (past its closing quote, or the end
/// of the text if it never closes), and the holes in it, each from just
/// past its `${` to its `}`.
struct Interpolated {
    start: usize,
    end: usize,
    closed: bool,
    holes: Vec<(usize, usize)>,
}

/// The string starting at the quote at `start`, if it has a hole in it.
fn interpolated(text: &str, start: usize) -> Option<Interpolated> {
    let bytes = text.as_bytes();
    let mut i = start + 1;
    let mut holes = Vec::new();
    loop {
        match bytes.get(i) {
            None | Some(b'\n' | b'\r' | b'\x0c') => {
                // never closed: only ours to report if it had a hole
                return (!holes.is_empty()).then_some(Interpolated { start, end: i, closed: false, holes });
            }
            Some(b'"') => {
                return (!holes.is_empty()).then_some(Interpolated { start, end: i + 1, closed: true, holes });
            }
            Some(b'\\') => i += 1 + text[i + 1..].chars().next().map_or(0, char::len_utf8),
            Some(b'$') if bytes.get(i + 1) == Some(&b'{') => {
                let open = i + 2;
                match hole_end(text, open) {
                    Some(close) => {
                        holes.push((open, close));
                        i = close + 1;
                    }
                    // a hole never closed runs to the end of the line
                    None => {
                        holes.push((open, text.len()));
                        return Some(Interpolated { start, end: text.len(), closed: false, holes });
                    }
                }
            }
            Some(_) => i += text[i..].chars().next().map_or(1, char::len_utf8),
        }
    }
}

/// Where the hole whose text starts at `open` ends: its `}`, past any braces,
/// strings and characters inside it.
fn hole_end(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while let Some(&b) = bytes.get(i) {
        match b {
            b'\n' | b'\r' => return None,
            b'{' => depth += 1,
            b'}' if depth == 0 => return Some(i),
            b'}' => depth -= 1,
            b'"' => {
                i = string_end(text, i)?;
                continue;
            }
            // an apostrophe after a word character is part of a name (`xs'`)
            b'\'' if !(i > open && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')) => {
                let rest = &text[i + 1..];
                let n = match rest.chars().next() {
                    Some('\\') => 1 + rest[1..].chars().next().map_or(0, char::len_utf8),
                    Some(c) => c.len_utf8(),
                    None => 0,
                };
                if rest[n..].starts_with('\'') {
                    i += n + 2;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Past the string starting at the quote at `start`, holes and all.
fn string_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = start + 1;
    loop {
        match bytes.get(i)? {
            b'"' => return Some(i + 1),
            b'\n' | b'\r' => return None,
            b'\\' => i += 2,
            b'$' if bytes.get(i + 1) == Some(&b'{') => i = hole_end(text, i + 2)? + 1,
            _ => i += 1,
        }
    }
}

/// A string read by hand, as its pieces and the tokens of its holes.
fn pieces(text: &str, base: usize, read: Interpolated, out: &mut LexOutput) {
    let span = |a: usize, b: usize| Span::new(base + a, base + b);
    let n = read.holes.len();
    let mut from = read.start;
    for (k, &(open, close)) in read.holes.iter().enumerate() {
        // the text before the hole: after the quote, or the last hole's `}`
        let body = &text[from + 1..open - 2];
        let value = resolve_escapes(body, base + from + 1, &mut out.diagnostics);
        let token = if k == 0 { Token::InterpStart(value) } else { Token::InterpMid(value) };
        out.tokens.push(Spanned::new(token, span(from, open)));
        if text[open..close].trim().is_empty() {
            out.diagnostics.push(
                Diagnostic::error("`${}` needs an expression inside")
                    .label(span(open - 2, (close + 1).min(text.len())), "nothing to put in the string")
                    .help("write `\\${` for the characters themselves"),
            );
            out.tokens.push(Spanned::new(Token::Error, span(open, close)));
        } else {
            lex_into(&text[open..close], base + open, out);
        }
        from = close;
        if k + 1 == n && close >= text.len() {
            out.diagnostics.push(
                Diagnostic::error("this `${` is never closed").label(span(open - 2, close), "no closing `}`"),
            );
            return;
        }
    }
    let last_end = if read.closed { read.end - 1 } else { read.end };
    let body = &text[from + 1..last_end];
    let value = resolve_escapes(body, base + from + 1, &mut out.diagnostics);
    out.tokens.push(Spanned::new(Token::InterpEnd(value), span(from, read.end)));
    if !read.closed {
        out.diagnostics.push(
            Diagnostic::error("unterminated string literal").label(span(read.start, read.end), "missing the closing double quote"),
        );
    }
}

// ---------------------------------------------------------------------------
// Callbacks
// ---------------------------------------------------------------------------

type Lex<'s> = Lexer<'s, Token>;

pub(crate) fn text(lex: &mut Lex) -> String {
    lex.slice().to_string()
}

/// A named hole's name: what follows its `?`.
pub(crate) fn goal(lex: &mut Lex) -> String {
    lex.slice()[1..].to_string()
}

/// Read as arbitrary precision, so this cannot overflow (the parser says
/// whether it fits an `Int`); the regex has already guaranteed every digit
/// is valid for the radix.
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

pub(crate) fn float(lex: &mut Lex) -> u64 {
    lex.slice().parse::<f64>().map_or(0, f64::to_bits)
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
            '$' => out.push('$'),
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
