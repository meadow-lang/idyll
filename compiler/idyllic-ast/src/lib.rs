//! The surface syntax tree, straight out of the parser.  Names are still
//! plain text; the renamer resolves them into `idyllic_hir`.

use idyllic_span::Spanned;

pub type LTerm = Spanned<Term>;
pub type Ident = Spanned<String>;
pub type LParam = Spanned<Param>;
pub type LDecl = Spanned<Decl>;
pub type LData = Spanned<Data>;

/// Whether an argument is written (`f x`) or left for the checker to work
/// out (`{a : Type} -> ..`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icit {
    Explicit,
    Implicit,
}

/// The grades on a binder, `(x : [s] a)` or `(x : (s, r) a)`: `subject` is
/// how often the function uses `x`, `in_type` how often the rest of its type
/// mentions `x`.  Grades are ordinary terms (of type `Grade`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GradeAnn {
    pub subject: Box<LTerm>,
    pub in_type: Option<Box<LTerm>>,
}

/// The core calculus: a Calculus-of-Constructions style dependent lambda
/// calculus.  Application and the binders are curried (one binder / one
/// argument per node); the parser desugars the surface telescopes into these.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Term {
    /// The universe `Type`.  A larger one, `Type 1`, is this applied to a
    /// level, as it parses; the type checker reads it as a universe.
    Type,
    Var(Ident),
    Lit(Lit),
    /// Dependent function type.  `(x : a) -> b` is `Pi(Some(x), a, b)`; the
    /// non-dependent arrow `a -> b` is `Pi(None, a, b)`.  Following Haskell
    /// conventions, binders are lower-case names.  `{x : a} -> b` is the
    /// same thing with an implicit, erased argument.
    Pi(Option<Ident>, Icit, Option<GradeAnn>, Box<LTerm>, Box<LTerm>),
    /// Lambda abstraction, `\x -> e` or `\(x : a) -> e`.
    Lam(Box<LParam>, Box<LTerm>),
    /// Application, `f a`.
    App(Box<LTerm>, Box<LTerm>),
    /// Type annotation, `e : a`.
    Ann(Box<LTerm>, Box<LTerm>),
    /// An explicit hole, `_`: elaborates to a fresh metavariable.
    Hole,
    /// Square brackets, `[e]`.  As a term this is promotion.  Juxtaposed to
    /// a *type* it is Granule's postfix modality instead: `a [r]` is an `a`
    /// that may be used `r` times.  Types and terms share a grammar here, so
    /// which one `f [e]` means is settled by the checker, from `f`'s type.
    BoxIntro(Box<LTerm>),
    /// The infinite grade, `∞`.
    Inf,
    /// An interval of grades, `lo..hi`.
    Range(Box<LTerm>, Box<LTerm>),
    /// Unboxing, `let [x] = e in body`: `x` gets the box's grade.
    LetBox(Ident, Box<LTerm>, Box<LTerm>),
    Let(Vec<LDecl>, Box<LTerm>),
    /// `case e of { C x y -> a; .. }`.  `if c then a else b` is a `case` on
    /// a `Bool`.
    Case(Box<LTerm>, Vec<Alt>),
}

/// One alternative of a `case`: a constructor, a variable for each of its
/// fields, and what to do with them.  Patterns do not nest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alt {
    pub ctor: Ident,
    pub vars: Vec<Ident>,
    pub body: LTerm,
}

/// A lambda or equation pattern: a (lower-case) name with optional grades and
/// type annotation (`x`, `(x : a)`, `(x : [2] a)`), or Granule's unboxing
/// pattern `[x]`, which takes a box and binds what is inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Param {
    pub name: Ident,
    pub unbox: bool,
    pub grade: Option<GradeAnn>,
    pub ty: Option<LTerm>,
}

/// A declaration, at the top level or in a `let` block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decl {
    /// A signature, `f : a`.
    Sig(Ident, LTerm),
    /// A defining equation, `f p1 p2 = e`.  A result type may be written
    /// inline, `f p1 p2 : b = e`; with no patterns that is `x : a = e`.
    Def(Ident, Vec<LParam>, Option<LTerm>, LTerm),
}

/// A whole source text: declarations, then optionally an expression to
/// evaluate against them (what the REPL is given, mostly).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub datas: Vec<LData>,
    pub decls: Vec<LDecl>,
    pub body: Option<LTerm>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Lit {
    Unit,
    /// Arbitrary precision.
    Int(num_bigint::BigInt),
    Bool(bool),
    Str(String),
}

impl std::fmt::Display for Lit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Lit::Unit => write!(f, "()"),
            Lit::Int(n) => write!(f, "{n}"),
            Lit::Bool(true) => write!(f, "True"),
            Lit::Bool(false) => write!(f, "False"),
            Lit::Str(s) => write!(f, "{s:?}"),
        }
    }
}

/// A data type, `data List a = Nil | Cons a (List a)`: its parameters (types,
/// unless given a kind) and its constructors with the types of their fields.
/// Only at the top level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Data {
    pub name: Ident,
    pub params: Vec<(Ident, Option<LTerm>)>,
    pub ctors: Vec<(Ident, Vec<LTerm>)>,
}
