//! Surface parser, via [chumsky].
//!
//! The grammar, loosest to tightest:
//!
//! ```text
//! program ::= item (';' item)*             -- an expression may only come last
//! item    ::= data | decl | term
//! data    ::= 'data' upperIdent tparam* '=' ctor ('|' ctor)*
//! tparam  ::= lowerIdent | '(' lowerIdent ':' term ')'
//! ctor    ::= (upperIdent | '(' conOp ')') field*      -- a field is an atom
//! decl    ::= lowerIdent ':' term                          -- signature
//!           | lowerIdent pattern* (':' term)? '=' term     -- equation
//!
//! term    ::= lambda | letBox | let | case | if | piOrArrow
//! lambda  ::= '\' pattern+ '->' term
//! letBox  ::= 'let' '[' lowerIdent ']' '=' term 'in' term
//! let     ::= 'let' decl (';' decl)* 'in' term
//! case    ::= 'case' term 'of' alt (';' alt)*
//! alt     ::= (constructor lowerIdent* | lowerIdent '::' lowerIdent) '->' term
//! if      ::= 'if' term 'then' term 'else' term    -- a case on a Bool
//! piOrArrow
//!         ::= domain+ '->' term            -- dependent: {a : Type} (x : a) -> c
//!           | annExpr ('->' term)?         -- non-dependent arrow, right assoc
//! annExpr ::= range (':' term)?
//! range   ::= cmpExpr ('..' cmpExpr)?      -- a grade interval, 0..1
//! cmpExpr ::= consExpr (('<' | '==') consExpr)?    -- does not chain
//! consExpr ::= addExpr ('::' consExpr)?    -- right assoc
//! addExpr ::= mulExpr (('+' | '-') mulExpr)*       -- left assoc
//! mulExpr ::= appExpr ('*' appExpr)*       -- left assoc
//! appExpr ::= atom atom*                   -- application, left assoc
//! atom    ::= 'Type' | '_' | '∞' | literal | ident | '(' term ')'
//!           | '[' term ']' | '[' ']'
//! domain  ::= '(' lowerIdent+ ':' grades? term ')'    -- explicit
//!           | '{' lowerIdent+ ':' grades? term '}'    -- implicit, erased by default
//! pattern ::= lowerIdent | '[' lowerIdent ']'
//!           | '(' lowerIdent+ (':' grades? term)? ')'
//! grades  ::= '[' term ']' | '(' term ',' term ')'
//! ```
//!
//! The graded syntax is Granule's.  `a [r]` is the graded modality, written
//! postfix; `[e]` promotes; `[x]` as a pattern unboxes; `a []` abbreviates
//! `a [0..∞]`.  Granule has no dependent functions (its `forall` binds type
//! variables only), so binders follow the dependently typed family instead.
//! An implicit argument is a binder in braces, as in Agda: `{a : Type} -> t`.
//! The grades of a binder are Gerty's: `(x : (s, r) a) -> b` uses `x` `s`
//! times and mentions it `r` times in `b`, and `(x : [s] a) -> b` gives just
//! the first.  A braced binder is erased (`s` is `0`) unless it says otherwise.
//!
//! There is no separate grammar for grades or for types: both are terms.  So
//! `a [r]` is parsed as the application of `a` to the bracketed `r`, and it is
//! the type checker that reads it as a modality once it sees `a : Type`.
//!
//! Binders (lambda parameters, Pi domains, definitions) are lower-case,
//! Haskell-style; an upper-case name is only a reference (`atom`).
//!
//! The parser runs over the *post-layout* token stream (see
//! `idyllic_layout`); the virtual braces and semicolons it inserted are what
//! delimit declarations and `let` blocks here.

use idyllic_ast::*;
use idyllic_diagnostics::Diagnostic;
use idyllic_span::{Span, Spanned};
use idyllic_lexer::token::Token;
use chumsky::input::ValueInput;
use chumsky::prelude::*;
use num_bigint::BigInt;

type Extra<'t> = extra::Err<Rich<'t, Token, SimpleSpan>>;

/// chumsky's span as ours.
fn span_of(s: SimpleSpan) -> Span {
    Span::new(s.start, s.end)
}

