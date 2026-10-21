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
    /// `{auto d : t}`, or `t => ..`: an implicit argument found by searching
    /// the auto binders in scope and the hints, rather than by unification
    Auto,
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
    /// An implicit argument given by hand, `f {Int}`: the next implicit
    /// binder of `f`'s type is given this, rather than worked out.
    ImplicitApp(Box<LTerm>, Box<LTerm>),
    /// An auto argument given by hand, `f @{loud}`: the next auto binder of
    /// `f`'s type is given this, rather than searched for (its implicit
    /// arguments before it are still worked out).
    AutoApp(Box<LTerm>, Box<LTerm>),
    /// What a name already resolved to, by its unique: never written, only
    /// made by the renamer (a trait's default, where an instance leaves a
    /// method out), with the name it is shown by.
    Resolved(usize, String),
    /// A record built with its type's arguments given, as an instance's
    /// body is: `Describe {List a} { describe = .. }`, so that its fields
    /// are checked against types that are known; with the instance it is,
    /// which a trait's default for a method left out is given. Made by the
    /// parser, never written.
    RecordOf(Ident, Vec<LTerm>, Vec<(Ident, LTerm)>, Ident),
    /// What a search for an auto argument of the type wanted finds: an
    /// instance's superclass field. Made by the renamer, never written.
    Search,
    /// Equations as a term, each its patterns and its right-hand side: a
    /// function by matching, as a definition's equations are (an
    /// instance's method). Made by the parser, never written.
    Equations(Vec<(Vec<LPat>, Rhs)>),
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
    /// An array literal, `#[a; b; c]` (and `#[x;]`, `#[;]`): always an
    /// `Array`, whatever is expected.
    ArrayLit(Vec<LTerm>),
    /// `Point { x = 1, y = 2 }`: a record, by its type and its fields'
    /// values, in any order
    RecordLit(Ident, Vec<(Ident, LTerm)>),
    /// `e.x`: a field of a record
    Proj(Box<LTerm>, Ident),
    /// `{ e | x = v }`: the record `e`, with these fields replaced
    Update(Box<LTerm>, Vec<(Ident, LTerm)>),
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
    /// `Point { x = p }`: a record, its fields matched by the patterns
    /// given, and any not named by anything.
    Record(Ident, Vec<(Ident, LPat)>),
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
    /// `@native f : a`: something every back end implements itself (see
    /// `idyllic_builtins::Native`), declared with its type.  Only at the top
    /// level.
    Native(Ident, LTerm),
}

/// The name a tuple, `(a, b)`, is built with (in a term, a pattern or a
/// type): the renamer takes it to be the prelude's `Pair`, whatever else is
/// called that.
pub const TUPLE: &str = "(,)";

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
    /// the definitions `@terminating` says end, though the checker cannot
    /// see why
    pub terminating: Vec<Ident>,
    /// the definitions `@hint` offers to the search for an auto argument
    pub hints: Vec<Ident>,
    /// `@linear`: an unannotated binder here is linear, `[1]`, rather than
    /// unrestricted, `[]`
    pub linear: bool,
    pub body: Option<LTerm>,
    /// `module M where ..`: a module written inside this one's file, rather
    /// than in a file of its own; it is the module `M` under this one
    pub modules: Vec<(Ident, Program)>,
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
    /// A Unicode scalar value, `'a'`.
    Char(char),
}

/// A character as Idyll writes one: in single quotes, escaped as a string
/// would be (`'\n'`), but for `'` itself, `'\''`, and `"`, which needs no
/// escape here. Every back end shows a `Char` answer so.
pub fn show_char(c: char) -> String {
    match c {
        '\'' => "'\\''".to_string(),
        '"' => "'\"'".to_string(),
        c => {
            let s = format!("{:?}", c.to_string());
            format!("'{}'", &s[1..s.len() - 1])
        }
    }
}

impl std::fmt::Display for Lit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Lit::Unit => write!(f, "()"),
            Lit::Int(n) => write!(f, "{n}"),
            Lit::Bool(true) => write!(f, "True"),
            Lit::Bool(false) => write!(f, "False"),
            Lit::Str(s) => write!(f, "{s:?}"),
            Lit::Char(c) => f.write_str(&show_char(*c)),
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
    /// a record's fields, in order: what its one constructor's fields are
    /// called (`record Point = { x : Int, y : Int }`)
    pub fields: Option<Vec<Ident>>,
    /// a trait's methods with a default, the definition that is it
    /// (`Describe.#twice`), and whether the method is a function: what an
    /// instance that leaves one out gets
    pub defaults: Vec<(Ident, String, bool)>,
    /// whether it is a trait's record, which instances are of
    pub is_trait: bool,
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

/// `t` with the free variable `name` replaced by `by`: what a record's
/// field type becomes when an earlier field is read off the record, and the
/// field's projection is what its type mentions instead. A binder of the
/// same name hides it. Inside a `let`, a `case`, a handler or a record
/// update nothing is replaced (a field's type is rarely one of those), so
/// the name stays, and is reported if nothing else binds it.
pub fn substitute(t: &LTerm, name: &str, by: &LTerm) -> LTerm {
    let go = |t: &LTerm| Box::new(substitute(t, name, by));
    let node = match &t.node {
        Term::Var(x) if x.node == name => return by.clone(),
        Term::Pi(x, icit, grade, dom, cod) => {
            let cod = if x.as_ref().is_some_and(|x| x.node == name) { cod.clone() } else { go(cod) };
            Term::Pi(x.clone(), *icit, grade.clone(), go(dom), cod)
        }
        Term::Lam(p, body) => {
            let body = if p.node.name.node == name { body.clone() } else { go(body) };
            let mut p = p.clone();
            p.node.ty = p.node.ty.as_ref().map(|t| substitute(t, name, by));
            Term::Lam(p, body)
        }
        Term::App(f, a) => Term::App(go(f), go(a)),
        Term::Ann(e, ty) => Term::Ann(go(e), go(ty)),
        Term::BoxIntro(e) => Term::BoxIntro(go(e)),
        Term::Range(a, b) => Term::Range(go(a), go(b)),
        Term::Eff(a, b) => Term::Eff(go(a), go(b)),
        Term::Row(labels, tail) => {
            Term::Row(labels.iter().map(|l| substitute(l, name, by)).collect(), tail.as_ref().map(|t| go(t)))
        }
        Term::Seq(items) => Term::Seq(items.iter().map(|e| substitute(e, name, by)).collect()),
        Term::ArrayLit(items) => Term::ArrayLit(items.iter().map(|e| substitute(e, name, by)).collect()),
        Term::RecordLit(r, fields) => {
            Term::RecordLit(r.clone(), fields.iter().map(|(f, e)| (f.clone(), substitute(e, name, by))).collect())
        }
        Term::Proj(e, f) => Term::Proj(go(e), f.clone()),
        other => other.clone(),
    };
    Spanned::new(node, t.span)
}
