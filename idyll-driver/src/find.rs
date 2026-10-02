//! **Finding a declaration, by name or by type**: what the REPL's `Ctrl-F`
//! finder and `:find` search.  Ported from Meadow's `meadow-find`.
//!
//! A query is read one of two ways, and [`Query::parse`] decides which:
//!
//! - **By name**, fuzzily, the way Helix's pickers match (the same matcher,
//!   `nucleo`): `shw` finds `showInt`, and `Nat.` the constructors of `Nat`.
//! - **By type**, the way Hoogle does, when the query is a type rather than a
//!   name: it has an arrow in it, or starts with `:`.  `Int -> String` finds
//!   `showInt`; `: String` finds what makes one.  See [`shape`] for what
//!   counts as a match, and in what order.
//!
//! A third reading belongs to the finder and `:prove`: a query starting with
//! `?` asks for a *term* of a type, which [`crate::prove`] searches for.

use idyllic::pipeline::Declaration;
use idyllic_typecheck::DeclKind;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// One thing a program can name.
#[derive(Debug, Clone)]
pub struct Decl {
    /// as a program writes it from outside: `showInt`, `Nat.Z`
    pub name: String,
    /// what it is spelled as at the prompt: the same, but for a
    /// constructor whose type's constructors are in scope bare (`Just`), and
    /// an operator, which is written in parentheses as a name (`(++)`)
    pub spelling: String,
    /// `Prelude`, a dependency's name (`Std`), or the empty string for the
    /// session's own
    pub module: String,
    pub kind: Kind,
    /// its type (a type's or an effect's kind), as written
    pub detail: String,
    /// its type, as a search compares it
    pub shape: Option<shape::Sig>,
    pub doc: Option<String>,
    /// whether the prelude or a dependency declares it, rather than the
    /// session: the session's own come first among equals
    pub prelude: bool,
    /// where it was written: the file, and its line there
    pub location: Option<(String, usize)>,
}

impl Decl {
    /// `Prelude.showInt`.
    pub fn qualified(&self) -> String {
        // (a dependency's names are written in full already)
        if self.module.is_empty() || self.name.starts_with(&format!("{}.", self.module)) {
            self.name.clone()
        } else {
            format!("{}.{}", self.module, self.name)
        }
    }

    /// One line saying what it is: `showInt : (n : [] Int) -> String`.
    pub fn headline(&self) -> String {
        format!("{} : {}", self.name, self.detail)
    }

    /// Every declaration a compiled entry knows of, spelled for the prompt:
    /// `bare` says which types' constructors are in scope without their
    /// type's name.
    pub fn all(declarations: &[Declaration], bare: &dyn Fn(&str) -> bool) -> Vec<Decl> {
        declarations
            .iter()
            .filter(|d| !d.name.starts_with("__"))
            .map(|d| {
                let kind = Kind::of(d);
                let spelling = match (kind, d.name.rsplit_once('.')) {
                    (Kind::Constructor, Some((ty, ctor))) if bare(ty) => operator(ctor),
                    _ => operator(&d.name),
                };
                Decl {
                    name: d.name.clone(),
                    spelling,
                    module: match &d.package {
                        Some(p) => p.clone(),
                        None if d.prelude => "Prelude".into(),
                        None => String::new(),
                    },
                    kind,
                    detail: d.shown.clone(),
                    shape: kind.is_value().then(|| shape::Sig::of_core(&d.ty)),
                    doc: d.doc.clone(),
                    prelude: d.prelude || d.package.is_some(),
                    location: (!d.prelude && d.package.is_none()).then(|| (d.file.clone(), d.line)),
                }
            })
            .collect()
    }
}

/// Whether the constructors of the type `ty` are in scope without its name,
/// after the declarations `source`: the prelude's `List`, `Maybe` and
/// `Result` are, and so is a type `use`d.
pub fn bare(source: &str, ty: &str) -> bool {
    matches!(ty, "List" | "Maybe" | "Result") || source.lines().any(|l| l.trim_start().starts_with(&format!("use {ty}.")))
}

