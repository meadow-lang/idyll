//! Surface parser, via [chumsky].
//!
//! The grammar, loosest to tightest:
//!
//! ```text
//! program ::= item (';' item)*             -- an expression may only come last
//! item    ::= data | effect | decl | term
//! effect  ::= 'effect' upperIdent tparam* 'where' '{' opSig (';' opSig)* '}'
//!           | 'effect' upperIdent tparam* '=' row     -- a name for a row
//! opSig   ::= lowerIdent ':' term
//! data    ::= seqAttr? 'data' upperIdent tparam* (':' term)?
//!             ('=' ctor ('|' ctor)* | 'where' '{' ctorSig (';' ctorSig)* '}')?
//! seqAttr ::= '@' 'seq' '(' 'nil' '=' ctorName ',' 'cons' '=' ctorName ')' ';'?
//! tparam  ::= lowerIdent | '(' lowerIdent ':' term ')'
//! ctor    ::= (upperIdent | '(' conOp ')') field*      -- a field is an atom
//! ctorSig ::= (upperIdent | '(' conOp ')') ':' term
//! decl    ::= name ':' comp                            -- signature
//!           | name param* (':' comp)? rhs              -- equation
//!           | operand op operand rhs                   -- operator equation
//!           | name param* 'impossible'                 -- nothing can match
//! name    ::= lowerIdent | '(' op ')'
//! rhs     ::= '=' term | ('|' term '=' term)+          -- guards
//!
//! term    ::= lambda | letBox | let | case | if | handle | piOrArrow
//! handle  ::= 'handle' term 'with' '{' clause (';' clause)* '}'
//! clause  ::= 'return' binder '->' term | lowerIdent binder binder '->' term
//! lambda  ::= '\' binder+ '->' term
//! letBox  ::= 'let' '[' lowerIdent ']' '=' term 'in' term
//! let     ::= 'let' decl (';' decl)* 'in' term
//! case    ::= 'case' term 'of' alt (';' alt)*
//! alt     ::= pat ('->' term | ('|' term '->' term)+ | 'impossible')
//! if      ::= 'if' term 'then' term 'else' term    -- a case on a Bool
//! piOrArrow
//!         ::= domain+ '->' comp            -- dependent: {a : Type} (x : a) -> c
//!           | annExpr ('->' comp)?         -- non-dependent arrow, right assoc
//! comp    ::= term ('!' row)?              -- a result and its effects
//! row     ::= '{' '}' | '{' term (',' term)* ('|' term)? '}' | appExpr
//! annExpr ::= range (':' term)?
//! range   ::= cmpExpr ('..' cmpExpr)?      -- a grade interval, 0..1
//! cmpExpr ::= consExpr (('<' | '==') consExpr)?    -- does not chain
//! consExpr ::= addExpr ('::' consExpr)?    -- right assoc
//! addExpr ::= mulExpr (('+' | '-') mulExpr)*       -- left assoc
//! mulExpr ::= appExpr (('*' | '/' | '%') appExpr)*   -- left assoc
//! appExpr ::= atom atom*                   -- application, left assoc
//! atom    ::= 'Type' | '_' | '∞' | literal | ident | '(' op ')' | '(' term ')'
//!           | '[' term ']' | '[' ']' | '#[' (term (',' term)*)? ']'
//! domain  ::= '(' lowerIdent+ ':' grades? term ')'    -- explicit
//!           | '{' lowerIdent+ ':' grades? term '}'    -- implicit, erased by default
//! binder  ::= lowerIdent | '_' | '[' lowerIdent ']'
//!           | '(' lowerIdent+ (':' grades? term)? ')'
//! param   ::= '(' lowerIdent+ ':' grades? term ')' | apat
//! pat     ::= operand ('::' pat)?                    -- `::` to the right
//! operand ::= upperIdent apat+ | apat
//! apat    ::= lowerIdent | lowerIdent '@' apat | '_' | literal | upperIdent
//!           | '.' (ident | '(' term ')') | '[' lowerIdent ']' | '(' pat ')'
//!           | '#[' (pat (',' pat)*)? ']'
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
    Effect(LEffect),
    /// `effect App = {Log, Ask}`: a signature and a definition,
    /// `App : Effect; App = {Log, Ask}`
    Alias(Box<(LDecl, LDecl)>),
    Decl(LDecl),
    Use(Spanned<Use>),
    Expr(LTerm),
}

