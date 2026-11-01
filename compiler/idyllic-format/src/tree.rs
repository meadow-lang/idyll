//! A lossless tree of a program: every byte of the source is in it, spaces
//! and comments as trivia, and the three tokens layout inserts as tokens of
//! no text. The parser (`idyllic-parser`) keeps none of that, and a formatter
//! has to.
//!
//! This is front/'s grammar (`front/src/Syntax.mw`), rule for rule, and what
//! `syntax!` makes of it: a node for each rule that is not a choice of rules,
//! and each choice made by looking ahead. `Reader` follows it by hand. It
//! only ever reads a program the parser has accepted, so what it cannot read
//! it gives up on rather than reports.
//!
//! Trivia goes where Lingua puts it: before a node opens, into the node
//! around it, and otherwise into the node being read.

use idyllic_layout::LayoutOutput;
use idyllic_lexer::{LexOutput, Token};

/// What a node or a token is, as far as a formatter's rules can tell. Every
/// rule of the grammar has a node; tokens are told apart only where a rule
/// names one by kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Item,
    ModuleDecl,
    AttrLine,
    AttrArgs,
    AttrArg,
    AttrValue,
    Line,
    Effects,
    Plain,
    Guards,
    Guard,
    WrittenSemi,
    LaidSemi,
    DataDecl,
    /// `Kind` in the grammar: a data type's `: Type`.
    DataKind,
    Ctors,
    Ctor,
    PlainCtor,
    ParenCtor,
    CtorSigs,
    CtorSig,
    RecordDecl,
    FieldDecl,
    TraitDecl,
    InstanceDecl,
    InstanceName,
    EffectDecl,
    OpBlock,
    OpSig,
    OpAttr,
    EffectAlias,
    UseDecl,
    UseAlias,
    UseNames,
    UseName,
    Lam,
    LetE,
    CaseE,
    Alt,
    AltPlain,
    AltGuards,
    AltGuard,
    IfE,
    HandleE,
    Clause,
    ClauseParam,
    Op,
    Annot,
    Arrow,
    Constraint,
    Chain,
    Link,
    App,
    Name,
    AsTail,
    Number,
    StrLit,
    CharLit,
    Hole,
    GoalA,
    TypeA,
    InfA,
    OpName,
    Paren,
    BoxA,
    SeqA,
    SeqRest,
    Braced,
    BraceRest,
    FieldVals,
    FieldVal,
    RecordLit,
    DotA,
    AutoA,
    InterpA,
    InterpRest,
    InterpHole,
    EmptyRow,

    // tokens
    OpenBrace,
    OpenLaid,
    CloseBrace,
    CloseLaid,
    Comma,
    Semi,
    LaidSep,
    Lower,
    Upper,
    Instance,
    InstanceNamed,
    Other,
    Space,
    Comment,
}

impl Kind {
    pub fn is_trivia(self) -> bool {
        matches!(self, Kind::Space | Kind::Comment)
    }
}

/// A token, trivia among them, and the bytes it covers.
#[derive(Clone, Debug)]
pub struct Leaf {
    pub kind: Kind,
    pub from: usize,
    pub to: usize,
}

#[derive(Clone, Debug)]
pub struct Branch {
    pub kind: Kind,
    /// Where it opened: after the trivia before it, which is not its own.
    pub from: usize,
    pub kids: Vec<Syntax>,
}

#[derive(Clone, Debug)]
pub enum Syntax {
    Leaf(Leaf),
    Branch(Branch),
}

impl Syntax {
    pub fn kind(&self) -> Kind {
        match self {
            Syntax::Leaf(l) => l.kind,
            Syntax::Branch(b) => b.kind,
        }
    }

    pub fn is_trivia(&self) -> bool {
        self.kind().is_trivia()
    }

    /// A token's text; a node has none of its own here.
    pub fn text<'s>(&self, src: &'s str) -> &'s str {
        match self {
            Syntax::Leaf(l) => &src[l.from..l.to],
            Syntax::Branch(_) => "",
        }
    }
}

// ---------------------------------------------------------------------------
// Tokens and trivia
// ---------------------------------------------------------------------------