/// A name, as an expression writes it: an operator in parentheses.
fn operator(name: &str) -> String {
    let last = name.rsplit('.').next().unwrap_or(name);
    if last.chars().next().is_some_and(|c| !c.is_alphanumeric() && c != '_') {
        format!("({last})")
    } else {
        name.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Function,
    Value,
    Constructor,
    Operation,
    /// a data type
    Type,
    Effect,
    /// a definition that is a type or an effect row: `Eff`
    Alias,
}

impl Kind {
    fn of(d: &Declaration) -> Kind {
        use idyllic_typecheck::core::Core;
        match d.kind {
            DeclKind::Constructor => Kind::Constructor,
            DeclKind::Operation => Kind::Operation,
            DeclKind::Type => Kind::Type,
            DeclKind::Effect => Kind::Effect,
            DeclKind::Value => {
                // what a definition's type ends in says what it is
                let mut ty = &*d.ty;
                while let Core::Pi { cod, .. } = ty {
                    ty = cod;
                }
                let is_type = matches!(ty, Core::Type(_)) || d.shown.ends_with("Effect") || d.shown.ends_with("Effect []");
                match &*d.ty {
                    _ if is_type => Kind::Alias,
                    Core::Pi { .. } => Kind::Function,
                    _ => Kind::Value,
                }
            }
        }
    }

    /// Something with a type of its own, that a program can use as a value.
    pub fn is_value(self) -> bool {
        matches!(self, Kind::Function | Kind::Value | Kind::Constructor | Kind::Operation)
    }

    /// A word for it, short enough for a column.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Function => "fun",
            Kind::Value => "val",
            Kind::Constructor => "ctor",
            Kind::Operation => "op",
            Kind::Type => "data",
            Kind::Effect => "effect",
            Kind::Alias => "type",
        }
    }
}

/// What somebody typed, read as a name or as a type.
#[derive(Debug, Clone)]
pub enum Query {
    /// Nothing typed yet: everything, in order.
    Empty,
    Name(String),
    Type(shape::Sig),
}

impl Query {
    /// A type when it looks like one -- an arrow, or a leading `:` to say so
    /// outright -- and a name otherwise.  A bare `Maybe` is a name: that is
    /// how most people would look for the type; `: Maybe a` asks for what
    /// makes one.
    pub fn parse(text: &str) -> Query {
        let t = text.trim();
        if t.is_empty() {
            return Query::Empty;
        }
        if let Some(rest) = t.strip_prefix(':') {
            return if rest.trim().is_empty() { Query::Empty } else { Query::Type(shape::parse(rest)) };
        }
        if t.contains("->") {
            return Query::Type(shape::parse(t));
        }
        Query::Name(t.to_string())
    }

    pub fn is_type(&self) -> bool {
        matches!(self, Query::Type(_))
    }
}

/// One answer: which declaration, and how good an answer it is -- higher is
/// better, whichever way the query was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub decl: usize,
    pub score: i64,
}

/// Every declaration a search can find.
#[derive(Debug, Clone, Default)]
pub struct Index {
    pub decls: Vec<Decl>,
}

impl Index {
    pub fn new(decls: Vec<Decl>) -> Index {
        Index { decls }
    }

    pub fn len(&self) -> usize {
        self.decls.len()
    }

    pub fn is_empty(&self) -> bool {
        self.decls.is_empty()
    }

    /// The best `limit` answers to `text`, best first; the session's own
    /// declarations win a tie with the prelude's.
    pub fn search(&self, text: &str, limit: usize) -> Vec<Hit> {
        let query = Query::parse(text);
        let mut hits = match &query {
            Query::Empty => self.decls.iter().enumerate().map(|(i, _)| Hit { decl: i, score: 0 }).collect(),
            Query::Name(text) => self.by_name(text),
            Query::Type(sig) => self.by_type(sig),
        };
        hits.sort_by(|a, b| {
            let (x, y) = (&self.decls[a.decl], &self.decls[b.decl]);
            b.score
                .cmp(&a.score)
                .then(x.prelude.cmp(&y.prelude))
                // among equals, the shorter name is the likelier one
                .then(x.name.len().cmp(&y.name.len()))
                .then(x.name.cmp(&y.name))
        });
        hits.truncate(limit);
        hits
    }

