//! The layout (offside) pass: turn significant indentation into explicit
//! block structure, so the parser can ignore whitespace entirely.
//!
//! It runs in two stages.  [`mark`] annotates the token stream with the two
//! facts layout cares about — where a block *opens* ([`Marked::OpenBlock`],
//! carrying the column its items will line up at) and where a line *begins*
//! ([`Marked::Indent`], carrying that line's column).  [`layout`] then walks
//! those markers with a stack of open blocks, emitting the virtual
//! [`Token::VLBrace`] / [`Token::VRBrace`] / [`Token::VSemi`] the parser
//! consumes.
//!
//! **How this differs from Haskell's rule.**  Haskell closes a layout block
//! purely by dedentation, plus the celebrated parse-error side condition.
//! Here, `let` and `do` instead record the keyword that closes them — `in` and
//! `where` respectively — and that keyword closes the block wherever it turns
//! up (see [`closes`]).  It is a much simpler rule to implement, but it is a
//! genuine restriction: because `in` closes the *innermost* block that is
//! waiting for one, a `let` cannot appear inside a `case` alternative without
//! explicit braces.  The `in` would be matched against the wrong block.
//!
//! Layout is suspended inside a sequence literal, `#[ … ]`, and inside
//! brackets and parentheses, as it is inside explicit braces: what is in them
//! may run over several lines, at any indentation, and a `;` there is always
//! the literal's own.  A block opened inside them is laid out as usual, and
//! their `]` or `)` closes it if it is still open, as Haskell's parse-error
//! rule would: `(case m of` with its alternatives on the lines after, then
//! `)`, is one expression.

use idyllic_diagnostics::Diagnostic;
use idyllic_span::{LineIndex, Span, Spanned};
use idyllic_lexer::token::Token;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// A block opened at a column that is not further right than the block
    /// containing it.
    NestedLayoutContext {
        outer: usize,
        inner: usize,
        span: Span,
    },
    UnexpectedClosingBrace(Span),
    NotInLayoutContextAtEndOfInput(Span),
}