enum Item {
    Data(LData),
    Decl(LDecl),
    Expr(LTerm),
}

/// Parse a program from a post-layout token stream.  `src_len` positions the
/// end-of-input span for errors that run off the end.
pub fn parse(src_len: usize, tokens: &[Spanned<Token>]) -> Result<Program, Vec<Diagnostic>> {
    // Post-layout, bare newlines carry no meaning (the significant ones have
    // become virtual `;`/`{`/`}`), but a few can survive; drop them.
    let toks: Vec<(Token, SimpleSpan)> = tokens
        .iter()
        .filter(|t| t.node != Token::Newline)
        .map(|t| (t.node.clone(), SimpleSpan::from(t.span.range())))
        .collect();
    let eoi = SimpleSpan::from(src_len..src_len);
    let input = toks.as_slice().map(eoi, |(t, s)| (t, s));
    // The errors borrow from `toks`, so turn them into owned diagnostics (and
    // drop the parser's temporaries) before `toks` goes out of scope.
    let items: Result<Vec<Item>, Vec<Diagnostic>> =
        top().parse(input).into_result().map_err(|errs| {
            errs.into_iter()
                .map(|e| {
                    Diagnostic::error("parse error").label(span_of(*e.span()), e.to_string())
                })
                .collect()
        });

    let mut program = Program { datas: Vec::new(), decls: Vec::new(), body: None };
    let mut items = items?.into_iter().peekable();
    while let Some(item) = items.next() {
        match item {
            Item::Data(d) => program.datas.push(d),
            Item::Decl(d) => program.decls.push(d),
            Item::Expr(e) if items.peek().is_none() => program.body = Some(e),
            Item::Expr(e) => {
                return Err(vec![Diagnostic::error("parse error")
                    .label(e.span, "an expression can only come last in a program")
                    .help("declarations look like f : a or f x = e")]);
            }
        }
    }
    Ok(program)
}

/// The layout pass always wraps the input in a virtual block, so a program is
/// a virtual-braced list of items.
fn top<'t, I>() -> impl Parser<'t, I, Vec<Item>, Extra<'t>>
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    let term = term();
    let item = choice((data(term.clone()).map(Item::Data), decl(term.clone()).map(Item::Decl), term.map(Item::Expr)));
    // a separator with nothing before it is what a leading comment or blank
    // line leaves behind
    let semi = just(Token::VSemi).or(just(Token::Semi));
    let items = semi
        .clone()
        .repeated()
        .ignore_then(item.separated_by(semi).allow_trailing().at_least(1).collect::<Vec<Item>>());
    items.delimited_by(just(Token::VLBrace), just(Token::VRBrace)).then_ignore(end())
}

/// A constructor's name: upper-case, or an operator that starts with a colon
/// (in a declaration, parenthesised: `(::)`).
fn constructor<'t, I>() -> impl Parser<'t, I, Ident, Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    select! {
        Token::UpperIdent(s) = e => Spanned::new(s, span_of(e.span())),
        Token::ConOpIdent(s) = e => Spanned::new(s, span_of(e.span())),
    }
    .labelled("constructor")
}

/// `data List a = Nil | Cons a (List a)`.  A constructor's fields are atoms,
/// so a compound type is parenthesised, as in Haskell.
fn data<'t, I>(term: impl Parser<'t, I, LTerm, Extra<'t>> + Clone + 't) -> Boxed<'t, 't, I, LData, Extra<'t>>
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    let upper = select! { Token::UpperIdent(s) = e => Spanned::new(s, span_of(e.span())) };
    let param = choice((
        lower().map(|x| (x, None)),
        lower()
            .then_ignore(just(Token::Colon))
            .then(term.clone())
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(|(x, kind)| (x, Some(kind))),
    ));
    // a field is one atom of the term grammar; reuse it by asking for a term
    // and insisting it was written as one
    let field = choice((
        select! {
            Token::UpperIdent(s) = e => Spanned::new(Term::Var(Spanned::new(s, span_of(e.span()))), span_of(e.span())),
            Token::LowerIdent(s) = e => Spanned::new(Term::Var(Spanned::new(s, span_of(e.span()))), span_of(e.span())),
        },
        just(Token::Type).map_with(|_, e| Spanned::new(Term::Type, span_of(e.span()))),
        term.delimited_by(just(Token::LParen), just(Token::RParen)),
    ));
    let name = choice((upper, constructor().delimited_by(just(Token::LParen), just(Token::RParen))));
    let ctor = name.then(field.repeated().collect::<Vec<LTerm>>());
    just(Token::Data)
        .ignore_then(upper)
        .then(param.repeated().collect::<Vec<_>>())
        .then_ignore(just(Token::Eq))
        .then(ctor.separated_by(just(Token::Bar)).at_least(1).collect::<Vec<_>>())
        .map_with(|((name, params), ctors), e| Spanned::new(Data { name, params, ctors }, span_of(e.span())))
        .labelled("data declaration")
        .boxed()
}

