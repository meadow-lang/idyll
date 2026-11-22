//! How operators group: `infixl 6 <+>`, as in Haskell and Meadow.
//!
//! A fixity belongs to an operator's spelling, not to one definition of it.
//! The language's own operators have theirs ([`builtin`]), the prelude's
//! pipes among them; a declaration may repeat one of those but not change
//! it. Every other operator is what a
//! declaration in the package, the prelude or a package it depends on says,
//! or `infixl 9` if none says. All of those are known before a file is
//! parsed ([`declared`] reads them off its tokens), so an operator chain is
//! grouped as it is read ([`group`]).

use std::collections::HashMap;

use idyllic_ast::{Alt, Ident, LTerm, Lit, Pat, Rhs, Term};
use idyllic_diagnostics::Diagnostic;
use idyllic_lexer::token::Token;
use idyllic_span::Spanned;

pub use idyllic_ast::{Assoc, Fixity};

/// Operators' fixities, by spelling.
pub type Fixities = HashMap<String, Fixity>;

/// What an operator nothing declares is.
pub const DEFAULT: Fixity = Fixity { assoc: Assoc::Left, level: 9 };

/// The language's own operators' fixities: `*` `/` `%` (`infixl 7`), `+`
/// `-` (`infixl 6`), `::` `++` (`infixr 5`), the comparisons (`infix 4`),
/// `and` (`infixr 3`) and `or` (`infixr 2`), and the prelude's pipes, `|>`
/// (`infixl 1`) and `<|` (`infixr 0`), which a file read alone groups as a
/// program does.
pub fn builtin(op: &str) -> Option<Fixity> {
    Some(match op {
        "|>" => Fixity::new(Assoc::Left, 1),
        "<|" => Fixity::new(Assoc::Right, 0),
        "or" => Fixity::new(Assoc::Right, 2),
        "and" => Fixity::new(Assoc::Right, 3),
        "*" | "/" | "%" => Fixity::new(Assoc::Left, 7),
        "+" | "-" => Fixity::new(Assoc::Left, 6),
        "::" | "++" => Fixity::new(Assoc::Right, 5),
        "==" | "!=" | "<" | ">" | "<=" | ">=" => Fixity::new(Assoc::None, 4),
        _ => return None,
    })
}

/// The fixity of `op`, given the declared ones.
pub fn of(op: &str, fixities: &Fixities) -> Fixity {
    builtin(op).or_else(|| fixities.get(op).copied()).unwrap_or(DEFAULT)
}

/// The keyword a fixity declaration starts with, and the way it groups: it
/// is a keyword only before a number, and an ordinary name anywhere else.
pub fn keyword(word: &str) -> Option<Assoc> {
    match word {
        "infixl" => Some(Assoc::Left),
        "infixr" => Some(Assoc::Right),
        "infix" => Some(Assoc::None),
        _ => None,
    }
}

/// The fixity declarations in a file's tokens (laid out): each operator
/// declared, where, and its fixity. A declaration that is not well formed
/// is the parser's to report; this takes what it can.
pub fn declared(tokens: &[Spanned<Token>]) -> Vec<(Ident, Fixity)> {
    let toks: Vec<&Spanned<Token>> = tokens.iter().filter(|t| t.node != Token::Newline).collect();
    let mut out = Vec::new();
    for i in 0..toks.len() {
        let Token::LowerIdent(word) = &toks[i].node else { continue };
        let Some(assoc) = keyword(word) else { continue };
        // where an item starts
        let starts = i == 0 || matches!(toks[i - 1].node, Token::VLBrace | Token::VSemi | Token::LBrace | Token::Semi);
        let level = match toks.get(i + 1).map(|t| &t.node) {
            Some(Token::Int(n)) => u8::try_from(n.clone()).ok().filter(|l| *l <= 9),
            _ => None,
        };
        let (true, Some(level)) = (starts, level) else { continue };
        let mut j = i + 2;
        loop {
            let op = match (toks.get(j).map(|t| &t.node), toks.get(j + 1).map(|t| &t.node), toks.get(j + 2).map(|t| &t.node)) {
                (Some(Token::OpIdent(s) | Token::ConOpIdent(s) | Token::BacktickIdent(s)), ..) => {
                    j += 1;
                    Spanned::new(s.clone(), toks[j - 1].span)
                }
                (Some(Token::LParen), Some(Token::OpIdent(s) | Token::ConOpIdent(s) | Token::BacktickIdent(s)), Some(Token::RParen)) => {
                    j += 3;
                    Spanned::new(s.clone(), toks[j - 2].span)
                }
                _ => break,
            };
            out.push((op, Fixity::new(assoc, level)));
            if matches!(toks.get(j).map(|t| &t.node), Some(Token::Comma)) {
                j += 1;
            } else {
                break;
            }
        }
    }
    out
}