/// A token of the source in the order the tree reads them: `token` is
/// `None` for trivia.
struct Lexeme {
    token: Option<Token>,
    kind: Kind,
    from: usize,
    to: usize,
}

/// The source's spaces, line ends and comments: everything between the
/// lexer's tokens. A run of spaces and line ends is one piece, as a blank
/// line is read off its text.
fn trivia_of(src: &str, lexed: &LexOutput) -> Vec<Leaf> {
    fn spaced(out: &mut Vec<Leaf>, from: usize, to: usize) {
        match out.last_mut() {
            Some(last) if last.kind == Kind::Space && last.to == from => last.to = to,
            _ => out.push(Leaf { kind: Kind::Space, from, to }),
        }
    }
    // what lies between two tokens is on one line: some space, and perhaps a
    // comment to its end
    fn gap(src: &str, out: &mut Vec<Leaf>, from: usize, to: usize) {
        let comment = src[from..to].find("--").map_or(to, |at| from + at);
        if comment > from {
            spaced(out, from, comment);
        }
        if comment < to {
            out.push(Leaf { kind: Kind::Comment, from: comment, to });
        }
    }
    let mut out = Vec::new();
    let mut at = 0;
    for t in &lexed.tokens {
        gap(src, &mut out, at, t.span.start);
        if t.node == Token::Newline {
            spaced(&mut out, t.span.start, t.span.end);
        }
        at = t.span.end;
    }
    gap(src, &mut out, at, src.len());
    out
}

/// Where the line a token ends at `at` ends, for what follows it: past a
/// comment after it on that line, if there is one.
fn line_end(src: &str, trivia: &[Leaf], at: usize) -> usize {
    let starting = |at: usize| trivia.binary_search_by_key(&at, |t| t.from).ok().map(|i| &trivia[i]);
    let past = match starting(at) {
        Some(t) if t.kind == Kind::Space && !src[t.from..t.to].contains('\n') => t.to,
        _ => at,
    };
    match starting(past) {
        Some(t) if t.kind == Kind::Comment => t.to,
        _ => at,
    }
}

fn kind_of(t: &Token) -> Kind {
    match t {
        Token::LBrace => Kind::OpenBrace,
        Token::VLBrace => Kind::OpenLaid,
        Token::RBrace => Kind::CloseBrace,
        Token::VRBrace => Kind::CloseLaid,
        Token::Comma => Kind::Comma,
        Token::Semi => Kind::Semi,
        Token::VSemi => Kind::LaidSep,
        Token::LowerIdent(_) => Kind::Lower,
        Token::UpperIdent(_) => Kind::Upper,
        _ => Kind::Other,
    }
}

