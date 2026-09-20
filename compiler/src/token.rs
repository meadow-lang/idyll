//! The token vocabulary the layout pass and the parser work over, and — via
//! the [`Logos`] derive — the lexical grammar that produces it.
//!
//! Tie-breaking: logos prefers the longest match, then the highest priority.
//! The generic patterns (identifiers, operators) are given `priority = 1` so
//! that keywords and specific punctuation win ties (`let`, `_`, `->`), while a
//! longer run still wins on length (`letter`, `_x`, `->>`).

use crate::lexer;
use logos::Logos;
use num_bigint::BigInt;
use std::fmt;

/// A token, with any literal payload already decoded (escapes resolved,
/// integers parsed).  The three `V*` variants are *virtual*: they are never
/// produced by the lexer, only inserted by the layout pass.
#[derive(Logos, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[logos(extras = lexer::Extras)]
#[logos(skip r"[ \t\x05\x0b]+")]
#[logos(skip r"--[^\n\r\x0c]*")]
pub enum Token {
    /// Something the lexer could not make sense of; a diagnostic has already
    /// been reported for it.  Malformed literals are recognised here so they
    /// can be reported precisely rather than as stray characters.
    #[regex(r#""([^"\\\n\r\x0c]|\\[^\n\r\x0c])*"#, lexer::unterminated_string)]
    #[regex(r"'([^'\\\n\r\x0c]|\\[^\n\r\x0c])", lexer::unterminated_char)]
    #[token("'", lexer::unterminated_char)]
    #[token("''", lexer::empty_char)]
    Error,

    /// `\r\n` is one newline, so CRLF sources lay out the same as LF ones.
    #[regex(r"\r\n|\n|\r|\x0c")]
    Newline,

    #[regex(r"[A-Z][A-Za-z0-9_']*", lexer::text, priority = 1)]
    UpperIdent(String),
    #[regex(r"[_a-z][A-Za-z0-9_']*", lexer::text, priority = 1)]
    LowerIdent(String),
    #[regex(r"[!$%&*+./<=>?@|\\~:^`\-]+", lexer::text, priority = 1)]
    OpIdent(String),
    /// A `:`-headed operator is a constructor operator; the priority breaks
    /// the tie with `OpIdent` on inputs like `::`.
    #[regex(r":[!$%&*+./<=>?@|\\~:^`\-]+", lexer::text, priority = 3)]
    ConOpIdent(String),

    #[regex(r"0b[01]+", lexer::binary)]
    #[regex(r"0o[0-7]+", lexer::octal)]
    #[regex(r"0x[0-9A-Fa-f]+", lexer::hexadecimal)]
    #[regex(r"[1-9][0-9]*|0", lexer::decimal)]
    /// Arbitrary precision.
    Int(BigInt),

    /// A backslash always pairs with the next character, so an escaped quote
    /// never terminates the literal; the escape itself is validated by the
    /// callback.
    #[regex(r#""([^"\\\n\r\x0c]|\\[^\n\r\x0c])*""#, lexer::string)]
    Str(String),
    #[regex(r"'([^'\\\n\r\x0c]|\\[^\n\r\x0c])'", lexer::character)]
    Char(char),

    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    VLBrace,
    VRBrace,
    VSemi,
    #[token("!")]
    Bang,
    #[token("#")]
    Hash,
    #[token("\\")]
    BackSlash,
    #[token(":")]
    Colon,
    #[token(";")]
    Semi,
    #[token(",")]
    Comma,
    #[token(".")]
    Period,
    #[token("=")]
    Eq,
    #[token("<-")]
    LArrow,
    #[token("->")]
    RArrow,
    #[token("=>")]
    RFatArrow,
    #[token("|")]
    Bar,
    #[token("_")]
    Underscore,
    #[token("@")]
    At,
    /// The infinite grade, as in `1..∞`.
    #[token("∞")]
    #[token("inf")]
    Infinity,
    #[token("mod")]
    Mod,
    #[token("use")]
    Use,
    #[token("data")]
    Data,
    #[token("type")]
    #[token("Type")]
    Type,
    #[token("let")]
    Let,
    #[token("in")]
    In,
    #[token("where")]
    Where,
    #[token("do")]
    Do,
    #[token("case")]
    Case,
    #[token("of")]
    Of,
    #[token("if")]
    If,
    #[token("then")]
    Then,
    #[token("else")]
    Else,
}

impl fmt::Display for Token {
    /// A short, human-facing rendering, used in parse error messages.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Token::*;
        match self {
            Error => write!(f, "<lex-error>"),
            Newline => write!(f, "<newline>"),
            UpperIdent(s) | LowerIdent(s) | OpIdent(s) | ConOpIdent(s) => write!(f, "{s}"),
            Int(n) => write!(f, "{n}"),
            Str(s) => write!(f, "{s:?}"),
            Char(c) => write!(f, "{c:?}"),
            LParen => write!(f, "("),
            RParen => write!(f, ")"),
            LBrace => write!(f, "{{"),
            RBrace => write!(f, "}}"),
            LBracket => write!(f, "["),
            RBracket => write!(f, "]"),
            VLBrace => write!(f, "{{ (virtual)"),
            VRBrace => write!(f, "}} (virtual)"),
            VSemi => write!(f, "; (virtual)"),
            Bang => write!(f, "!"),
            Hash => write!(f, "#"),
            BackSlash => write!(f, "\\"),
            Colon => write!(f, ":"),
            Semi => write!(f, ";"),
            Comma => write!(f, ","),
            Period => write!(f, "."),
            Eq => write!(f, "="),
            LArrow => write!(f, "<-"),
            RArrow => write!(f, "->"),
            RFatArrow => write!(f, "=>"),
            Bar => write!(f, "|"),
            Underscore => write!(f, "_"),
            At => write!(f, "@"),
            Infinity => write!(f, "∞"),
            Mod => write!(f, "mod"),
            Use => write!(f, "use"),
            Data => write!(f, "data"),
            Type => write!(f, "Type"),
            Let => write!(f, "let"),
            In => write!(f, "in"),
            Where => write!(f, "where"),
            Do => write!(f, "do"),
            Case => write!(f, "case"),
            Of => write!(f, "of"),
            If => write!(f, "if"),
            Then => write!(f, "then"),
            Else => write!(f, "else"),
        }
    }
}