/// The fixities `declared` (in every file a package is read from, the
/// prelude's and those of the packages it depends on) come to, and what is
/// wrong with them: a built-in operator's changed, or two declarations of
/// one operator that disagree.
pub fn table(declared: &[(Ident, Fixity)]) -> (Fixities, Vec<Diagnostic>) {
    let mut table = Fixities::new();
    let mut first: HashMap<&str, &Ident> = HashMap::new();
    let mut errors = Vec::new();
    for (op, fixity) in declared {
        if let Some(own) = builtin(&op.node) {
            if own != *fixity {
                errors.push(
                    Diagnostic::error(format!("the fixity of `{}` cannot change", op.node))
                        .label(op.span, format!("`{}` is `{own}`, not `{fixity}`", op.node)),
                );
            }
            continue;
        }
        match table.get(&op.node) {
            Some(before) if before != fixity => {
                let mut d = Diagnostic::error(format!("two fixities for `{}`", op.node))
                    .label(op.span, format!("this says `{fixity}`, but `{before}` was declared first"));
                if let Some(at) = first.get(op.node.as_str()) {
                    d = d.label(at.span, format!("`{before}` here"));
                }
                errors.push(d);
            }
            Some(_) => {}
            None => {
                table.insert(op.node.clone(), *fixity);
                first.insert(&op.node, op);
            }
        }
    }
    (table, errors)
}

/// `a + b`: the operator applied to both, so that nothing after the parser
/// needs to know about infix syntax. `a and b` is `if a then b else False`,
/// and `a or b` is `if a then True else b`, which look at `b` only if they
/// have to.
pub fn binary(lhs: LTerm, op: Ident, rhs: LTerm) -> LTerm {
    if let "and" | "or" = op.node.as_str() {
        let span = lhs.span.merge(rhs.span);
        let truth = |b: bool| Spanned::new(Term::Lit(Lit::Bool(b)), span);
        let alt = |v: bool, body: LTerm| Alt { pat: Spanned::new(Pat::Lit(Lit::Bool(v)), span), rhs: Rhs::Plain(body) };
        let alts = if op.node == "and" { vec![alt(true, rhs), alt(false, truth(false))] } else { vec![alt(true, truth(true)), alt(false, rhs)] };
        return Spanned::new(Term::Case(Box::new(lhs), alts), span);
    }
    let op_span = op.span;
    let partial_span = lhs.span.merge(op_span);
    let span = lhs.span.merge(rhs.span);
    let op = Spanned::new(Term::Var(op), op_span);
    let partial = Spanned::new(Term::App(Box::new(op), Box::new(lhs)), partial_span);
    Spanned::new(Term::App(Box::new(partial), Box::new(rhs)), span)
}

/// A chain of operands and the operators between them, `a + b * c`,
/// grouped by the operators' fixities; or, where two operators of one level
/// do not group the same way, the second of them and why.
pub fn group(first: LTerm, rest: Vec<(Ident, LTerm)>, fixities: &Fixities) -> Result<LTerm, (Ident, String)> {
    let mut rest = rest.into_iter().peekable();
    climb(first, 0, &mut rest, fixities)
}

fn climb(
    mut lhs: LTerm,
    min: u8,
    rest: &mut std::iter::Peekable<std::vec::IntoIter<(Ident, LTerm)>>,
    fixities: &Fixities,
) -> Result<LTerm, (Ident, String)> {
    while let Some(here) = rest.peek().map(|(op, _)| of(&op.node, fixities)) {
        if here.level < min {
            break;
        }
        let (op, mut rhs) = rest.next().expect("peeked");
        while let Some((next, after)) = rest.peek().map(|(o, _)| (o.clone(), of(&o.node, fixities))) {
            if after.level > here.level {
                rhs = climb(rhs, here.level + 1, rest, fixities)?;
            } else if after.level == here.level {
                match (here.assoc, after.assoc) {
                    (Assoc::Right, Assoc::Right) => rhs = climb(rhs, here.level, rest, fixities)?,
                    (Assoc::Left, Assoc::Left) => break,
                    _ => {
                        let why = if here == after {
                            format!("`{}` and `{}` are both `{here}`, and do not chain: write parentheses", op.node, next.node)
                        } else {
                            format!(
                                "`{}` is `{here}` and `{}` is `{after}`, which do not group the same way: write parentheses",
                                op.node, next.node
                            )
                        };
                        return Err((next, why));
                    }
                }
            } else {
                break;
            }
        }
        lhs = binary(lhs, op, rhs);
    }
    Ok(lhs)
}
