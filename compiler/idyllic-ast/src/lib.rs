//! The surface syntax tree, straight out of the parser.  Names are still
//! plain text; the renamer resolves them into `idyllic_hir`.

use idyllic_span::Spanned;

pub type LTerm = Spanned<Term>;
pub type Ident = Spanned<String>;
pub type LParam = Spanned<Param>;
pub type LDecl = Spanned<Decl>;
pub type LData = Spanned<Data>;
pub type LPat = Spanned<Pat>;
pub type LEffect = Spanned<EffectDecl>;

/// Whether an argument is written (`f x`) or left for the checker to work
/// out (`{a : Type} -> ..`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    /// `case e of { p -> a; p | g -> b; .. }`.  `if c then a else b` is a
    /// `case` on a `Bool`.
    Case(Box<LTerm>, Vec<Alt>),
    /// `b ! e`: a computation that gives a `b` and may have the effects `e`.
    /// Written after an arrow, `a -> b ! e`, or as the type of a definition.
    Eff(Box<LTerm>, Box<LTerm>),
    /// An effect row: `{Log, Div}`, or `{Log | e}` with a variable for the
    /// rest.  `{}` is no effects at all.
    Row(Vec<LTerm>, Option<Box<LTerm>>),
    /// `handle e with { return x -> a; op x k -> b }`
    Handle(Box<LTerm>, Vec<HandleClause>),
    /// A sequence literal, `#[a, b, c]`: built with the constructors of
    /// whatever sequence type is expected (see [`Data::seq`]).
    Seq(Vec<LTerm>),
}

/// One clause of a handler: `op x k -> e`, or `return x -> e`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleClause {
    pub op: Ident,
    /// `x k` for an operation, `x` for `return`; `_` for a name not used
    pub params: Vec<Ident>,
    pub body: LTerm,
}

/// `effect State (s : Type) where get : () -> s; put : s -> ()`: an
/// effect, its parameters, and its operations with their types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectDecl {
    pub name: Ident,
    pub params: Vec<(Ident, Option<LTerm>)>,
    pub ops: Vec<OpSig>,
    /// its operations', too
    pub visibility: Visibility,
}

/// An operation of an effect: `get : () -> s`, or `@many flip : () -> Bool`
/// for one a handler may resume more than once.  Unmarked, a handler
/// resumes it at most once, which is what lets it run on a stack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpSig {
    pub name: Ident,
    pub ty: LTerm,
    pub many: bool,
}

/// One alternative of a `case`: a pattern, and what to do if it matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alt {
    pub pat: LPat,
    pub rhs: Rhs,
}

/// A pattern, in an equation or a `case`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pat {
    /// A variable, possibly annotated or unboxing: `x`, `(x : [2] a)`,
    /// `[x]`.  Only a plain name nests inside other patterns.
    Bind(Param),
    /// `_`
    Wild,
    /// `3`, `"s"`, `()`, `True`
    Lit(Lit),
    /// A constructor and patterns for its (explicit) fields: `Cons x xs`,
    /// `x :: xs`, `Nil`.
    Con(Ident, Vec<LPat>),
    /// `xs@p`: `p`, and a name for the whole.
    As(Ident, Box<LPat>),
    /// `.e`: whatever matching has already worked out must be here, which is
    /// `e`.
    Dot(Box<LTerm>),
    /// `#[p, q]`: a sequence of exactly these, taken apart with the
    /// constructors of the matched type.
    Seq(Vec<LPat>),
}

/// What an equation or an alternative gives, if its patterns match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rhs {
    /// `= e`, or `-> e` in a `case`
    Plain(LTerm),
    /// `| g = e | h = f`: the first guard that holds.  If none does, the
    /// next equation is tried.
    Guarded(Vec<(LTerm, LTerm)>),
    /// `f Nil impossible`: there is nothing to say, because no value can
    /// match these patterns.
    Impossible,
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
    /// inline, `f p1 p2 : b = e`; with no patterns that is `x : a = e`.  A
    /// function may be defined by several equations in a row, and an
    /// operator by equations written infix, `Z + m = m`.
    Def(Ident, Vec<LPat>, Option<LTerm>, Rhs),
}

/// A whole source text, which is one module: declarations, then optionally
/// an expression to evaluate against them (what the REPL is given, mostly).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub datas: Vec<LData>,
    pub effects: Vec<LEffect>,
    pub decls: Vec<LDecl>,
    pub uses: Vec<Spanned<Use>>,
    /// how visible each name the definitions in `decls` define is, where an
    /// attribute says (a data type's or an effect's is on it)
    pub visibility: Vec<(Ident, Visibility)>,
    pub body: Option<LTerm>,
}

/// Who may see a declaration, from outside the module it is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub enum Visibility {
    /// its own module, and the modules inside it
    #[default]
    Private,
    /// `@pub(super)`: and its parent module's
    Super,
    /// `@pub(pkg)`: and every module of its package
    Package,
    /// `@pub`: and anyone who depends on the package
    Public,
}

/// `use App.Math`, `use App.Math as M (double)`, `use Nat.*`, `use Nat (Z)`:
/// names from another module, or a type's constructors, brought into scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Use {
    /// `@pub use`: given on to whoever uses this module
    pub public: bool,
    /// the dotted path, a segment each
    pub path: Vec<Ident>,
    /// `.*`: everything there, unqualified
    pub glob: bool,
    /// `as C`: what to qualify its names with
    pub alias: Option<Ident>,
    /// `(a, b)`: just these, unqualified
    pub names: Option<Vec<Ident>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Lit {
    Unit,
    /// 64 bits, wrapping: the runtime's primitive integer.
    Int(i64),
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
/// Or, written with the constructors' types in full,
///
/// ```text
/// data Vec : Nat -> Type -> Type where
///   Nil : Vec Z a
///   Cons : a -> Vec n a -> Vec (S n) a
/// ```
///
/// which can say what each constructor's result is: its *indices*.  Only at
/// the top level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Data {
    pub name: Ident,
    pub params: Vec<(Ident, Option<LTerm>)>,
    /// the indices after the parameters, `: Nat -> Type -> Type`; a type
    /// written without one has none
    pub kind: Option<LTerm>,
    pub ctors: Vec<(Ident, Ctor)>,
    /// `@seq(nil = Nil, cons = (::))`: the constructors a sequence literal
    /// `#[a, b]` of this type is built with, `a :: b :: Nil`
    pub seq: Option<SeqCtors>,
    pub visibility: Visibility,
}

/// The empty sequence and adding an element to the front of one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeqCtors {
    pub nil: Ident,
    pub cons: Ident,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ctor {
    /// `Cons a (List a)`: the types of the fields
    Fields(Vec<LTerm>),
    /// `Cons : a -> Vec n a -> Vec (S n) a`: the constructor's whole type
    Sig(LTerm),
}