    /// Fuzzy, against the name.
    fn by_name(&self, text: &str) -> Vec<Hit> {
        let pattern = Pattern::parse(text, CaseMatching::Smart, Normalization::Smart);
        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut buf = Vec::new();
        let wanted = text.trim();
        self.decls
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                let hay = d.name.as_str();
                let score = pattern.score(Utf32Str::new(hay, &mut buf), &mut matcher)?;
                // the name itself, exactly, above anything that merely
                // contains it; and exactly as typed above exactly but for
                // case
                let last = hay.rsplit('.').next().unwrap_or(hay);
                let bonus = if hay == wanted || last == wanted {
                    1 << 20
                } else if hay.eq_ignore_ascii_case(wanted) || last.eq_ignore_ascii_case(wanted) {
                    1 << 19
                } else {
                    0
                };
                Some(Hit { decl: i, score: score as i64 + bonus })
            })
            .collect()
    }

    fn by_type(&self, sig: &shape::Sig) -> Vec<Hit> {
        self.decls
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                let cost = shape::cost(sig, d.shape.as_ref()?)?;
                Some(Hit { decl: i, score: -(cost as i64) })
            })
            .collect()
    }
}

pub mod shape {
    //! **Types as a search sees them**, and what it means for one to match
    //! another.
    //!
    //! The checker's types carry what checking needs: grades, effect rows,
    //! dependency, implicit binders.  A search needs less.  A signature is
    //! flattened into a [`Sig`] -- its explicit arguments in order, and its
    //! result -- over a small [`Ty`]: an implicit binder (`{a : Type} ->`)
    //! makes a type variable, a grade (`(n : [] Int)`, `a [2]`) is left out,
    //! and so is an effect (`! Console`).  A term in a type (`Vec 3 a`) is
    //! compared as a type is: a number is a name, a constructor applied is
    //! a name applied.
    //!
    //! # Matching the way Hoogle does
    //!
    //! A match is a unification of the query with the declaration, their
    //! variables kept apart, under every way of pairing the query's arguments
    //! with the declaration's -- and a match is then *priced*, cheapest
    //! first:
    //!
    //! | what happened                                             | cost |
    //! | --------------------------------------------------------- | ---- |
    //! | the same type, up to naming its variables                 |    0 |
    //! | the arguments came in another order                       |    1 |
    //! | a variable of the declaration had to be a particular type |    1 |
    //! | the declaration takes an argument the query did not give  |    2 |
    //! | the declaration forces two of the query's variables equal |    2 |
    //! | a variable of the query had to be a particular type       |    3 |
    //! | the declaration's result is only a variable               |    3 |
    //!
    //! The result has to unify or there is no match at all.  Everything else
    //! is a price, which is what lets a search half typed still show
    //! something useful.

    use idyllic_ast::Icit;
    use idyllic_typecheck::core::Core;
    use std::collections::HashMap;