fn lower<'t, I>() -> impl Parser<'t, I, Ident, Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    select! { Token::LowerIdent(s) = e => Spanned::new(s, span_of(e.span())) }.labelled("identifier")
}

/// Several names sharing one annotation, `(x y : [2] a)`: each becomes its
/// own parameter, all carrying the span of the whole group so a Pi built
/// from one reads as `(x : a) -> b` rather than starting mid-token.
fn params(names: Vec<Ident>, grade: Option<GradeAnn>, ty: Option<LTerm>, span: Span) -> Vec<LParam> {
    names
        .into_iter()
        .map(|name| Spanned::new(Param { name, unbox: false, grade: grade.clone(), ty: ty.clone() }, span))
        .collect()
}

/// The inside of a parenthesised binder after its names: `: grades? type`.
fn annotation<'t, I>(
    term: impl Parser<'t, I, LTerm, Extra<'t>> + Clone + 't,
) -> impl Parser<'t, I, (Option<GradeAnn>, LTerm), Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    let grades = choice((
        // `[]`, any number of uses; as an atom it already means `[0..∞]`
        just(Token::LBracket).then(just(Token::RBracket)).map_with(|_, e| {
            let span = span_of(e.span());
            let zero = Spanned::new(Term::Lit(Lit::Int(BigInt::from(0))), span);
            let any = Term::Range(Box::new(zero), Box::new(Spanned::new(Term::Inf, span)));
            GradeAnn { subject: Box::new(Spanned::new(any, span)), in_type: None }
        }),
        term.clone()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map(|subject| GradeAnn { subject: Box::new(subject), in_type: None }),
        term.clone()
            .then_ignore(just(Token::Comma))
            .then(term.clone())
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(|(subject, in_type)| GradeAnn { subject: Box::new(subject), in_type: Some(Box::new(in_type)) }),
    ));
    // chumsky backtracks out of `or_not`, so `(x : (f y) a)` — a type that
    // merely starts with a parenthesis — still parses.
    just(Token::Colon).ignore_then(grades.or_not()).then(term)
}

/// A lambda / equation pattern; a parenthesised group yields one parameter
/// per name.
fn pattern<'t, I>(
    term: impl Parser<'t, I, LTerm, Extra<'t>> + Clone + 't,
) -> Boxed<'t, 't, I, Vec<LParam>, Extra<'t>>
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    choice((
        lower().map(|name| {
            let span = name.span;
            vec![Spanned::new(Param { name, unbox: false, grade: None, ty: None }, span)]
        }),
        lower()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map_with(|name, e| {
                vec![Spanned::new(Param { name, unbox: true, grade: None, ty: None }, span_of(e.span()))]
            }),
        lower()
            .repeated()
            .at_least(1)
            .collect::<Vec<Ident>>()
            .then(annotation(term).or_not())
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map_with(|(names, ann), e| {
                let (grade, ty) = ann.map_or((None, None), |(g, ty)| (g, Some(ty)));
                params(names, grade, ty, span_of(e.span()))
            }),
    ))
    .labelled("pattern")
    .boxed()
}