/// What can be written in front of a top-level item, each on a line of its
/// own or before the item on its line.
#[derive(Clone)]
enum Attribute {
    /// `@seq(nil = .., cons = ..)`, on a data type
    Seq(SeqCtors),
    /// `@pub`, `@pub(pkg)`, `@pub(super)`
    Vis(Visibility),
}

/// The item with its attributes, or what is wrong with them.
fn attributed(attrs: Vec<(Attribute, Span)>, item: Item) -> Result<(Item, Option<Visibility>), (Span, String)> {
    let mut seq = None;
    let mut vis = None;
    for (attr, at) in attrs {
        match attr {
            Attribute::Seq(s) if seq.is_none() => seq = Some(s),
            Attribute::Vis(v) if vis.is_none() => vis = Some(v),
            _ => return Err((at, "this attribute is given twice".into())),
        }
    }
    let item = match item {
        Item::Data(mut d) => {
            d.node.seq = seq.take();
            d.node.visibility = vis.unwrap_or_default();
            Item::Data(d)
        }
        Item::Effect(mut e) => {
            e.node.visibility = vis.unwrap_or_default();
            Item::Effect(e)
        }
        Item::Use(mut u) => {
            u.node.public = vis.is_some();
            if vis.is_some_and(|v| v != Visibility::Public) {
                return Err((u.span, "a use is given on with @pub, or not at all".into()));
            }
            Item::Use(u)
        }
        Item::Expr(e) if vis.is_some() || seq.is_some() => {
            return Err((e.span, "an expression has no attributes; only a declaration can".into()));
        }
        other => other,
    };
    if seq.is_some() {
        return Err((Span::at(0), "@seq goes on a data type".into()));
    }
    Ok((item, vis))
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
    let items: Result<Vec<(Item, Option<Visibility>)>, Vec<Diagnostic>> =
        top().parse(input).into_result().map_err(|errs| {
            errs.into_iter()
                .map(|e| {
                    Diagnostic::error("parse error").label(span_of(*e.span()), e.to_string())
                })
                .collect()
        });

    let mut program = Program {
        datas: Vec::new(),
        effects: Vec::new(),
        decls: Vec::new(),
        uses: Vec::new(),
        visibility: Vec::new(),
        body: None,
    };
    let mut items = items?.into_iter().peekable();
    while let Some((item, vis)) = items.next() {
        // the name a definition's visibility is for
        let defined = |d: &LDecl| match &d.node {
            Decl::Sig(x, _) | Decl::Def(x, ..) => x.clone(),
        };
        match item {
            Item::Data(d) => program.datas.push(d),
            Item::Effect(e) => program.effects.push(e),
            Item::Alias(decls) => {
                program.visibility.extend(vis.map(|v| (defined(&decls.0), v)));
                program.decls.extend(<[LDecl; 2]>::from(*decls));
            }
            Item::Decl(d) => {
                program.visibility.extend(vis.map(|v| (defined(&d), v)));
                program.decls.push(d);
            }
            Item::Use(u) => program.uses.push(u),
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
fn top<'t, I>() -> impl Parser<'t, I, Vec<(Item, Option<Visibility>)>, Extra<'t>>
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    let term = term();
    let semi = just(Token::VSemi).or(just(Token::Semi));
    let attribute = choice((seq_attribute().map(Attribute::Seq), visibility().map(Attribute::Vis)))
        .map_with(|a, e| (a, span_of(e.span())))
        .then_ignore(semi.clone().or_not());
    let item = attribute
        .repeated()
        .collect::<Vec<_>>()
        .then(choice((
            data(term.clone()).map(Item::Data),
            effect(term.clone()),
            use_item().map(Item::Use),
            decl(term.clone()).map(Item::Decl),
            term.map(Item::Expr),
        )))
        .validate(|(attrs, item), e, emitter| match attributed(attrs, item) {
            Ok(done) => done,
            Err((at, why)) => {
                let at = if at == Span::at(0) { span_of(e.span()) } else { at };
                emitter.emit(Rich::custom(SimpleSpan::from(at.range()), why));
                // (reported; what is left is nothing)
                (Item::Decl(Spanned::new(Decl::Sig(Spanned::new("_".into(), at), Spanned::new(Term::Hole, at)), at)), None)
            }
        });
    // a separator with nothing before it is what a leading comment or blank
    // line leaves behind; a file with nothing in it is an empty module
    let items = semi
        .clone()
        .repeated()
        .ignore_then(item.separated_by(semi).allow_trailing().collect::<Vec<_>>());
    items.delimited_by(just(Token::VLBrace), just(Token::VRBrace)).then_ignore(end())
}

/// A literal: a number, a string, `True`, `False` or `()`.
fn literal<'t, I>() -> impl Parser<'t, I, Lit, Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    choice((
        select! {
            Token::Int(n) => Lit::Int(n),
            Token::Str(s) => Lit::Str(s),
            Token::UpperIdent(s) if s == "True" => Lit::Bool(true),
            Token::UpperIdent(s) if s == "False" => Lit::Bool(false),
        },
        just(Token::LParen).then(just(Token::RParen)).to(Lit::Unit),
    ))
    .labelled("literal")
}

/// An operator that can be defined: `+`, `==`, `<>` (not one starting with
/// a colon: those are constructors).
fn operator_name<'t, I>() -> impl Parser<'t, I, Ident, Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    select! { Token::OpIdent(s) = e => Spanned::new(s, span_of(e.span())) }.labelled("operator")
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
        term.clone().delimited_by(just(Token::LParen), just(Token::RParen)),
    ));
    let name = choice((upper, constructor().delimited_by(just(Token::LParen), just(Token::RParen))));
    let ctor = name.clone().then(field.repeated().collect::<Vec<LTerm>>().map(Ctor::Fields));
    let ctors = just(Token::Eq).ignore_then(ctor.separated_by(just(Token::Bar)).at_least(1).collect::<Vec<_>>());
    // `where` opens a layout block of constructor signatures
    let semi = just(Token::VSemi).or(just(Token::Semi));
    let sig = name.then_ignore(just(Token::Colon)).then(term.clone().map(Ctor::Sig));
    let sigs = semi.clone().repeated().ignore_then(sig.separated_by(semi).allow_trailing().collect::<Vec<_>>());
    let block = choice((
        sigs.clone().delimited_by(just(Token::LBrace), just(Token::RBrace)),
        sigs.delimited_by(just(Token::VLBrace), just(Token::VRBrace)),
    ));
    let body = choice((ctors, just(Token::Where).ignore_then(block))).or_not().map(Option::unwrap_or_default);
    just(Token::Data)
        .ignore_then(upper)
        .then(param.repeated().collect::<Vec<_>>())
        .then(just(Token::Colon).ignore_then(term).or_not())
        .then(body)
        .map_with(|(((name, params), kind), ctors), e| {
            let visibility = Visibility::Private;
            Spanned::new(Data { name, params, kind, ctors, seq: None, visibility }, span_of(e.span()))
        })
        .labelled("data declaration")
        .boxed()
}