/// The laid-out tokens and the trivia as one run, in the order of the text.
///
/// A block's virtual close and `;` stand where the last token before them
/// ends (or a comment after it on its line), not where the next one starts:
/// the blank lines and comments after a block are around it, not in it, and
/// those between two items are before the second. An empty block's open moves
/// with its close, and a block's open that is not empty stands at its first
/// token, so that what is before that is before the block. A virtual token
/// comes before trivia that starts where it stands.
fn lexemes(src: &str, trivia: Vec<Leaf>, laid: &LayoutOutput) -> Vec<Lexeme> {
    let tokens = &laid.tokens;
    let is_virtual = |t: &Token| matches!(t, Token::VLBrace | Token::VRBrace | Token::VSemi);
    // where the next real token starts: where a virtual one stands if it has
    // nothing to move back to
    let next_start = |i: usize| {
        if i == 0 {
            return 0;
        }
        tokens[i..].iter().find(|t| !is_virtual(&t.node)).map_or(src.len(), |t| t.span.start)
    };
    let mut placed: Vec<Lexeme> = Vec::with_capacity(tokens.len());
    let (mut last, mut movable) = (0, false);
    for (i, t) in tokens.iter().enumerate() {
        let at = |movable: bool| if movable { last } else { next_start(i) };
        let (from, to) = match t.node {
            Token::VRBrace | Token::VSemi => (at(movable), at(movable)),
            Token::VLBrace if tokens.get(i + 1).is_some_and(|n| n.node == Token::VRBrace) => (at(movable), at(movable)),
            Token::VLBrace => {
                movable = false;
                (at(false), at(false))
            }
            _ => {
                last = line_end(src, &trivia, t.span.end);
                movable = true;
                (t.span.start, t.span.end)
            }
        };
        placed.push(Lexeme { token: Some(t.node.clone()), kind: kind_of(&t.node), from, to });
    }
    // two runs, each in order; on a tie, the token first
    let mut out = Vec::with_capacity(placed.len() + trivia.len());
    let mut trivia = trivia.into_iter().peekable();
    for x in placed {
        while let Some(y) = trivia.next_if(|y| y.from < x.from) {
            out.push(Lexeme { token: None, kind: y.kind, from: y.from, to: y.to });
        }
        out.push(x);
    }
    out.extend(trivia.map(|y| Lexeme { token: None, kind: y.kind, from: y.from, to: y.to }));
    out
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Reading stopped, at this byte: something the grammar has no rule for.
pub type Read = Result<(), usize>;

struct Reader {
    lexemes: Vec<Lexeme>,
    /// Which lexemes are tokens, what a choice looks at, and where it is.
    tokens: Vec<usize>,
    pos: usize,
    /// The next lexeme to go into the tree.
    cursor: usize,
    /// The nodes open, innermost last.
    open: Vec<Branch>,
    offset: usize,
}

/// `src` as a tree, or the byte it could not be read past.
pub fn read(src: &str, lexed: &LexOutput, laid: &LayoutOutput) -> Result<Syntax, usize> {
    let lexemes = lexemes(src, trivia_of(src, lexed), laid);
    let tokens = (0..lexemes.len()).filter(|&i| lexemes[i].token.is_some()).collect();
    let mut r = Reader { lexemes, tokens, pos: 0, cursor: 0, open: Vec::new(), offset: 0 };
    r.file()?;
    // what is left, trailing space and comments, is the file's
    while r.cursor < r.lexemes.len() {
        r.take();
    }
    Ok(Syntax::Branch(r.open.pop().expect("the file's node stays open")))
}

impl Reader {
    fn nth(&self, n: usize) -> Option<&Token> {
        self.tokens.get(self.pos + n).and_then(|&i| self.lexemes[i].token.as_ref())
    }

    fn at(&self, t: Token) -> bool {
        self.nth(0) == Some(&t)
    }

    fn nth_is(&self, n: usize, t: Token) -> bool {
        self.nth(n) == Some(&t)
    }

    fn nth_lower(&self, n: usize) -> bool {
        matches!(self.nth(n), Some(Token::LowerIdent(_)))
    }

    fn nth_upper(&self, n: usize) -> bool {
        matches!(self.nth(n), Some(Token::UpperIdent(_)))
    }

    fn nth_op(&self, n: usize) -> bool {
        matches!(self.nth(n), Some(Token::OpIdent(_) | Token::ConOpIdent(_)))
    }

    fn at_word(&self, word: &str) -> bool {
        matches!(self.nth(0), Some(Token::LowerIdent(w)) if w == word)
    }

    /// Where reading stopped.
    fn stuck(&self) -> usize {
        self.tokens.get(self.pos).map_or(self.offset, |&i| self.lexemes[i].from)
    }

    /// The next lexeme, into the node being read.
    fn take(&mut self) {
        let l = &self.lexemes[self.cursor];
        let leaf = Leaf { kind: l.kind, from: l.from, to: l.to };
        self.cursor += 1;
        self.offset = leaf.to;
        self.open.last_mut().expect("a node is open").kids.push(Syntax::Leaf(leaf));
    }

    fn trivia(&mut self) {
        while self.lexemes.get(self.cursor).is_some_and(|l| l.token.is_none()) {
            self.take();
        }
    }

    fn advance(&mut self) {
        self.trivia();
        self.take();
        self.pos += 1;
    }

    /// Take the next token, as a kind of its own by where it is.
    fn advance_as(&mut self, kind: Kind) {
        let i = self.tokens[self.pos];
        self.lexemes[i].kind = kind;
        self.advance();
    }

    fn expect(&mut self, t: Token) -> Read {
        if self.at(t) {
            self.advance();
            Ok(())
        } else {
            Err(self.stuck())
        }
    }

    fn expect_lower(&mut self) -> Read {
        if self.nth_lower(0) {
            self.advance();
            Ok(())
        } else {
            Err(self.stuck())
        }
    }

    fn expect_upper(&mut self) -> Read {
        if self.nth_upper(0) {
            self.advance();
            Ok(())
        } else {
            Err(self.stuck())
        }
    }

    fn node(&mut self, kind: Kind, body: impl FnOnce(&mut Self) -> Read) -> Read {
        if !self.open.is_empty() {
            self.trivia();
        }
        self.open.push(Branch { kind, from: self.offset, kids: Vec::new() });
        body(self)?;
        let done = self.open.pop().expect("the node just opened");
        self.open.last_mut().expect("a node is open").kids.push(Syntax::Branch(done));
        Ok(())
    }

    fn file(&mut self) -> Read {
        self.open.push(Branch { kind: Kind::File, from: 0, kids: Vec::new() });
        self.expect(Token::VLBrace)?;
        self.entries(Self::item)?;
        self.expect(Token::VRBrace)
    }

    /// A block's items and what parts them, up to what closes it.
    fn entries(&mut self, item: fn(&mut Self) -> Read) -> Read {
        loop {
            match self.nth(0) {
                Some(Token::Semi) => self.node(Kind::WrittenSemi, |r| Ok(r.advance()))?,
                Some(Token::VSemi) => self.node(Kind::LaidSemi, |r| Ok(r.advance()))?,
                Some(Token::RBrace | Token::VRBrace) | None => return Ok(()),
                _ => item(self)?,
            }
        }
    }

    /// `open:("{" | "vlbrace") entries close:("}" | "vrbrace")`
    fn block(&mut self, item: fn(&mut Self) -> Read) -> Read {
        if !matches!(self.nth(0), Some(Token::LBrace | Token::VLBrace)) {
            return Err(self.stuck());
        }
        self.advance();
        self.entries(item)?;
        if !matches!(self.nth(0), Some(Token::RBrace | Token::VRBrace)) {
            return Err(self.stuck());
        }
        self.advance();
        Ok(())
    }

    // --- items ---

    fn item(&mut self) -> Read {
        self.node(Kind::Item, |r| {
            while r.at(Token::At) && r.nth_lower(1) {
                r.attr_line()?;
            }
            match r.nth(0) {
                Some(Token::Module) => r.module_decl(),
                Some(Token::Data) => r.data_decl(),
                Some(Token::Effect) => r.effect_decl(),
                Some(Token::Use) => r.use_decl(),
                _ if r.at_word("record") && r.nth_upper(1) => r.record_decl(),
                _ if r.at_word("trait") && r.nth_upper(1) => r.trait_decl(),
                _ if r.at_word("instance") && (r.nth_upper(1) || r.nth_lower(1)) => r.instance_decl(),
                _ => r.line(),
            }
        })
    }

    fn module_decl(&mut self) -> Read {
        self.node(Kind::ModuleDecl, |r| {
            r.advance();
            r.expect_upper()?;
            r.expect(Token::Where)?;
            r.block(Self::item)
        })
    }

    fn attr_line(&mut self) -> Read {
        self.node(Kind::AttrLine, |r| {
            r.advance();
            r.advance();
            if r.at(Token::LParen) {
                r.attr_args()?;
            }
            if matches!(r.nth(0), Some(Token::Semi | Token::VSemi)) {
                r.advance();
            }
            Ok(())
        })
    }

    fn attr_args(&mut self) -> Read {
        self.node(Kind::AttrArgs, |r| {
            r.advance();
            if !r.at(Token::RParen) {
                r.attr_arg()?;
                while r.at(Token::Comma) {
                    r.advance();
                    r.attr_arg()?;
                }
            }
            r.expect(Token::RParen)
        })
    }

    fn attr_arg(&mut self) -> Read {
        self.node(Kind::AttrArg, |r| {
            if !r.nth_lower(0) && !r.nth_upper(0) {
                return Err(r.stuck());
            }
            r.advance();
            if r.at(Token::Eq) {
                r.node(Kind::AttrValue, |r| {
                    r.advance();
                    r.ctor_name()
                })?;
            }
            Ok(())
        })
    }

    /// An equation, a signature, or the program's expression.
    fn line(&mut self) -> Read {
        self.node(Kind::Line, |r| {
            r.term()?;
            if r.at(Token::Bang) {
                r.effects()?;
            }
            if r.at(Token::Eq) {
                r.node(Kind::Plain, |r| {
                    r.advance();
                    r.term()
                })
            } else if r.at(Token::Bar) {
                r.node(Kind::Guards, |r| {
                    while r.at(Token::Bar) {
                        r.node(Kind::Guard, |r| {
                            r.advance();
                            r.term()?;
                            r.expect(Token::Eq)?;
                            r.term()
                        })?;
                    }
                    Ok(())
                })
            } else {
                Ok(())
            }
        })
    }

    fn effects(&mut self) -> Read {
        self.node(Kind::Effects, |r| {
            r.advance();
            r.row()
        })
    }

    fn row(&mut self) -> Read {
        if self.at(Token::LBrace) && self.nth_is(1, Token::RBrace) {
            self.node(Kind::EmptyRow, |r| {
                r.advance();
                r.advance();
                Ok(())
            })
        } else {
            self.app()
        }
    }

    fn atoms(&mut self) -> Read {
        while self.atom_at(0) {
            self.atom()?;
        }
        Ok(())
    }

    fn data_decl(&mut self) -> Read {
        self.node(Kind::DataDecl, |r| {
            r.advance();
            r.expect_upper()?;
            r.atoms()?;
            if r.at(Token::Colon) {
                r.node(Kind::DataKind, |r| {
                    r.advance();
                    r.term()
                })?;
            }
            if r.at(Token::Eq) {
                r.node(Kind::Ctors, |r| {
                    r.advance();
                    r.ctor()?;
                    while r.at(Token::Bar) {
                        r.advance();
                        r.ctor()?;
                    }
                    Ok(())
                })?;
            } else if r.at(Token::Where) {
                r.node(Kind::CtorSigs, |r| {
                    r.advance();
                    r.block(Self::ctor_sig)
                })?;
            }
            Ok(())
        })
    }

    fn ctor(&mut self) -> Read {
        self.node(Kind::Ctor, |r| {
            r.ctor_name()?;
            r.atoms()
        })
    }

    fn ctor_name(&mut self) -> Read {
        if self.at(Token::LParen) {
            self.node(Kind::ParenCtor, |r| {
                r.advance();
                if !r.nth_upper(0) && !matches!(r.nth(0), Some(Token::ConOpIdent(_))) {
                    return Err(r.stuck());
                }
                r.advance();
                r.expect(Token::RParen)
            })
        } else {
            self.node(Kind::PlainCtor, Self::expect_upper)
        }
    }

    fn ctor_sig(&mut self) -> Read {
        self.node(Kind::CtorSig, |r| {
            r.ctor_name()?;
            r.expect(Token::Colon)?;
            r.term()
        })
    }

    fn record_decl(&mut self) -> Read {
        self.node(Kind::RecordDecl, |r| {
            r.advance();
            r.expect_upper()?;
            r.atoms()?;
            r.expect(Token::Eq)?;
            r.expect(Token::LBrace)?;
            if r.nth_lower(0) {
                r.field_decl()?;
                while r.at(Token::Comma) && r.nth_lower(1) {
                    r.advance();
                    r.field_decl()?;
                }
                if r.at(Token::Comma) {
                    r.advance();
                }
            }
            r.expect(Token::RBrace)
        })
    }

    fn field_decl(&mut self) -> Read {
        self.node(Kind::FieldDecl, |r| {
            r.advance();
            r.expect(Token::Colon)?;
            r.term()
        })
    }

    fn trait_decl(&mut self) -> Read {
        self.node(Kind::TraitDecl, |r| {
            r.advance();
            r.term()?;
            r.expect(Token::Where)?;
            r.block(Self::line)
        })
    }

    fn instance_decl(&mut self) -> Read {
        self.node(Kind::InstanceDecl, |r| {
            r.advance_as(if r.nth_upper(1) { Kind::Instance } else { Kind::InstanceNamed });
            if r.nth_lower(0) && r.nth_is(1, Token::Colon) {
                r.node(Kind::InstanceName, |r| {
                    r.advance();
                    r.advance();
                    Ok(())
                })?;
            }
            r.term()?;
            r.expect(Token::Where)?;
            r.block(Self::line)
        })
    }

    fn effect_decl(&mut self) -> Read {
        self.node(Kind::EffectDecl, |r| {
            r.advance();
            r.expect_upper()?;
            r.atoms()?;
            if r.at(Token::Where) {
                r.node(Kind::OpBlock, |r| {
                    r.advance();
                    r.block(Self::op_sig)
                })
            } else if r.at(Token::Eq) {
                r.node(Kind::EffectAlias, |r| {
                    r.advance();
                    r.row()
                })
            } else {
                Err(r.stuck())
            }
        })
    }

    fn op_sig(&mut self) -> Read {
        self.node(Kind::OpSig, |r| {
            if r.at(Token::At) {
                r.node(Kind::OpAttr, |r| {
                    r.advance();
                    r.expect_lower()
                })?;
            }
            r.expect_lower()?;
            r.expect(Token::Colon)?;
            r.term()
        })
    }

    fn use_decl(&mut self) -> Read {
        self.node(Kind::UseDecl, |r| {
            r.advance();
            if !r.nth_upper(0) && !matches!(r.nth(0), Some(Token::QualUpper(_))) {
                return Err(r.stuck());
            }
            r.advance();
            if matches!(r.nth(0), Some(Token::OpIdent(_))) {
                r.advance();
            }
            if r.nth_lower(0) {
                r.node(Kind::UseAlias, |r| {
                    r.advance();
                    r.expect_upper()
                })?;
            }
            if r.at(Token::LParen) {
                r.node(Kind::UseNames, |r| {
                    r.advance();
                    if !r.at(Token::RParen) {
                        r.use_name()?;
                        while r.at(Token::Comma) {
                            r.advance();
                            r.use_name()?;
                        }
                    }
                    r.expect(Token::RParen)
                })?;
            }
            Ok(())
        })
    }

    fn use_name(&mut self) -> Read {
        self.node(Kind::UseName, |r| {
            if r.at(Token::LParen) {
                r.advance();
                if !r.nth_op(0) {
                    return Err(r.stuck());
                }
                r.advance();
                r.expect(Token::RParen)
            } else if r.nth_lower(0) || r.nth_upper(0) {
                r.advance();
                Ok(())
            } else {
                Err(r.stuck())
            }
        })
    }

    // --- terms ---

    fn term(&mut self) -> Read {
        match self.nth(0) {
            Some(Token::BackSlash) => self.node(Kind::Lam, |r| {
                r.advance();
                r.atom()?;
                r.atoms()?;
                r.expect(Token::RArrow)?;
                r.term()
            }),
            Some(Token::Let) => self.node(Kind::LetE, |r| {
                r.advance();
                r.block(Self::line)?;
                r.expect(Token::In)?;
                r.term()
            }),
            Some(Token::Case) => self.node(Kind::CaseE, |r| {
                r.advance();
                r.term()?;
                r.expect(Token::Of)?;
                r.block(Self::alt)
            }),
            Some(Token::If) => self.node(Kind::IfE, |r| {
                r.advance();
                r.term()?;
                r.expect(Token::Then)?;
                r.term()?;
                r.expect(Token::Else)?;
                r.term()
            }),
            Some(Token::Handle) => self.node(Kind::HandleE, |r| {
                r.advance();
                r.term()?;
                r.expect(Token::With)?;
                r.block(Self::clause)
            }),
            _ => self.op(),
        }
    }

    fn alt(&mut self) -> Read {
        self.node(Kind::Alt, |r| {
            r.chain()?;
            if r.at(Token::RArrow) {
                r.node(Kind::AltPlain, |r| {
                    r.advance();
                    r.term()
                })
            } else if r.at(Token::Bar) {
                r.node(Kind::AltGuards, |r| {
                    while r.at(Token::Bar) {
                        r.node(Kind::AltGuard, |r| {
                            r.advance();
                            r.chain()?;
                            r.expect(Token::RArrow)?;
                            r.term()
                        })?;
                    }
                    Ok(())
                })
            } else {
                Ok(())
            }
        })
    }

    fn clause(&mut self) -> Read {
        self.node(Kind::Clause, |r| {
            r.expect_lower()?;
            while r.nth_lower(0) || r.at(Token::Underscore) {
                r.node(Kind::ClauseParam, |r| Ok(r.advance()))?;
            }
            r.expect(Token::RArrow)?;
            r.term()
        })
    }

    /// A chain of operators, then perhaps an annotation or an arrow.
    fn op(&mut self) -> Read {
        self.node(Kind::Op, |r| {
            r.chain()?;
            match r.nth(0) {
                Some(Token::Colon) => r.node(Kind::Annot, |r| {
                    r.advance();
                    r.term()
                }),
                Some(Token::RArrow) => r.arrow(Kind::Arrow),
                Some(Token::RFatArrow) => r.arrow(Kind::Constraint),
                _ => Ok(()),
            }
        })
    }

    fn arrow(&mut self, kind: Kind) -> Read {
        self.node(kind, |r| {
            r.advance();
            r.term()?;
            if r.at(Token::Bang) {
                r.effects()?;
            }
            Ok(())
        })
    }

    fn chain(&mut self) -> Read {
        self.node(Kind::Chain, |r| {
            r.app()?;
            // an operator with nothing to take is not a link: the `:?` of `${x:?}`
            while r.nth_op(0) && r.atom_at(1) {
                r.node(Kind::Link, |r| {
                    r.advance();
                    r.app()
                })?;
            }
            Ok(())
        })
    }

    fn app(&mut self) -> Read {
        self.node(Kind::App, |r| {
            r.atom()?;
            r.atoms()
        })
    }

    /// Whether an atom begins `n` tokens ahead.
    fn atom_at(&self, n: usize) -> bool {
        match self.nth(n) {
            Some(
                Token::LowerIdent(_)
                | Token::UpperIdent(_)
                | Token::QualUpper(_)
                | Token::QualLower(_)
                | Token::Int(_)
                | Token::Float(_)
                | Token::Str(_)
                | Token::Char(_)
                | Token::Underscore
                | Token::Goal(_)
                | Token::Type
                | Token::Infinity
                | Token::LParen
                | Token::LBracket
                | Token::InterpStart(_),
            ) => true,
            Some(Token::Hash) => self.nth_is(n + 1, Token::LBracket),
            Some(Token::LBrace) => !self.nth_is(n + 1, Token::RBrace),
            Some(Token::Period) => self.atom_at(n + 1),
            Some(Token::At) => self.nth_is(n + 1, Token::LBrace),
            _ => false,
        }
    }

    fn atom(&mut self) -> Read {
        match self.nth(0) {
            Some(Token::LowerIdent(_) | Token::QualLower(_)) => self.name(),
            Some(Token::UpperIdent(_) | Token::QualUpper(_)) => {
                // `T { x = 1 }` is a record; `T {a}` a name and what is given it
                let record = self.nth_is(1, Token::LBrace)
                    && (self.nth_is(2, Token::RBrace) || (self.nth_lower(2) && self.nth_is(3, Token::Eq)));
                if record {
                    self.record_lit()
                } else {
                    self.name()
                }
            }
            Some(Token::Int(_) | Token::Float(_)) => self.node(Kind::Number, |r| Ok(r.advance())),
            Some(Token::Str(_)) => self.node(Kind::StrLit, |r| Ok(r.advance())),
            Some(Token::Char(_)) => self.node(Kind::CharLit, |r| Ok(r.advance())),
            Some(Token::Underscore) => self.node(Kind::Hole, |r| Ok(r.advance())),
            Some(Token::Goal(_)) => self.node(Kind::GoalA, |r| Ok(r.advance())),
            Some(Token::Type) => self.node(Kind::TypeA, |r| Ok(r.advance())),
            Some(Token::Infinity) => self.node(Kind::InfA, |r| Ok(r.advance())),
            Some(Token::LParen) if self.nth_op(1) => self.node(Kind::OpName, |r| {
                r.advance();
                r.advance();
                r.expect(Token::RParen)
            }),
            Some(Token::LParen) => self.node(Kind::Paren, |r| {
                r.advance();
                if !r.at(Token::RParen) {
                    r.term()?;
                    while r.at(Token::Comma) {
                        r.advance();
                        r.term()?;
                    }
                }
                r.expect(Token::RParen)
            }),
            Some(Token::LBracket) => self.node(Kind::BoxA, |r| {
                r.advance();
                if !r.at(Token::RBracket) {
                    r.term()?;
                }
                r.expect(Token::RBracket)
            }),
            Some(Token::Hash) => self.node(Kind::SeqA, |r| {
                r.advance();
                r.expect(Token::LBracket)?;
                if !matches!(r.nth(0), Some(Token::RBracket | Token::Comma | Token::Semi)) {
                    r.term()?;
                }
                while matches!(r.nth(0), Some(Token::Comma | Token::Semi)) {
                    r.node(Kind::SeqRest, |r| {
                        r.advance();
                        if !matches!(r.nth(0), Some(Token::RBracket | Token::Comma | Token::Semi)) {
                            r.term()?;
                        }
                        Ok(())
                    })?;
                }
                r.expect(Token::RBracket)
            }),
            Some(Token::LBrace) => self.node(Kind::Braced, |r| {
                r.advance();
                r.term()?;
                while r.at(Token::Comma) {
                    r.advance();
                    r.term()?;
                }
                if r.at(Token::Bar) {
                    r.node(Kind::BraceRest, |r| {
                        r.advance();
                        if r.nth_lower(0) && r.nth_is(1, Token::Eq) {
                            r.field_vals()
                        } else {
                            r.term()
                        }
                    })?;
                }
                r.expect(Token::RBrace)
            }),
            Some(Token::Period) => self.node(Kind::DotA, |r| {
                r.advance();
                r.atom()
            }),
            Some(Token::At) => self.node(Kind::AutoA, |r| {
                r.advance();
                r.expect(Token::LBrace)?;
                r.term()?;
                r.expect(Token::RBrace)
            }),
            Some(Token::InterpStart(_)) => self.node(Kind::InterpA, |r| {
                r.advance();
                r.interp_hole()?;
                while matches!(r.nth(0), Some(Token::InterpMid(_))) {
                    r.node(Kind::InterpRest, |r| {
                        r.advance();
                        r.interp_hole()
                    })?;
                }
                if matches!(r.nth(0), Some(Token::InterpEnd(_))) {
                    r.advance();
                    Ok(())
                } else {
                    Err(r.stuck())
                }
            }),
            _ => Err(self.stuck()),
        }
    }

    fn interp_hole(&mut self) -> Read {
        self.node(Kind::InterpHole, |r| {
            r.term()?;
            if matches!(r.nth(0), Some(Token::ConOpIdent(_))) {
                r.advance();
            }
            Ok(())
        })
    }

    fn name(&mut self) -> Read {
        self.node(Kind::Name, |r| {
            r.advance();
            // `x@p`, and `f@{d}`, which is told apart after
            if r.at(Token::At) && r.atom_at(1) {
                r.node(Kind::AsTail, |r| {
                    r.advance();
                    r.atom()
                })?;
            }
            Ok(())
        })
    }

    fn record_lit(&mut self) -> Read {
        self.node(Kind::RecordLit, |r| {
            r.advance();
            r.advance();
            if !r.at(Token::RBrace) {
                r.field_vals()?;
            }
            r.expect(Token::RBrace)
        })
    }

    fn field_vals(&mut self) -> Read {
        self.node(Kind::FieldVals, |r| {
            r.field_val()?;
            while r.at(Token::Comma) && r.nth_lower(1) {
                r.advance();
                r.field_val()?;
            }
            if r.at(Token::Comma) {
                r.advance();
            }
            Ok(())
        })
    }

    fn field_val(&mut self) -> Read {
        self.node(Kind::FieldVal, |r| {
            r.expect_lower()?;
            r.expect(Token::Eq)?;
            r.term()
        })
    }
}