    /// A type, as simple as matching can make do with.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub enum Ty {
        /// A type variable, numbered within its signature.
        Var(u32),
        /// A named type applied to its arguments: `Int`, `List a`, `()`.
        Con(String, Vec<Ty>),
        /// A function taken or returned as a value, its arguments flattened
        /// as a [`Sig`]'s are.
        Fun(Vec<Ty>, Box<Ty>),
        /// Anything at all: `_` in a query, the part of a query not typed
        /// yet, or something the checker could not say.
        Any,
    }

    /// A signature flattened for comparison.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Sig {
        pub args: Vec<Ty>,
        pub result: Ty,
    }

    impl Sig {
        /// A declaration's type, as a search compares it.
        pub fn of_core(ty: &Core) -> Sig {
            let mut vars = Vec::new();
            let mut next = 0;
            match flatten(ty, &mut vars, &mut next) {
                Ty::Fun(args, result) => Sig { args, result: *result },
                result => Sig { args: Vec::new(), result },
            }
        }

        /// The signature as a type: a function of its arguments, if it has
        /// any.
        pub fn as_ty(&self) -> Ty {
            if self.args.is_empty() { self.result.clone() } else { Ty::Fun(self.args.clone(), Box::new(self.result.clone())) }
        }
    }

    /// A type, its binders' variables in `vars` (innermost last), numbering
    /// new ones from `next`.
    fn flatten(ty: &Core, vars: &mut Vec<Ty>, next: &mut u32) -> Ty {
        match ty {
            Core::Pi { icit, dom, cod, .. } => {
                let dom_ty = flatten(dom, vars, next);
                // whatever the binder is, a use of it in what follows is a
                // variable
                vars.push(Ty::Var(*next));
                *next += 1;
                let rest = flatten(cod, vars, next);
                vars.pop();
                if *icit == Icit::Implicit {
                    return rest;
                }
                match rest {
                    Ty::Fun(mut args, result) => {
                        args.insert(0, dom_ty);
                        Ty::Fun(args, result)
                    }
                    result => Ty::Fun(vec![dom_ty], Box::new(result)),
                }
            }
            Core::Comp(t, _) | Core::BoxTy(_, t) | Core::Runtime(_, t) => flatten(t, vars, next),
            Core::Type(_) => Ty::Con("Type".into(), Vec::new()),
            Core::LitTy(b) => Ty::Con(Core::LitTy(*b).to_string(), Vec::new()),
            Core::Lit(l) => Ty::Con(l.to_string(), Vec::new()),
            Core::Var(i, name) => match vars.len().checked_sub(i + 1) {
                Some(at) => vars[at].clone(),
                None => Ty::Con(spelling(name), Vec::new()),
            },
            Core::Opaque(_, name) => Ty::Con(spelling(name), Vec::new()),
            Core::App(..) => {
                let mut head = ty;
                let mut args = Vec::new();
                while let Core::App(f, a) = head {
                    args.push(flatten(a, vars, next));
                    head = f;
                }
                args.reverse();
                match flatten(head, vars, next) {
                    Ty::Con(name, mut already) => {
                        already.extend(args);
                        Ty::Con(name, already)
                    }
                    _ => Ty::Any,
                }
            }
            _ => Ty::Any,
        }
    }

    /// `Nat.Z` is `Z`, and `Math.Vec` is `Vec`: a search matches spellings.
    fn spelling(name: &str) -> String {
        name.rsplit('.').next().unwrap_or(name).to_string()
    }

    // --- a query, as somebody types it ------------------------------------

    /// Read a type the way it is written in a signature, and forgive
    /// whatever has not been typed yet: `List a -> ` is a function from
    /// `List a` to anything, and `Maybe` with nothing after it is `Maybe` of
    /// anything.  Implicit binders (`{a : Type} ->`), named ones (`(n : Int)
    /// ->`) and grades (`[] Int`, `a [2]`) are read and left out; an
    /// effect's `!` is where comparing stops.
    pub fn parse(query: &str) -> Sig {
        let mut p = Parser { tokens: lex(query), at: 0, vars: HashMap::new() };
        match p.arrow() {
            Ty::Fun(args, result) => Sig { args, result: *result },
            result => Sig { args: Vec::new(), result },
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Tok {
        Arrow,
        Open,
        Close,
        OpenGrade,
        CloseGrade,
        OpenBrace,
        CloseBrace,
        Colon,
        Bang,
        Upper(String),
        Lower(String),
        Number(String),
        Hole,
    }

    fn lex(s: &str) -> Vec<Tok> {
        let chars: Vec<char> = s.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            match c {
                c if c.is_whitespace() => i += 1,
                '-' if chars.get(i + 1) == Some(&'>') => {
                    out.push(Tok::Arrow);
                    i += 2;
                }
                '(' | ')' | '[' | ']' | '{' | '}' | ':' | '!' => {
                    out.push(match c {
                        '(' => Tok::Open,
                        ')' => Tok::Close,
                        '[' => Tok::OpenGrade,
                        ']' => Tok::CloseGrade,
                        '{' => Tok::OpenBrace,
                        '}' => Tok::CloseBrace,
                        ':' => Tok::Colon,
                        _ => Tok::Bang,
                    });
                    i += 1;
                }
                c if c.is_alphanumeric() || c == '_' => {
                    let start = i;
                    while i < chars.len()
                        && (chars[i].is_alphanumeric()
                            || chars[i] == '_'
                            || chars[i] == '\''
                            || (chars[i] == '.' && chars.get(i + 1).is_some_and(|c| c.is_alphabetic())))
                    {
                        i += 1;
                    }
                    let word: String = chars[start..i].iter().collect();
                    let last = word.rsplit('.').next().unwrap_or(&word).to_string();
                    out.push(if last == "_" {
                        Tok::Hole
                    } else if last.starts_with(|c: char| c.is_ascii_digit()) {
                        Tok::Number(last)
                    } else if last.starts_with(|c: char| c.is_uppercase()) {
                        Tok::Upper(last)
                    } else {
                        Tok::Lower(last)
                    });
                }
                // anything else is not something a type is written with;
                // skipping it is kinder than refusing the whole query
                _ => i += 1,
            }
        }
        out
    }

    struct Parser {
        tokens: Vec<Tok>,
        at: usize,
        /// each variable's number, in the order the query first mentions it
        vars: HashMap<String, u32>,
    }

    impl Parser {
        fn peek(&self) -> Option<&Tok> {
            self.tokens.get(self.at)
        }

        fn eat(&mut self, t: &Tok) -> bool {
            if self.peek() == Some(t) {
                self.at += 1;
                true
            } else {
                false
            }
        }

        fn var(&mut self, name: String) -> Ty {
            let next = self.vars.len() as u32;
            Ty::Var(*self.vars.entry(name).or_insert(next))
        }

        /// A type, arrows and all: right-associative, and flattened as it
        /// goes.
        fn arrow(&mut self) -> Ty {
            // `{a : Type} ->`: a variable, and nothing to pass
            if self.eat(&Tok::OpenBrace) {
                while !matches!(self.peek(), None | Some(Tok::CloseBrace)) {
                    self.at += 1;
                }
                self.eat(&Tok::CloseBrace);
                self.eat(&Tok::Arrow);
                return self.arrow();
            }
            let first = self.binder().unwrap_or_else(|| self.app());
            if !self.eat(&Tok::Arrow) {
                return first;
            }
            // nothing after the arrow yet is a result not typed yet
            let rest = if self.starts_type() { self.arrow() } else { Ty::Any };
            match rest {
                Ty::Fun(mut args, result) => {
                    args.insert(0, first);
                    Ty::Fun(args, result)
                }
                result => Ty::Fun(vec![first], Box::new(result)),
            }
        }

        /// `(x : [g] T)`, a named binder: its type.
        fn binder(&mut self) -> Option<Ty> {
            if !(self.peek() == Some(&Tok::Open)
                && matches!(self.tokens.get(self.at + 1), Some(Tok::Lower(_) | Tok::Hole))
                && self.tokens.get(self.at + 2) == Some(&Tok::Colon))
            {
                return None;
            }
            self.at += 3;
            self.grade();
            let ty = self.arrow();
            self.eat(&Tok::Close);
            Some(ty)
        }

        /// A grade, `[..]`, left out.
        fn grade(&mut self) {
            if self.eat(&Tok::OpenGrade) {
                while !matches!(self.peek(), None | Some(Tok::CloseGrade)) {
                    self.at += 1;
                }
                self.eat(&Tok::CloseGrade);
            }
        }

        fn starts_type(&self) -> bool {
            matches!(self.peek(), Some(Tok::OpenBrace)) || self.starts_atom()
        }

        fn starts_atom(&self) -> bool {
            matches!(self.peek(), Some(Tok::Upper(_) | Tok::Lower(_) | Tok::Number(_) | Tok::Hole | Tok::Open))
        }

        /// `Maybe a`, or anything else an atom starts; a grade after it is
        /// left out.
        fn app(&mut self) -> Ty {
            if !self.starts_atom() {
                return Ty::Any;
            }
            let head = self.atom();
            let mut args = Vec::new();
            loop {
                if self.peek() == Some(&Tok::OpenGrade) {
                    self.grade();
                } else if self.starts_atom() {
                    args.push(self.atom());
                } else {
                    break;
                }
            }
            match head {
                Ty::Con(name, mut already) if already.is_empty() => {
                    already.append(&mut args);
                    Ty::Con(name, already)
                }
                // a variable applied to something is more than this search
                // compares: it matches anything
                _ if !args.is_empty() => Ty::Any,
                head => head,
            }
        }

        fn atom(&mut self) -> Ty {
            let Some(t) = self.peek().cloned() else { return Ty::Any };
            self.at += 1;
            match t {
                Tok::Upper(name) | Tok::Number(name) => Ty::Con(name, Vec::new()),
                Tok::Lower(name) => self.var(name),
                Tok::Hole => Ty::Any,
                Tok::Open => {
                    if self.eat(&Tok::Close) {
                        return Ty::Con("()".into(), Vec::new());
                    }
                    let inner = self.arrow();
                    self.eat(&Tok::Close);
                    inner
                }
                // punctuation where a type should be: `!` starts an effect,
                // which is the end of what is compared; anything else is a
                // slip
                _ => {
                    self.at = self.tokens.len();
                    Ty::Any
                }
            }
        }
    }

    // --- matching ----------------------------------------------------------

    /// Where a declaration's variables are numbered from while it is
    /// matched, so that they and the query's are never the same variable by
    /// accident.
    pub const DECL: u32 = 1 << 24;

    /// What it costs to read `decl` as an answer to `query`: `None` if it is
    /// not one, and the cheaper the better otherwise.  See the module docs
    /// for the prices.
    pub fn cost(query: &Sig, decl: &Sig) -> Option<u32> {
        let m = query.args.len();
        let n = decl.args.len();
        // more arguments than the declaration takes, it cannot be given
        if m > n {
            return None;
        }
        let result = rename(&decl.result, DECL);
        let args: Vec<Ty> = decl.args.iter().map(|a| rename(a, DECL)).collect();

        // the result is what cannot be negotiated, and trying it first rules
        // out nearly every declaration before any argument is looked at
        let mut base = Subst::default();
        if !base.unify(&query.result, &result) {
            return None;
        }

        let mut best: Option<u32> = None;
        for pick in pairings(m, n) {
            let mut s = base.clone();
            if !query.args.iter().zip(&pick).all(|(q, &j)| s.unify(q, &args[j])) {
                continue;
            }
            let mut c = s.price(query, decl);
            // a declaration taking more than was asked still answers, a
            // little less well
            let extra = (n - m) as u32;
            c += extra * if m == 0 { 1 } else { 2 };
            if pick.windows(2).any(|w| w[0] > w[1]) {
                c += 1;
            }
            best = Some(best.map_or(c, |b| b.min(c)));
            if best == Some(0) {
                break;
            }
        }
        best.map(|c| c + anything_cost(query, decl))
    }

    /// A declaration whose result is a bare type variable produces whatever
    /// the query asks for, and so answers every query anybody types.  It is
    /// rarely what they meant.
    fn anything_cost(query: &Sig, decl: &Sig) -> u32 {
        let asked_for_something = !matches!(query.result, Ty::Var(_) | Ty::Any);
        if asked_for_something && matches!(decl.result, Ty::Var(_)) { 3 } else { 0 }
    }

    /// Every way of giving `m` query arguments to `n` declaration arguments,
    /// one each: all of them while there are few, and past that only the
    /// ones that keep the query's order.
    fn pairings(m: usize, n: usize) -> Vec<Vec<usize>> {
        fn go(m: usize, n: usize, every: bool, current: &mut Vec<usize>, used: &mut Vec<bool>, out: &mut Vec<Vec<usize>>) {
            if current.len() == m {
                out.push(current.clone());
                return;
            }
            let from = if every { 0 } else { current.last().map_or(0, |&j| j + 1) };
            for j in from..n {
                if used[j] {
                    continue;
                }
                used[j] = true;
                current.push(j);
                go(m, n, every, current, used, out);
                current.pop();
                used[j] = false;
            }
        }
        let mut out = Vec::new();
        let every = (n - m + 1..=n).product::<usize>().max(1) <= 120;
        go(m, n, every, &mut Vec::with_capacity(m), &mut vec![false; n], &mut out);
        out
    }

    /// A type, its variables moved clear of others by `by`.
    pub fn rename(t: &Ty, by: u32) -> Ty {
        match t {
            Ty::Var(v) => Ty::Var(by + v),
            Ty::Con(n, args) => Ty::Con(n.clone(), args.iter().map(|a| rename(a, by)).collect()),
            Ty::Fun(args, r) => Ty::Fun(args.iter().map(|a| rename(a, by)).collect(), Box::new(rename(r, by))),
            Ty::Any => Ty::Any,
        }
    }

    /// A unifier.  With `rigid`, the variables below [`DECL`] (the query's)
    /// stand for one particular type each, unknown: they equal themselves,
    /// and a declaration's variable may become one, but nothing else may.
    /// That is what proof search needs; a search for a declaration lets the
    /// query's variables be particular types, at a price.
    #[derive(Clone, Default)]
    pub struct Subst {
        map: HashMap<u32, Ty>,
        pub rigid: bool,
    }

    impl Subst {
        pub fn rigid() -> Subst {
            Subst { map: HashMap::new(), rigid: true }
        }

        fn walk(&self, t: &Ty) -> Ty {
            let mut t = t.clone();
            while let Ty::Var(v) = t {
                match self.map.get(&v) {
                    Some(next) => t = next.clone(),
                    None => break,
                }
            }
            t
        }

        /// `t`, with what its variables have become.
        pub fn resolve(&self, t: &Ty) -> Ty {
            match self.walk(t) {
                Ty::Con(n, args) => Ty::Con(n, args.iter().map(|a| self.resolve(a)).collect()),
                Ty::Fun(args, r) => Ty::Fun(args.iter().map(|a| self.resolve(a)).collect(), Box::new(self.resolve(&r))),
                other => other,
            }
        }

        fn occurs(&self, v: u32, t: &Ty) -> bool {
            match self.walk(t) {
                Ty::Var(w) => w == v,
                Ty::Con(_, args) => args.iter().any(|a| self.occurs(v, a)),
                Ty::Fun(args, r) => args.iter().any(|a| self.occurs(v, a)) || self.occurs(v, &r),
                Ty::Any => false,
            }
        }

        fn flexible(&self, v: u32) -> bool {
            !self.rigid || v >= DECL
        }

        pub fn unify(&mut self, a: &Ty, b: &Ty) -> bool {
            let (a, b) = (self.walk(a), self.walk(b));
            match (&a, &b) {
                (Ty::Any, _) | (_, Ty::Any) => true,
                (Ty::Var(x), Ty::Var(y)) if x == y => true,
                (Ty::Var(x), t) | (t, Ty::Var(x)) if self.flexible(*x) => {
                    if self.occurs(*x, t) {
                        return false;
                    }
                    self.map.insert(*x, t.clone());
                    true
                }
                (Ty::Var(_), _) | (_, Ty::Var(_)) => false,
                (Ty::Con(n1, a1), Ty::Con(n2, a2)) => {
                    n1 == n2 && a1.len() == a2.len() && a1.iter().zip(a2).all(|(x, y)| self.unify(x, y))
                }
                (Ty::Fun(p1, r1), Ty::Fun(p2, r2)) => {
                    p1.len() == p2.len() && p1.iter().zip(p2).all(|(x, y)| self.unify(x, y)) && self.unify(r1, r2)
                }
                _ => false,
            }
        }

        /// What the variables had to become for the match to hold.  See the
        /// module docs for why each costs what it does.
        fn price(&self, query: &Sig, decl: &Sig) -> u32 {
            let mut qvars = Vec::new();
            for t in query.args.iter().chain(std::iter::once(&query.result)) {
                vars(t, &mut qvars);
            }
            let mut dvars = Vec::new();
            for t in decl.args.iter().chain(std::iter::once(&decl.result)) {
                vars(&rename(t, DECL), &mut dvars);
            }
            qvars.sort_unstable();
            qvars.dedup();
            dvars.sort_unstable();
            dvars.dedup();
            let mut cost = 0;
            // which variable each ended up as, for the ones that stayed
            // variables
            let mut classes: HashMap<u32, (u32, u32)> = HashMap::new();
            for &v in &qvars {
                match self.resolve(&Ty::Var(v)) {
                    Ty::Var(root) => classes.entry(root).or_default().0 += 1,
                    Ty::Any => {}
                    _ => cost += 3,
                }
            }
            for &v in &dvars {
                match self.resolve(&Ty::Var(v)) {
                    Ty::Var(root) => classes.entry(root).or_default().1 += 1,
                    Ty::Any => {}
                    _ => cost += 1,
                }
            }
            for (q, d) in classes.values() {
                cost += 2 * q.saturating_sub(1);
                cost += d.saturating_sub(1);
            }
            cost
        }
    }

    fn vars(t: &Ty, out: &mut Vec<u32>) {
        match t {
            Ty::Var(v) => out.push(*v),
            Ty::Con(_, args) => args.iter().for_each(|a| vars(a, out)),
            Ty::Fun(args, r) => {
                args.iter().for_each(|a| vars(a, out));
                vars(r, out);
            }
            Ty::Any => {}
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn sig(s: &str) -> Sig {
            parse(s)
        }

        #[test]
        fn a_query_is_read_as_a_signature_is_written() {
            let s = sig("{a : Type} -> (n : [] Int) -> List a [2] -> (a -> b) -> Maybe b ! Console");
            assert_eq!(
                s.args,
                vec![
                    Ty::Con("Int".into(), vec![]),
                    Ty::Con("List".into(), vec![Ty::Var(0)]),
                    Ty::Fun(vec![Ty::Var(0)], Box::new(Ty::Var(1))),
                ]
            );
            assert_eq!(s.result, Ty::Con("Maybe".into(), vec![Ty::Var(1)]));
        }

        #[test]
        fn what_is_not_typed_yet_is_anything() {
            let s = sig("List a -> ");
            assert_eq!(s.args.len(), 1);
            assert_eq!(s.result, Ty::Any);
            assert_eq!(sig("Maybe").result, Ty::Con("Maybe".into(), vec![]));
            assert_eq!(sig("()").result, Ty::Con("()".into(), vec![]));
            assert_eq!(sig("Nat.Nat").result, Ty::Con("Nat".into(), vec![]));
        }

        #[test]
        fn the_same_type_under_other_names_costs_nothing() {
            assert_eq!(cost(&sig("List a -> Int"), &sig("List b -> Int")), Some(0));
            assert_eq!(cost(&sig("(a -> b) -> List a -> List b"), &sig("(x -> y) -> List x -> List y")), Some(0));
        }

        #[test]
        fn a_different_result_is_no_match() {
            assert_eq!(cost(&sig("List a -> Int"), &sig("List a -> String")), None);
            assert_eq!(cost(&sig("a -> b -> c"), &sig("a -> c")), None, "more arguments than it takes");
        }

        #[test]
        fn arguments_in_another_order_still_match() {
            let take = sig("Int -> List a -> List a");
            assert_eq!(cost(&sig("List a -> Int -> List a"), &take), Some(1));
            assert_eq!(cost(&sig("Int -> List a -> List a"), &take), Some(0));
        }

        #[test]
        fn more_general_is_better_than_less() {
            let general = cost(&sig("List Int -> Int"), &sig("List a -> Int")).unwrap();
            let specific = cost(&sig("List a -> Int"), &sig("List Int -> Int")).unwrap();
            assert!(general < specific, "{general} < {specific}");
        }

        #[test]
        fn a_rigid_variable_is_one_type() {
            let mut s = Subst::rigid();
            assert!(!s.unify(&Ty::Var(0), &Ty::Con("Int".into(), vec![])));
            assert!(s.unify(&Ty::Var(DECL), &Ty::Var(0)));
            assert!(!s.unify(&Ty::Var(DECL), &Ty::Con("Int".into(), vec![])));
        }
    }
}