/// `@seq(nil = Nil, cons = (::))`, in front of a data type: what a sequence
/// literal of the type is built with.
fn seq_attribute<'t, I>() -> impl Parser<'t, I, SeqCtors, Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    let word = |w: &'static str| select! { Token::LowerIdent(s) if s == w => () };
    let name = choice((
        select! { Token::UpperIdent(s) = e => Spanned::new(s, span_of(e.span())) },
        constructor().delimited_by(just(Token::LParen), just(Token::RParen)),
    ));
    let nil = word("nil").ignore_then(just(Token::Eq)).ignore_then(name.clone());
    let cons = word("cons").ignore_then(just(Token::Eq)).ignore_then(name);
    just(Token::At)
        .ignore_then(word("seq"))
        .ignore_then(
            nil.then_ignore(just(Token::Comma))
                .then(cons)
                .delimited_by(just(Token::LParen), just(Token::RParen)),
        )
        .map(|(nil, cons)| SeqCtors { nil, cons })
        .labelled("@seq attribute")
}

/// `@pub`, `@pub(pkg)`, `@pub(super)`.
fn visibility<'t, I>() -> impl Parser<'t, I, Visibility, Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    let word = |w: &'static str| select! { Token::LowerIdent(s) if s == w => () };
    let narrowed = choice((word("pkg").to(Visibility::Package), word("super").to(Visibility::Super)))
        .delimited_by(just(Token::LParen), just(Token::RParen));
    just(Token::At)
        .ignore_then(word("pub"))
        .ignore_then(narrowed.or_not())
        .map(|v| v.unwrap_or(Visibility::Public))
        .labelled("visibility")
}