/// A signature or a defining equation.
fn decl<'t, I>(
    term: impl Parser<'t, I, LTerm, Extra<'t>> + Clone + 't,
) -> Boxed<'t, 't, I, LDecl, Extra<'t>>
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    let def = lower()
        .then(pattern(term.clone()).repeated().collect::<Vec<Vec<LParam>>>())
        .then(just(Token::Colon).ignore_then(term.clone()).or_not())
        .then_ignore(just(Token::Eq))
        .then(term.clone())
        .map(|(((f, groups), ty), rhs)| Decl::Def(f, groups.into_iter().flatten().collect(), ty, rhs));
    let sig = lower().then_ignore(just(Token::Colon)).then(term).map(|(f, ty)| Decl::Sig(f, ty));
    // `x : a = e` and `x : a` share a prefix; the equation is tried first and
    // backtracks at the missing `=`
    def.or(sig).map_with(|d, e| Spanned::new(d, span_of(e.span()))).labelled("declaration").boxed()
}

fn term<'t, I>() -> impl Parser<'t, I, LTerm, Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    recursive(|term| {
        let upper = select! { Token::UpperIdent(s) = e => Spanned::new(s, span_of(e.span())) }
            .labelled("constructor");
        let any_ident = lower().or(upper);

        let literal = choice((
            select! {
                Token::Int(n) => Lit::Int(n),
                Token::Str(s) => Lit::Str(s),
                Token::UpperIdent(s) if s == "True" => Lit::Bool(true),
                Token::UpperIdent(s) if s == "False" => Lit::Bool(false),
            },
            just(Token::LParen).then(just(Token::RParen)).to(Lit::Unit),
        ));

        let atom = choice((
            just(Token::Type).map_with(|_, e| Spanned::new(Term::Type, span_of(e.span()))),
            just(Token::Underscore).map_with(|_, e| Spanned::new(Term::Hole, span_of(e.span()))),
            just(Token::Infinity).map_with(|_, e| Spanned::new(Term::Inf, span_of(e.span()))),
            literal.map_with(|l, e| Spanned::new(Term::Lit(l), span_of(e.span()))),
            any_ident.map(|i| {
                let span = i.span;
                Spanned::new(Term::Var(i), span)
            }),
            term.clone().delimited_by(just(Token::LParen), just(Token::RParen)),
            // `[e]`: promotion, or a modality's grade
            term.clone()
                .delimited_by(just(Token::LBracket), just(Token::RBracket))
                .map_with(|t, e| Spanned::new(Term::BoxIntro(Box::new(t)), span_of(e.span()))),
            // `[]` is Granule's shorthand for `[0..∞]`
            just(Token::LBracket).then(just(Token::RBracket)).map_with(|_, e| {
                let span = span_of(e.span());
                let zero = Spanned::new(Term::Lit(Lit::Int(BigInt::from(0))), span);
                let any = Term::Range(Box::new(zero), Box::new(Spanned::new(Term::Inf, span)));
                Spanned::new(Term::BoxIntro(Box::new(Spanned::new(any, span))), span)
            }),
        ))
        .labelled("atom")
        .boxed();

        let app = atom
            .clone()
            .foldl(atom.repeated(), |f: LTerm, a: LTerm| {
                let span = f.span.merge(a.span);
                Spanned::new(Term::App(Box::new(f), Box::new(a)), span)
            })
            .boxed();

        // `a + b` is sugar for applying the primitive named `+` (which the
        // renamer finds in the global scope), so nothing downstream needs to
        // know about infix syntax.  Both operators are left-associative.
        let operator = |name: &'static str| {
            select! { Token::OpIdent(s) = e if s == name => Spanned::new(s, span_of(e.span())) }
                .labelled("operator")
        };
        let binary = |lhs: LTerm, (op, rhs): (Ident, LTerm)| {
            let op_span = op.span;
            let partial_span = lhs.span.merge(op_span);
            let span = lhs.span.merge(rhs.span);
            let op = Spanned::new(Term::Var(op), op_span);
            let partial = Spanned::new(Term::App(Box::new(op), Box::new(lhs)), partial_span);
            Spanned::new(Term::App(Box::new(partial), Box::new(rhs)), span)
        };
        let mul = app.clone().foldl(operator("*").then(app).repeated(), binary).boxed();
        let add = mul.clone().foldl(operator("+").or(operator("-")).then(mul).repeated(), binary).boxed();

        // `x :: xs` applies the constructor named `::`, and associates to the
        // right: `1 :: 2 :: Nil`
        let cons_op = select! { Token::ConOpIdent(s) = e if s == "::" => Spanned::new(s, span_of(e.span())) }
            .labelled("operator");
        let cons = add
            .clone()
            .then(cons_op.then(add).repeated().collect::<Vec<(Ident, LTerm)>>())
            .map(move |(first, rest)| {
                // a :: (b :: (c :: d)): fold from the right
                let mut operands = vec![first];
                let mut ops = Vec::new();
                for (op, e) in rest {
                    ops.push(op);
                    operands.push(e);
                }
                let mut acc = operands.pop().expect("there is a first operand");
                while let (Some(op), Some(lhs)) = (ops.pop(), operands.pop()) {
                    acc = binary(lhs, (op, acc));
                }
                acc
            })
            .boxed();

        // comparisons do not chain
        let compare = cons
            .clone()
            .then(operator("<").or(operator("==")).then(cons).or_not())
            .map(move |(lhs, rest)| match rest {
                None => lhs,
                Some(rest) => binary(lhs, rest),
            })
            .boxed();

        let range = compare.clone().then(operator("..").ignore_then(compare).or_not()).map(|(lo, hi)| match hi {
            None => lo,
            Some(hi) => {
                let span = lo.span.merge(hi.span);
                Spanned::new(Term::Range(Box::new(lo), Box::new(hi)), span)
            }
        });

        let ann = range
            .then(just(Token::Colon).ignore_then(term.clone()).or_not())
            .map(|(e, ty)| match ty {
                None => e,
                Some(ty) => {
                    let span = e.span.merge(ty.span);
                    Spanned::new(Term::Ann(Box::new(e), Box::new(ty)), span)
                }
            })
            .boxed();

        // A dependent-function domain: `(x y : a)` or `(x : [2] a)`; in braces,
        // `{a b : Type}`, the argument is implicit.
        let names_and_type = lower()
            .repeated()
            .at_least(1)
            .collect::<Vec<Ident>>()
            .then(annotation(term.clone()))
            .boxed();
        let domain = choice((
            names_and_type.clone().delimited_by(just(Token::LParen), just(Token::RParen)).map(|d| (d, Icit::Explicit)),
            names_and_type.delimited_by(just(Token::LBrace), just(Token::RBrace)).map(|d| (d, Icit::Implicit)),
        ))
        .map_with(|((names, (g, ty)), icit), e| (params(names, g, Some(ty), span_of(e.span())), icit));

        let pi_telescope = domain
            .repeated()
            .at_least(1)
            .collect::<Vec<(Vec<LParam>, Icit)>>()
            .then_ignore(just(Token::RArrow))
            .then(term.clone())
            .map(|(groups, body)| {
                let binders = groups.into_iter().flat_map(|(ps, icit)| ps.into_iter().map(move |p| (p, icit)));
                binders.rev().fold(body, |acc, (p, icit)| {
                    let span = p.span.merge(acc.span);
                    let Param { name, grade, ty, .. } = p.node;
                    let dom = ty.expect("a domain is always annotated");
                    Spanned::new(Term::Pi(Some(name), icit, grade, Box::new(dom), Box::new(acc)), span)
                })
            });

        let arrow = ann
            .then(just(Token::RArrow).ignore_then(term.clone()).or_not())
            .map(|(lhs, rhs)| match rhs {
                None => lhs,
                Some(rhs) => {
                    let span = lhs.span.merge(rhs.span);
                    Spanned::new(Term::Pi(None, Icit::Explicit, None, Box::new(lhs), Box::new(rhs)), span)
                }
            });

        // chumsky backtracks on `or`, so the telescope form is simply tried
        // first: `(x : a) -> b` is a telescope, `(f x) -> b` falls through.
        let pi_or_arrow = pi_telescope.or(arrow).boxed();

        let lambda = just(Token::BackSlash)
            .map_with(|_, e| span_of(e.span()))
            .then(pattern(term.clone()).repeated().at_least(1).collect::<Vec<Vec<LParam>>>())
            .then_ignore(just(Token::RArrow))
            .then(term.clone())
            .map(|((backslash, groups), body)| {
                let span = backslash.merge(body.span);
                groups.into_iter().flatten().rev().fold(body, |acc, p| {
                    Spanned::new(Term::Lam(Box::new(p), Box::new(acc)), span)
                })
            });

        // Accept either the layout-inserted virtual ';' or an explicit ';',
        // so a one-line `let a = ..; b = .. in ..` and a laid-out block both
        // work.
        let semi = just(Token::VSemi).or(just(Token::Semi));
        let semi_sep = decl(term.clone())
            .separated_by(semi.clone())
            .allow_trailing()
            .at_least(1)
            .collect::<Vec<LDecl>>();
        let let_decls = choice((
            semi_sep.clone().delimited_by(just(Token::LBrace), just(Token::RBrace)),
            semi_sep.clone().delimited_by(just(Token::VLBrace), just(Token::VRBrace)),
            semi_sep,
        ));
        let let_expr = just(Token::Let)
            .map_with(|_, e| span_of(e.span()))
            .then(let_decls)
            .then_ignore(just(Token::In))
            .then(term.clone())
            .map(|((let_span, decls), body)| {
                let span = let_span.merge(body.span);
                Spanned::new(Term::Let(decls, Box::new(body)), span)
            });

        // Unboxing, `let [x] = e in body`.  `let` is a layout herald, so the
        // single binding usually arrives wrapped in a virtual block.
        let unbox_bind = lower()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .then_ignore(just(Token::Eq))
            .then(term.clone())
            .then_ignore(semi.clone().or_not())
            .boxed();
        let let_box = just(Token::Let)
            .map_with(|_, e| span_of(e.span()))
            .then(choice((
                unbox_bind.clone().delimited_by(just(Token::LBrace), just(Token::RBrace)),
                unbox_bind.clone().delimited_by(just(Token::VLBrace), just(Token::VRBrace)),
                unbox_bind,
            )))
            .then_ignore(just(Token::In))
            .then(term.clone())
            .map(|((let_span, (x, boxed)), body)| {
                let span = let_span.merge(body.span);
                Spanned::new(Term::LetBox(x, Box::new(boxed), Box::new(body)), span)
            });

        // `case e of C x y -> a; ..`.  `of` opens a layout block, closed by
        // dedenting — so inside parentheses, or on one line with more after
        // it, write the braces out.
        let alt_pattern = choice((
            // `x :: xs`
            lower().then(cons_op).then(lower()).map(|((x, op), xs)| (op, vec![x, xs])),
            constructor().then(lower().repeated().collect::<Vec<Ident>>()),
        ));
        let alt = alt_pattern
            .then_ignore(just(Token::RArrow))
            .then(term.clone())
            .map(|((ctor, vars), body)| Alt { ctor, vars, body });
        let alts = alt.separated_by(semi.clone()).allow_trailing().at_least(1).collect::<Vec<Alt>>();
        let case = just(Token::Case)
            .map_with(|_, e| span_of(e.span()))
            .then(term.clone())
            .then_ignore(just(Token::Of))
            .then(choice((
                alts.clone().delimited_by(just(Token::LBrace), just(Token::RBrace)),
                alts.delimited_by(just(Token::VLBrace), just(Token::VRBrace)),
            )))
            .map_with(|((case_span, scrutinee), alts), e| {
                let span = case_span.merge(span_of(e.span()));
                Spanned::new(Term::Case(Box::new(scrutinee), alts), span)
            })
            .boxed();

        // `if c then a else b` is `case c of True -> a; False -> b`
        let conditional = just(Token::If)
            .map_with(|_, e| span_of(e.span()))
            .then(term.clone())
            .then(just(Token::Then).map_with(|_, e| span_of(e.span())).then(term.clone()))
            .then(just(Token::Else).map_with(|_, e| span_of(e.span())).then(term.clone()))
            .map(|(((if_span, c), (then_span, a)), (else_span, b))| {
                let span = if_span.merge(b.span);
                let alt = |name: &str, at: Span, body: LTerm| Alt {
                    ctor: Spanned::new(name.to_string(), at),
                    vars: Vec::new(),
                    body,
                };
                let alts = vec![alt("True", then_span, a), alt("False", else_span, b)];
                Spanned::new(Term::Case(Box::new(c), alts), span)
            })
            .boxed();

        choice((lambda, let_box, let_expr, case, conditional, pi_or_arrow))
    })
}