impl LayoutError {
    pub fn to_diagnostic(&self) -> Diagnostic {
        match self {
            LayoutError::NestedLayoutContext { outer, inner, span } => {
                let msg = format!(
                    "nested layout context: expected indentation greater than {outer}, but got {inner}"
                );
                Diagnostic::error(msg.clone()).label(*span, msg)
            }
            LayoutError::UnexpectedClosingBrace(span) => {
                Diagnostic::error("unexpected closing brace").label(*span, "unexpected closing brace")
            }
            LayoutError::NotInLayoutContextAtEndOfInput(span) => {
                Diagnostic::error("not inside a layout context at end of input")
                    .label(*span, "not inside a layout context at end of input")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Heralds
// ---------------------------------------------------------------------------

/// What sort of block a keyword opens.
enum Herald {
    /// `let` and `do`: closed by dedenting *or* by an explicit keyword.
    ClosedBy(Token),
    /// `where` and `of`: closed only by dedenting.
    OpenEnded,
}

impl Herald {
    fn closer(&self) -> Option<Token> {
        match self {
            Herald::ClosedBy(t) => Some(t.clone()),
            Herald::OpenEnded => None,
        }
    }
}

/// The keywords that open a layout block.
fn herald_of(t: &Token) -> Option<Herald> {
    match t {
        Token::Let => Some(Herald::ClosedBy(Token::Let)),
        Token::Do => Some(Herald::ClosedBy(Token::Do)),
        Token::Where | Token::Of | Token::With => Some(Herald::OpenEnded),
        _ => None,
    }
}

/// `in` closes a `let`; `where` closes a `do`.  This is the token-driven half
/// of the algorithm described at the top of the module.
fn closes(t: &Token, closer: &Token) -> bool {
    matches!((t, closer), (Token::In, Token::Let) | (Token::Where, Token::Do))
}

// ---------------------------------------------------------------------------
// Marking
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Marked {
    Token(Spanned<Token>),
    OpenBlock {
        column: usize,
        closer: Option<Token>,
        span: Span,
    },
    Indent {
        column: usize,
        span: Span,
    },
}

pub fn mark(li: &LineIndex, tokens: &[Spanned<Token>]) -> Vec<Marked> {
    mark_indents(li, mark_blocks(li, tokens))
}

fn is_newline(t: &Spanned<Token>) -> bool {
    t.node == Token::Newline
}

/// Insert an `OpenBlock` after every layout herald, recording the column of
/// the first token of the block.
fn mark_blocks(li: &LineIndex, toks: &[Spanned<Token>]) -> Vec<Marked> {
    let mut out = Vec::with_capacity(toks.len());
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        out.push(Marked::Token(t.clone()));
        i += 1;
        let Some(herald) = herald_of(&t.node) else { continue };
        // An explicit '{' means the block is written out by hand, so layout
        // keeps out of it.  At end of input only 'let'/'do' open a
        // (necessarily empty) block; a trailing 'where'/'of' opens nothing.
        let opens = match toks.get(i) {
            None => matches!(herald, Herald::ClosedBy(_)),
            Some(n) => n.node != Token::LBrace,
        };
        if !opens {
            continue;
        }
        // the block's column is that of its first content token
        let mut j = i;
        while j < toks.len() && is_newline(&toks[j]) {
            j += 1;
        }
        let closer = herald.closer();
        match toks.get(j) {
            None => {
                out.push(Marked::OpenBlock { column: 0, closer, span: t.span });
                i = j;
            }
            Some(first) => {
                out.push(Marked::OpenBlock {
                    column: li.column(first.span.start),
                    closer,
                    span: t.span,
                });
                // 'let'/'do' re-examine the block's first token, which may
                // itself be a herald ("let x = do .."); 'where'/'of' take it
                // as read.
                match herald {
                    Herald::ClosedBy(_) => i = j,
                    Herald::OpenEnded => {
                        out.push(Marked::Token(first.clone()));
                        i = j + 1;
                    }
                }
            }
        }
    }
    out
}

/// Replace each newline with an `Indent` carrying the column of the token
/// that follows it.
fn mark_indents(li: &LineIndex, ms: Vec<Marked>) -> Vec<Marked> {
    let mut out = Vec::with_capacity(ms.len());
    let mut i = 0;
    while i < ms.len() {
        match &ms[i] {
            // an OpenBlock already knows its column; nothing to do
            m @ Marked::OpenBlock { .. } => {
                out.push(m.clone());
                i += 1;
            }
            Marked::Token(n) if is_newline(n) => match ms.get(i + 1) {
                // a trailing newline indents nothing
                None => i += 1,
                // Nor does one followed by another: the line between them is
                // blank, or only a comment.  (Measuring it anyway would take
                // the column of the *next newline* for an indentation — the
                // far end of a comment.)
                Some(Marked::Token(t)) if is_newline(t) => i += 1,
                Some(Marked::Token(t)) => {
                    out.push(Marked::Indent { column: li.column(t.span.start), span: n.span });
                    out.push(Marked::Token(t.clone()));
                    i += 2;
                }
                Some(_) => {
                    out.push(ms[i].clone());
                    i += 1;
                }
            },
            m => {
                out.push(m.clone());
                i += 1;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// What opened a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// a herald (`let`, `where`, `of`, ..), or the file itself: its items
    /// line up, and dedenting closes it
    Layout,
    /// `{`, a block written out by hand, closed by its `}`
    Brace,
    /// `#[`, a sequence literal, closed by its `]`
    Seq,
    /// `[`, closed by its `]`
    Bracket,
    /// `(`, closed by its `)`
    Paren,
}

/// An open block.  `column` is the column its items line up at, or 0 for one
/// that dedenting does not close: braces, brackets and parentheses, inside
/// which layout is suspended (which `is_explicit` is asking about), though a
/// block opened inside them is laid out as any is.
#[derive(Clone, Debug)]
struct LayoutCtx {
    column: usize,
    closer: Option<Token>,
    kind: Kind,
}

impl LayoutCtx {
    fn is_explicit(&self) -> bool {
        self.kind != Kind::Layout
    }
}

pub struct LayoutOutput {
    pub tokens: Vec<Spanned<Token>>,
    pub errors: Vec<LayoutError>,
}

struct Layouter {
    /// open blocks, innermost last
    stack: Vec<LayoutCtx>,
    out: Vec<Spanned<Token>>,
    errors: Vec<LayoutError>,
    /// Everything still open at end of input is closed at the last token.
    end_span: Span,
}

pub fn layout(li: &LineIndex, input: &[Spanned<Token>]) -> LayoutOutput {
    let markers = mark(li, input);
    let mut st = Layouter {
        stack: vec![LayoutCtx { column: 1, closer: None, kind: Kind::Layout }],
        out: vec![Spanned::new(Token::VLBrace, Span::at(0))],
        errors: Vec::new(),
        end_span: input.last().map_or(Span::at(0), |t| t.span),
    };
    st.run(&markers);
    LayoutOutput { tokens: st.out, errors: st.errors }
}

impl Layouter {
    fn emit(&mut self, t: Token, span: Span) {
        self.out.push(Spanned::new(t, span));
    }

    fn push(&mut self, column: usize, closer: Option<Token>) {
        self.stack.push(LayoutCtx { column, closer, kind: Kind::Layout });
    }

    /// A block that dedenting does not close: `{`, `#[`, `[` or `(`.
    fn push_group(&mut self, kind: Kind, closer: Option<Token>) {
        self.stack.push(LayoutCtx { column: 0, closer, kind });
    }

    fn top(&self) -> Option<LayoutCtx> {
        self.stack.last().cloned()
    }

    /// Each handler consumes the marker at `i` (and sometimes the one after)
    /// and returns the index to continue from.
    fn run(&mut self, ms: &[Marked]) {
        let mut i = 0;
        while i < ms.len() {
            i = match &ms[i] {
                Marked::Indent { column, span } => self.indent(*column, *span, ms, i),
                Marked::OpenBlock { column, closer, span } => {
                    self.open(*column, closer.clone(), *span, ms, i)
                }
                Marked::Token(t) => self.token(t, ms, i),
            };
        }
        self.close_all();
    }

    /// An `Indent` is where a line starts.  Against the innermost block's
    /// column it means: equal, a new item; further right, a continuation
    /// line; further left, the block has ended.
    fn indent(&mut self, col: usize, sp: Span, ms: &[Marked], i: usize) -> usize {
        loop {
            let Some(ctx) = self.top() else { return i + 1 };
            if col == ctx.column {
                self.emit(Token::VSemi, sp);
                return i + 1;
            }
            if col > ctx.column {
                return i + 1;
            }
            // Dedenting closes the innermost block.  If the token right after
            // it is the keyword that also closes that block, it belongs just
            // after the virtual '}' and is consumed here.
            self.emit(Token::VRBrace, sp);
            self.stack.pop();
            if let (Some(closer), Some(Marked::Token(t))) = (&ctx.closer, ms.get(i + 1)) {
                if closes(&t.node, closer) {
                    self.emit(t.node.clone(), t.span);
                    return i + 2;
                }
            }
            // one dedent can close several blocks, so re-run this same
            // Indent against whatever is now innermost
        }
    }

    /// A block may only open further right than the block containing it.
    fn open(&mut self, col: usize, closer: Option<Token>, sp: Span, ms: &[Marked], i: usize) -> usize {
        match self.top() {
            Some(ctx) if col > ctx.column => {
                self.emit(Token::VLBrace, sp);
                self.push(col, closer);
                i + 1
            }
            // Nothing further right: the block is empty, as in Haskell, and
            // what follows is a new line of the enclosing one (`data Void
            // where` with the next declaration under it).  Except for a
            // `let` or `do`, whose block must hold something.
            Some(_) if closer.is_none() => {
                self.emit(Token::VLBrace, sp);
                self.emit(Token::VRBrace, sp);
                self.indent(col, sp, ms, i)
            }
            Some(ctx) => {
                self.errors.push(LayoutError::NestedLayoutContext {
                    outer: ctx.column,
                    inner: col,
                    span: sp,
                });
                self.stack.pop();
                i + 1
            }
            None if col > 0 => {
                self.emit(Token::VLBrace, sp);
                self.push(col, closer);
                i + 1
            }
            // a herald with nothing after it: an empty block, '{}' at once
            None => {
                self.emit(Token::VLBrace, sp);
                self.emit(Token::VRBrace, sp);
                self.indent(col, sp, ms, i)
            }
        }
    }

    fn token(&mut self, t: &Spanned<Token>, ms: &[Marked], i: usize) -> usize {
        if t.node == Token::RBrace {
            return self.close_explicit(t, ms, i);
        }
        // 'let {' / 'do {': the user opens the block, but we still record
        // which keyword closes it so a later 'in' can be matched up
        if let Some(closer) = herald_of(&t.node).and_then(|h| h.closer()) {
            if let Some(Marked::Token(lb)) = ms.get(i + 1) {
                if lb.node == Token::LBrace {
                    self.emit(t.node.clone(), t.span);
                    self.emit(lb.node.clone(), lb.span);
                    self.push_group(Kind::Brace, Some(closer));
                    return i + 2;
                }
            }
        }
        if t.node == Token::LBrace {
            self.emit(t.node.clone(), t.span);
            self.push_group(Kind::Brace, None);
            return i + 1;
        }
        // `#[`: a sequence literal, with layout suspended until its `]`
        if t.node == Token::Hash {
            if let Some(Marked::Token(lb)) = ms.get(i + 1) {
                if lb.node == Token::LBracket {
                    self.emit(t.node.clone(), t.span);
                    self.emit(lb.node.clone(), lb.span);
                    self.push_group(Kind::Seq, None);
                    return i + 2;
                }
            }
        }
        // `[` and `(` suspend layout too, until what closes them
        let group = match t.node {
            Token::LBracket => Some(Kind::Bracket),
            Token::LParen => Some(Kind::Paren),
            _ => None,
        };
        if let Some(kind) = group {
            self.emit(t.node.clone(), t.span);
            self.push_group(kind, None);
            return i + 1;
        }
        let closing: &[Kind] = match t.node {
            Token::RBracket => &[Kind::Seq, Kind::Bracket],
            Token::RParen => &[Kind::Paren],
            _ => &[],
        };
        if !closing.is_empty() {
            self.close_group(closing, t.span);
            self.emit(t.node.clone(), t.span);
            return i + 1;
        }
        // a `,` separates the items of the group it is in, never of a layout
        // block: the blocks opened since the group's item began are over
        if t.node == Token::Comma {
            if let Some(at) = self.stack.iter().rposition(|c| c.is_explicit()) {
                self.close_above(at, t.span);
            }
        }
        // 'in' / 'where' closing the block that is waiting for it
        if let Some(ctx) = self.top() {
            if !ctx.is_explicit() {
                if let Some(closer) = &ctx.closer {
                    if closes(&t.node, closer) {
                        self.emit(Token::VRBrace, t.span);
                        self.emit(t.node.clone(), t.span);
                        self.stack.pop();
                        return i + 1;
                    }
                }
            }
        }
        self.emit(t.node.clone(), t.span);
        i + 1
    }

    /// A `]` or a `)` closes the `#[`, `[` or `(` it matches, and the layout
    /// blocks opened inside it, as Haskell's parse-error rule would: so a
    /// `case` in parentheses may have its alternatives on lines of their
    /// own. (One that matches nothing, or with an explicit `{` in between, is
    /// the parser's to report.)
    fn close_group(&mut self, kinds: &[Kind], sp: Span) {
        let Some(at) = self.stack.iter().rposition(|c| c.is_explicit()) else { return };
        if !kinds.contains(&self.stack[at].kind) {
            return;
        }
        self.close_above(at, sp);
        self.stack.pop();
    }

    /// Close every layout block above the one at `at`.
    fn close_above(&mut self, at: usize, sp: Span) {
        while self.stack.len() > at + 1 {
            self.emit(Token::VRBrace, sp);
            self.stack.pop();
        }
    }

    /// '}' closes an explicit block; if that block was opened by 'let {' its
    /// closing keyword may follow immediately.
    fn close_explicit(&mut self, t: &Spanned<Token>, ms: &[Marked], i: usize) -> usize {
        if let Some(ctx) = self.top() {
            // inside a `#[`, `[` or `(`: the parser reports the `}`
            if ctx.is_explicit() && ctx.kind != Kind::Brace {
                self.emit(t.node.clone(), t.span);
                return i + 1;
            }
            if ctx.is_explicit() {
                match (&ctx.closer, ms.get(i + 1)) {
                    (None, _) => {
                        self.emit(t.node.clone(), t.span);
                        self.stack.pop();
                        return i + 1;
                    }
                    (Some(closer), Some(Marked::Token(kw))) if closes(&kw.node, closer) => {
                        self.emit(t.node.clone(), t.span);
                        self.emit(kw.node.clone(), kw.span);
                        self.stack.pop();
                        return i + 2;
                    }
                    _ => {}
                }
            }
        }
        self.errors.push(LayoutError::UnexpectedClosingBrace(t.span));
        i + 1
    }

    /// At end of input every layout block is closed; an explicit block that
    /// never saw its '}' is an error, and stops the unwinding.
    fn close_all(&mut self) {
        while let Some(ctx) = self.top() {
            // a `[` or `(` never closed is the parser's to report
            if matches!(ctx.kind, Kind::Bracket | Kind::Paren) {
                self.stack.pop();
                continue;
            }
            if ctx.is_explicit() {
                self.errors.push(LayoutError::NotInLayoutContextAtEndOfInput(self.end_span));
                return;
            }
            self.emit(Token::VRBrace, self.end_span);
            self.stack.pop();
        }
    }
}