/// A dotted path, `App.Syntax` or `Nat`, a segment each.
fn path<'t, I>() -> impl Parser<'t, I, Vec<Ident>, Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    select! {
        Token::UpperIdent(s) = e => vec![Spanned::new(s, span_of(e.span()))],
        Token::QualUpper(s) = e => segments(&s, span_of(e.span())),
    }
    .labelled("module or type")
}

/// The segments of a qualified name, each with its own span.
fn segments(dotted: &str, span: Span) -> Vec<Ident> {
    let mut at = span.start;
    dotted
        .split('.')
        .map(|seg| {
            let ident = Spanned::new(seg.to_string(), Span::new(at, at + seg.len()));
            at += seg.len() + 1;
            ident
        })
        .collect()
}

/// `use App.Math`, `use App.Math as M`, `use App.Math (double, Expr)`,
/// `use Nat.*`.
fn use_item<'t, I>() -> impl Parser<'t, I, Spanned<Use>, Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    let glob = select! { Token::OpIdent(s) if s == ".*" => () };
    let upper = select! { Token::UpperIdent(s) = e => Spanned::new(s, span_of(e.span())) };
    let alias = select! { Token::LowerIdent(s) if s == "as" => () }.ignore_then(upper);
    let name = choice((
        lower(),
        upper,
        choice((operator_name(), constructor())).delimited_by(just(Token::LParen), just(Token::RParen)),
    ));
    let names = name
        .separated_by(just(Token::Comma))
        .at_least(1)
        .collect::<Vec<Ident>>()
        .delimited_by(just(Token::LParen), just(Token::RParen));
    just(Token::Use)
        .ignore_then(path())
        .then(glob.or_not().map(|g| g.is_some()))
        .then(alias.or_not())
        .then(names.or_not())
        .validate(|(((path, glob), alias), names), e, emitter| {
            if glob && (alias.is_some() || names.is_some()) {
                emitter.emit(Rich::custom(e.span(), "use M.* brings everything in M; it takes no names and no alias"));
            }
            Spanned::new(Use { public: false, path, glob, alias, names }, span_of(e.span()))
        })
        .labelled("use")
}

/// A sequence literal, `#[a, b]`, of terms or of patterns: its elements.
/// `#[a; b]` would be an array, and there are none of those yet.
fn seq_items<'t, I, T>(
    item: impl Parser<'t, I, T, Extra<'t>> + Clone + 't,
) -> impl Parser<'t, I, Vec<T>, Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
    T: 't,
{
    let sep = just(Token::Comma).to(false).or(just(Token::Semi).to(true));
    just(Token::Hash)
        .ignore_then(
            item.clone()
                .or_not()
                .then(sep.then(item.or_not()).repeated().collect::<Vec<_>>())
                .delimited_by(just(Token::LBracket), just(Token::RBracket)),
        )
        // (reported, but parsed all the same: backtracking out of the literal
        // would lose the message)
        .validate(|(first, rest), e, emitter| {
            if rest.iter().any(|(semi, _)| *semi) {
                emitter.emit(Rich::custom(e.span(), "arrays, #[a; b], are not supported yet; a sequence is #[a, b]"));
            } else if first.is_none() && !rest.is_empty() || rest.iter().any(|(_, item)| item.is_none()) {
                emitter.emit(Rich::custom(e.span(), "a sequence's elements are separated by single commas"));
            }
            first.into_iter().chain(rest.into_iter().filter_map(|(_, item)| item)).collect()
        })
}

/// A name a binder can have: a lower-case one, or `_` for none.
fn binder_name<'t, I>() -> impl Parser<'t, I, Ident, Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    let wild = just(Token::Underscore).map_with(|_, e| Spanned::new("_".to_string(), span_of(e.span())));
    lower().or(wild)
}

/// `effect State (s : Type) where get : () -> s; put : s -> ()`.  The
/// operations come in a layout block, as a `data`'s constructors do.
///
/// Or `effect App = {Log, Ask}`, a name for a row.  That is a definition like
/// any other, of a term of type `Effect`, with parameters taken as an
/// effect's are: `effect St s = {State s, Div}` is
///
/// ```text
/// St : (s : Type) -> Effect
/// St s = {State s, Div}
/// ```
fn effect<'t, I>(term: impl Parser<'t, I, LTerm, Extra<'t>> + Clone + 't) -> Boxed<'t, 't, I, Item, Extra<'t>>
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
    let semi = just(Token::VSemi).or(just(Token::Semi));
    let op = lower().then_ignore(just(Token::Colon)).then(term.clone());
    let ops = semi.clone().repeated().ignore_then(op.separated_by(semi).allow_trailing().collect::<Vec<_>>());
    let block = choice((
        ops.clone().delimited_by(just(Token::LBrace), just(Token::RBrace)),
        ops.delimited_by(just(Token::VLBrace), just(Token::VRBrace)),
    ));
    let declared = just(Token::Where).ignore_then(block).map(Ok);
    let alias = just(Token::Eq).ignore_then(row(term)).map(Err);
    just(Token::Effect)
        .ignore_then(upper)
        .then(param.repeated().collect::<Vec<_>>())
        .then(declared.or(alias))
        .map_with(|((name, params), body), e| {
            let span = span_of(e.span());
            let row = match body {
                Ok(ops) => {
                    let visibility = Visibility::Private;
                    return Item::Effect(Spanned::new(EffectDecl { name, params, ops, visibility }, span));
                }
                Err(row) => row,
            };
            let effect = Spanned::new(Term::Var(Spanned::new("Effect".to_string(), name.span)), name.span);
            // a parameter is used as often as the row mentions it: `[]`
            let any = |at: Span| {
                let zero = Spanned::new(Term::Lit(Lit::Int(BigInt::from(0))), at);
                let any = Term::Range(Box::new(zero), Box::new(Spanned::new(Term::Inf, at)));
                GradeAnn { subject: Box::new(Spanned::new(any, at)), in_type: None }
            };
            let ty = params.iter().rev().fold(effect, |acc, (x, kind)| {
                let kind = kind.clone().unwrap_or_else(|| Spanned::new(Term::Type, x.span));
                let pi = Term::Pi(Some(x.clone()), Icit::Explicit, Some(any(x.span)), Box::new(kind), Box::new(acc));
                Spanned::new(pi, span)
            });
            let pats = params
                .into_iter()
                .map(|(x, _)| {
                    let at = x.span;
                    Spanned::new(Pat::Bind(Param { name: x, unbox: false, grade: None, ty: None }), at)
                })
                .collect();
            let sig = Spanned::new(Decl::Sig(name.clone(), ty), span);
            Item::Alias(Box::new((sig, Spanned::new(Decl::Def(name, pats, None, Rhs::Plain(row)), span))))
        })
        .labelled("effect declaration")
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

/// A lambda's binder; a parenthesised group yields one parameter per name.
/// `_` is a binder nothing can refer to.
fn binder<'t, I>(
    term: impl Parser<'t, I, LTerm, Extra<'t>> + Clone + 't,
) -> Boxed<'t, 't, I, Vec<LParam>, Extra<'t>>
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    let wild = just(Token::Underscore).map_with(|_, e| Spanned::new("_".to_string(), span_of(e.span())));
    choice((
        lower().or(wild).map(|name| {
            let span = name.span;
            vec![Spanned::new(Param { name, unbox: false, grade: None, ty: None }, span)]
        }),
        lower()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map_with(|name, e| {
                vec![Spanned::new(Param { name, unbox: true, grade: None, ty: None }, span_of(e.span()))]
            }),
        binder_name()
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
    .labelled("binder")
    .boxed()
}

/// A pattern that needs no parentheses to be an argument: a variable, `_`,
/// a literal, a constructor on its own, `x@p`, `.e`, `[x]`, or anything in
/// parentheses.
fn apattern<'t, I>(
    term: impl Parser<'t, I, LTerm, Extra<'t>> + Clone + 't,
) -> Boxed<'t, 't, I, LPat, Extra<'t>>
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    recursive(|apat| {
        let upper = select! {
            Token::UpperIdent(s) = e => Spanned::new(s, span_of(e.span())),
            Token::QualUpper(s) = e => Spanned::new(s, span_of(e.span())),
        };
        let var = |name: Ident| Pat::Bind(Param { name, unbox: false, grade: None, ty: None });
        // `.x` or `.(e)`: a term, not a pattern
        let dotted = choice((
            lower().or(upper).map(|x| {
                let span = x.span;
                Spanned::new(Term::Var(x), span)
            }),
            term.delimited_by(just(Token::LParen), just(Token::RParen)),
        ));
        choice((
            lower().then_ignore(just(Token::At)).then(apat.clone()).map(|(x, p)| Pat::As(x, Box::new(p))),
            literal().map(Pat::Lit),
            just(Token::Underscore).to(Pat::Wild),
            lower().map(var),
            upper.map(|c| Pat::Con(c, Vec::new())),
            just(Token::Period).ignore_then(dotted).map(|e| Pat::Dot(Box::new(e))),
            lower()
                .delimited_by(just(Token::LBracket), just(Token::RBracket))
                .map(|name| Pat::Bind(Param { name, unbox: true, grade: None, ty: None })),
            seq_items(pattern(apat.clone())).map(Pat::Seq),
        ))
        .map_with(|p, e| Spanned::new(p, span_of(e.span())))
        .or(pattern(apat).delimited_by(just(Token::LParen), just(Token::RParen)))
    })
    .labelled("pattern")
    .boxed()
}

/// A pattern: a constructor applied to argument patterns, or patterns
/// joined by `::` (to the right, as in terms).
fn pattern<'t, I>(
    apat: impl Parser<'t, I, LPat, Extra<'t>> + Clone + 't,
) -> impl Parser<'t, I, LPat, Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    let operand = operand(apat);
    let cons_op = select! { Token::ConOpIdent(s) = e if s == "::" => Spanned::new(s, span_of(e.span())) };
    operand.clone().then(cons_op.then(operand).repeated().collect::<Vec<(Ident, LPat)>>()).map(|(first, rest)| {
        let mut operands = vec![first];
        let mut ops = Vec::new();
        for (op, p) in rest {
            ops.push(op);
            operands.push(p);
        }
        let mut acc = operands.pop().expect("there is a first operand");
        while let (Some(op), Some(lhs)) = (ops.pop(), operands.pop()) {
            let span = lhs.span.merge(acc.span);
            acc = Spanned::new(Pat::Con(op, vec![lhs, acc]), span);
        }
        acc
    })
}

/// A constructor applied to argument patterns, or an argument pattern.
fn operand<'t, I>(
    apat: impl Parser<'t, I, LPat, Extra<'t>> + Clone + 't,
) -> impl Parser<'t, I, LPat, Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    let upper = select! {
        Token::UpperIdent(s) = e => Spanned::new(s, span_of(e.span())),
        Token::QualUpper(s) = e => Spanned::new(s, span_of(e.span())),
    };
    let applied = upper
        .then(apat.clone().repeated().at_least(1).collect::<Vec<LPat>>())
        .map_with(|(c, args), e| Spanned::new(Pat::Con(c, args), span_of(e.span())));
    applied.or(apat)
}

/// `f Nil impossible` parses as an equation with a last pattern named
/// `impossible` and nothing after it; this takes that pattern off again.
fn impossible(mut pats: Vec<LPat>) -> Option<Vec<LPat>> {
    match pats.pop()?.node {
        Pat::Bind(Param { name, unbox: false, grade: None, ty: None }) if name.node == "impossible" => Some(pats),
        _ => None,
    }
}

/// `b` or `b ! e`: a result type, and the effects of computing it.
fn comp<'t, I>(
    term: impl Parser<'t, I, LTerm, Extra<'t>> + Clone + 't,
) -> impl Parser<'t, I, LTerm, Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    term.clone().then(just(Token::Bang).ignore_then(row(term)).or_not()).map(|(t, row)| match row {
        None => t,
        Some(row) => {
            let span = t.span.merge(row.span);
            Spanned::new(Term::Eff(Box::new(t), Box::new(row)), span)
        }
    })
}

/// A row of effects, in braces, `{Log, Div | e}`, or a single effect, `Div`
/// or `State Int`.
fn row<'t, I>(term: impl Parser<'t, I, LTerm, Extra<'t>> + Clone + 't) -> impl Parser<'t, I, LTerm, Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    let ident = select! {
        Token::UpperIdent(s) = e => Spanned::new(s, span_of(e.span())),
        Token::QualUpper(s) = e => Spanned::new(s, span_of(e.span())),
        Token::QualLower(s) = e => Spanned::new(s, span_of(e.span())),
        Token::LowerIdent(s) = e => Spanned::new(s, span_of(e.span())),
    };
    let atom = choice((
        ident.map(|x| {
            let span = x.span;
            Spanned::new(Term::Var(x), span)
        }),
        term.clone().delimited_by(just(Token::LParen), just(Token::RParen)),
    ));
    let single = atom.clone().foldl(atom.repeated(), |f: LTerm, a: LTerm| {
        let span = f.span.merge(a.span);
        Spanned::new(Term::App(Box::new(f), Box::new(a)), span)
    });
    let braced = term
        .clone()
        .separated_by(just(Token::Comma))
        .collect::<Vec<LTerm>>()
        .then(just(Token::Bar).ignore_then(term.clone()).or_not())
        .delimited_by(just(Token::LBrace), just(Token::RBrace))
        .map_with(|(labels, tail), e| Spanned::new(Term::Row(labels, tail.map(Box::new)), span_of(e.span())));
    braced.or(single)
}

/// A signature or a defining equation.
fn decl<'t, I>(
    term: impl Parser<'t, I, LTerm, Extra<'t>> + Clone + 't,
) -> Boxed<'t, 't, I, LDecl, Extra<'t>>
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    let apat = apattern(term.clone());
    // `(x y : [2] a)` gives each name the annotation
    let group = binder_name()
        .repeated()
        .at_least(1)
        .collect::<Vec<Ident>>()
        .then(annotation(term.clone()))
        .delimited_by(just(Token::LParen), just(Token::RParen))
        .map_with(|(names, (grade, ty)), e| {
            params(names, grade, Some(ty), span_of(e.span())).into_iter().map(|p| p.map(Pat::Bind)).collect()
        });
    let params = group.or(apat.clone().map(|p| vec![p])).repeated().collect::<Vec<Vec<LPat>>>();
    let guard = just(Token::Bar).ignore_then(term.clone()).then_ignore(just(Token::Eq)).then(term.clone());
    let rhs = choice((
        just(Token::Eq).ignore_then(term.clone()).map(Rhs::Plain),
        guard.repeated().at_least(1).collect::<Vec<(LTerm, LTerm)>>().map(Rhs::Guarded),
    ));
    let name = lower().or(operator_name().delimited_by(just(Token::LParen), just(Token::RParen)));
    let prefix = name
        .clone()
        .then(params.map(|groups| groups.into_iter().flatten().collect::<Vec<LPat>>()))
        .then(just(Token::Colon).ignore_then(comp(term.clone())).or_not())
        .then(rhs.clone().or_not())
        .try_map(|(((f, pats), ty), rhs), span| match rhs {
            Some(rhs) => Ok(Decl::Def(f, pats, ty, rhs)),
            None => match impossible(pats) {
                Some(pats) if !pats.is_empty() && ty.is_none() => Ok(Decl::Def(f, pats, None, Rhs::Impossible)),
                _ => Err(Rich::custom(span, "expected '=' or a guard")),
            },
        });
    // `Z + m = m`, `S n + m = S (n + m)`
    let infix = operand(apat.clone())
        .then(operator_name())
        .then(operand(apat))
        .then(rhs)
        .map(|(((lhs, op), rhs_pat), rhs)| Decl::Def(op, vec![lhs, rhs_pat], None, rhs));
    let sig = name.then_ignore(just(Token::Colon)).then(comp(term)).map(|(f, ty)| Decl::Sig(f, ty));
    // `x : a = e` and `x : a` share a prefix; the equation is tried first and
    // backtracks at the missing `=`
    choice((prefix, infix, sig)).map_with(|d, e| Spanned::new(d, span_of(e.span()))).labelled("declaration").boxed()
}

fn term<'t, I>() -> impl Parser<'t, I, LTerm, Extra<'t>> + Clone
where
    I: ValueInput<'t, Token = Token, Span = SimpleSpan>,
{
    recursive(|term| {
        let upper = select! { Token::UpperIdent(s) = e => Spanned::new(s, span_of(e.span())) }
            .labelled("constructor");
        // a qualified name, `Nat.Z` or `M.double`, is a name like any other
        let qualified = select! {
            Token::QualUpper(s) = e => Spanned::new(s, span_of(e.span())),
            Token::QualLower(s) = e => Spanned::new(s, span_of(e.span())),
        };
        let any_ident = lower().or(upper).or(qualified);

        let atom = choice((
            just(Token::Type).map_with(|_, e| Spanned::new(Term::Type, span_of(e.span()))),
            just(Token::Underscore).map_with(|_, e| Spanned::new(Term::Hole, span_of(e.span()))),
            just(Token::Infinity).map_with(|_, e| Spanned::new(Term::Inf, span_of(e.span()))),
            literal().map_with(|l, e| Spanned::new(Term::Lit(l), span_of(e.span()))),
            any_ident.map(|i| {
                let span = i.span;
                Spanned::new(Term::Var(i), span)
            }),
            // an operator on its own, `(+)`
            operator_name()
                .delimited_by(just(Token::LParen), just(Token::RParen))
                .map_with(|op, e| Spanned::new(Term::Var(op), span_of(e.span()))),
            term.clone().delimited_by(just(Token::LParen), just(Token::RParen)),
            // `[e]`: promotion, or a modality's grade
            term.clone()
                .delimited_by(just(Token::LBracket), just(Token::RBracket))
                .map_with(|t, e| Spanned::new(Term::BoxIntro(Box::new(t)), span_of(e.span()))),
            // `#[a, b]`, a sequence
            seq_items(term.clone()).map_with(|items, e| Spanned::new(Term::Seq(items), span_of(e.span()))),
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
        let mul_op = operator("*").or(operator("/")).or(operator("%"));
        let mul = app.clone().foldl(mul_op.then(app).repeated(), binary).boxed();
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

        // what a guard in a `case` can be: anything short of `->`, which
        // ends it
        let guard_expr = range.clone().boxed();

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
            .then(comp(term.clone()))
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
            .then(just(Token::RArrow).ignore_then(comp(term.clone())).or_not())
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
            .then(binder(term.clone()).repeated().at_least(1).collect::<Vec<Vec<LParam>>>())
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
        let guard = just(Token::Bar).ignore_then(guard_expr).then_ignore(just(Token::RArrow)).then(term.clone());
        let alt_rhs = choice((
            just(Token::RArrow).ignore_then(term.clone()).map(Rhs::Plain),
            guard.repeated().at_least(1).collect::<Vec<(LTerm, LTerm)>>().map(Rhs::Guarded),
        ));
        let alt = pattern(apattern(term.clone())).then(alt_rhs.or_not()).try_map(|(pat, rhs), span| match rhs {
            Some(rhs) => Ok(Alt { pat, rhs }),
            // `Nil impossible`
            None => match pat.node {
                Pat::Con(c, args) => match impossible(args) {
                    Some(args) => Ok(Alt { pat: Spanned::new(Pat::Con(c, args), pat.span), rhs: Rhs::Impossible }),
                    None => Err(Rich::custom(span, "expected '->' or a guard")),
                },
                _ => Err(Rich::custom(span, "expected '->' or a guard")),
            },
        });
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
                let alt = |b: bool, at: Span, body: LTerm| Alt {
                    pat: Spanned::new(Pat::Lit(Lit::Bool(b)), at),
                    rhs: Rhs::Plain(body),
                };
                let alts = vec![alt(true, then_span, a), alt(false, else_span, b)];
                Spanned::new(Term::Case(Box::new(c), alts), span)
            })
            .boxed();

        // `handle e with return x -> a; op x k -> b`: a layout block of
        // clauses, as `case` has
        let clause = lower()
            .then(binder_name().repeated().collect::<Vec<Ident>>())
            .then_ignore(just(Token::RArrow))
            .then(term.clone())
            .map(|((op, params), body)| HandleClause { op, params, body });
        let clauses = clause.separated_by(semi.clone()).allow_trailing().at_least(1).collect::<Vec<HandleClause>>();
        let handle = just(Token::Handle)
            .map_with(|_, e| span_of(e.span()))
            .then(term.clone())
            .then_ignore(just(Token::With))
            .then(choice((
                clauses.clone().delimited_by(just(Token::LBrace), just(Token::RBrace)),
                clauses.delimited_by(just(Token::VLBrace), just(Token::VRBrace)),
            )))
            .map_with(|((at, body), clauses), e| {
                let span = at.merge(span_of(e.span()));
                Spanned::new(Term::Handle(Box::new(body), clauses), span)
            })
            .boxed();

        choice((lambda, let_box, let_expr, case, conditional, handle, pi_or_arrow))
    })
}
