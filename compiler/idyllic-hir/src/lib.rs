//! The High-level IR: the surface AST after name resolution.
//!
//! Differences from [`idyllic_ast`]:
//!
//! * Every name is an [`Ident`] carrying a scope-unique integer, so binders
//!   and their uses can be compared by identity rather than spelling.
//!
//! * Every [`Term`] and [`Bind`] carries a [`NodeId`] (a fresh integer).
//!   Spans, and later inferred types, live in side tables ([`SpanTable`], the
//!   type checker's type table) keyed by that id rather than inline on the
//!   node.
//!
//! * Recursive `let` / top-level groups are represented as ordered
//!   [`BindGroup`]s.  The renamer emits one conservative recursive group;
//!   `idyllic_scc` then splits it into strongly-connected components and
//!   reorders them into dependency order.

pub use idyllic_ast::Icit;
use idyllic_ast::Lit;
use idyllic_span::Span;

/// A resolved name: a scope-unique integer plus the original spelling (kept
/// only for display).  Two `Ident`s denote the same entity iff their uniques
/// are equal.  `None` marks a use the renamer could not resolve; it has
/// already been reported, and later passes stay quiet about it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ident {
    pub unique: Option<usize>,
    pub text: String,
}

/// The identity of an HIR node.  Freshly allocated per node, densely from 0;
/// used to key the side tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub usize);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Term {
    pub id: NodeId,
    pub kind: TermKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TermKind {
    /// The universe `Type`; applied to a number, a larger one (`Type 1`).
    Type,
    Var(Ident),
    Lit(Lit),
    /// Dependent function type; `Some(x)` for `(x : a) -> b`, `None` for the
    /// plain arrow `a -> b`; implicit for `{x : a} -> b`.
    Pi(Option<Ident>, Icit, Option<GradeAnn>, Box<Term>, Box<Term>),
    /// `\x -> e`, `\(x : a) -> e` or `\(x : [2] a) -> e`: binder, grades,
    /// annotation, body.
    Lam(Ident, Option<GradeAnn>, Option<Box<Term>>, Box<Term>),
    App(Box<Term>, Box<Term>),
    /// `f {a}`: an implicit argument given by hand.
    ImplicitApp(Box<Term>, Box<Term>),
    /// `f @{d}`: an auto argument given by hand.
    AutoApp(Box<Term>, Box<Term>),
    /// What a search for an auto argument of the type wanted finds.
    Search,
    /// `e : a`.
    Ann(Box<Term>, Box<Term>),
    /// `_`: becomes a fresh metavariable during type checking.
    Hole,
    /// `[e]`: promotion, or — applied to a type — a modality's grade.
    BoxIntro(Box<Term>),
    /// The infinite grade, `∞`.
    Inf,
    /// A grade interval, `lo..hi`.
    Range(Box<Term>, Box<Term>),
    /// Unboxing, `let [x] = e in body`.
    LetBox(Ident, Box<Term>, Box<Term>),
    /// `let` with dependency-ordered groups.
    Let(Vec<BindGroup>, Box<Term>),
    /// `case e of ..`: clauses of one pattern each.
    Case(Box<Term>, Vec<Clause>),
    /// A function defined by pattern matching: the equations of `f`, with
    /// `f` left off.  Every clause has the same number of patterns.
    Match(Vec<Clause>),
    /// A term left for the checker to work out, like `_` but not reported:
    /// the type of a variable a signature mentions without binding.
    Omitted,
    /// `b ! e`: a computation type.
    Eff(Box<Term>, Box<Term>),
    /// `{Log, Div | e}`: an effect row.
    Row(Vec<Term>, Option<Box<Term>>),
    /// `handle e with clauses`
    Handle(Box<Term>, Vec<HandleClause>),
    /// A sequence literal, taken apart: `#[a, b]` is `SeqCons(a, SeqCons(b,
    /// SeqNil))`.  Which constructors these are is up to the type the literal
    /// is checked against (see [`DataDecl::seq`]).
    SeqNil,
    SeqCons(Box<Term>, Box<Term>),
    /// An array literal, `#[a; b]`: always an `Array`.
    ArrayLit(Vec<Term>),
}

/// `op x k -> body`, or with no operation, `return x -> body`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleClause {
    pub id: NodeId,
    pub op: Option<Ident>,
    pub params: Vec<Ident>,
    pub body: Term,
}

/// `effect E (a : k) .. where op : t; ..`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectDecl {
    pub id: NodeId,
    pub name: Ident,
    pub params: Vec<(Ident, Option<Term>)>,
    pub ops: Vec<OpDecl>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpDecl {
    pub id: NodeId,
    pub name: Ident,
    /// its type under the effect's parameters
    pub ty: Term,
    /// whether a handler may resume it more than once (`@many`)
    pub many: bool,
}

/// An equation, or an alternative of a `case`: patterns, and what to do if
/// they match.  The patterns bind the variables the right-hand side sees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clause {
    pub id: NodeId,
    pub pats: Vec<Pattern>,
    pub rhs: Rhs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rhs {
    Plain(Term),
    /// the first guard that holds; if none does, the next clause
    Guarded(Vec<(Term, Term)>),
    /// no value can match the patterns
    Impossible,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    pub id: NodeId,
    pub kind: PatKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatKind {
    Var(Ident),
    Wild,
    /// `3`, `"s"`, `()`, and `True` and `False`, which are literals rather
    /// than declared constructors
    Lit(Lit),
    /// a constructor, and patterns for its explicit arguments
    Con(Ident, Vec<Pattern>),
    As(Ident, Box<Pattern>),
    /// `.e`: what must be there, rather than what to look for
    Dot(Term),
    /// A sequence pattern, taken apart as a literal is (see
    /// [`TermKind::SeqNil`]): `#[p, q]` is `SeqCons([p, SeqCons([q,
    /// SeqNil])])`, the two patterns a head and a tail.
    SeqNil,
    SeqCons(Vec<Pattern>),
}

/// `data T (a : k) .. : kind where ..` or `data T (a : k) .. = C t .. | ..`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataDecl {
    pub id: NodeId,
    pub name: Ident,
    /// each with its kind, where one was written (otherwise `Type`)
    pub params: Vec<(Ident, Option<Term>)>,
    /// the kind of the indices, `Nat -> Type`; `None` for none
    pub kind: Option<Term>,
    pub ctors: Vec<CtorDecl>,
    /// `@seq(nil = Nil, cons = (::))`: the constructors a sequence literal of
    /// this type is made of
    pub seq: Option<(Ident, Ident)>,
    /// whether it was declared in a linear module, where a field that says
    /// nothing is linear (elsewhere it is `[]`, as any binder is)
    pub linear: bool,
    /// whether it is a trait's record, which instances are of
    pub is_trait: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtorDecl {
    pub id: NodeId,
    pub name: Ident,
    pub shape: CtorShape,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CtorShape {
    /// `C a (List a)`: the types of the fields, under the parameters
    Fields(Vec<Term>),
    /// `C : a -> Vec n a -> Vec (S n) a`: the whole type, under the
    /// parameters, which it does not repeat
    Sig(Term),
}

/// The grades written on a binder: how often the function uses it, and how
/// often the rest of the type mentions it.  Omitted ones take the defaults in
/// `idyllic_typecheck::grade::Grade`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GradeAnn {
    pub subject: Box<Term>,
    pub in_type: Option<Box<Term>>,
}

/// `@native f : a`: a type or an operation the back ends implement (see
/// `idyllic_builtins::Native`), with its type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeDecl {
    pub id: NodeId,
    pub name: Ident,
    pub ty: Term,
}

/// A single binding, `x [: a] = e`.  Function sugar `f p1 p2 = e` is desugared
/// by the renamer into `f = \p1 p2 -> e` when the patterns are variables,
/// and otherwise into a [`TermKind::Match`] of all of `f`'s equations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bind {
    pub id: NodeId,
    pub name: Ident,
    pub ty: Option<Term>,
    pub rhs: Term,
    /// written in a linear module (`@linear`, `--linear`), where what has no
    /// grade is used once rather than as often as wanted
    pub linear: bool,
}

/// A set of bindings that must be handled together.  Emitted by the SCC pass:
/// `NonRec` for a binding that does not refer to itself or its group, `Rec`
/// for a strongly-connected (mutually recursive) set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindGroup {
    NonRec(Bind),
    Rec(Vec<Bind>),
}

impl BindGroup {
    pub fn binds(&self) -> &[Bind] {
        match self {
            BindGroup::NonRec(b) => std::slice::from_ref(b),
            BindGroup::Rec(bs) => bs,
        }
    }

    pub fn into_binds(self) -> Vec<Bind> {
        match self {
            BindGroup::NonRec(b) => vec![b],
            BindGroup::Rec(bs) => bs,
        }
    }

    pub fn is_recursive(&self) -> bool {
        matches!(self, BindGroup::Rec(_))
    }
}

/// A renamed program: top-level bind groups (in dependency order after the
/// SCC pass), optionally followed by a trailing expression (the REPL / demo
/// entry point).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    /// in source order; a data type may mention those before it, and itself
    pub datas: Vec<DataDecl>,
    pub effects: Vec<EffectDecl>,
    /// the prelude's, then the modules' in order
    pub natives: Vec<NativeDecl>,
    pub groups: Vec<BindGroup>,
    pub body: Option<Term>,
    /// Every name the prelude declares, by its spelling.  The prelude's
    /// declarations come first in the others; the program may shadow these
    /// names, and these are still the prelude's.
    pub prelude: std::collections::HashMap<String, usize>,
    /// the root module's `main`, if it has one: what running the package
    /// runs
    pub main: Option<usize>,
    /// For a top-level name of a module other than the root, that module,
    /// as written before the name from outside: `Math` for `Math.double`.
    pub qualifiers: std::collections::HashMap<usize, String>,
    /// the package's name, and what it gives other packages
    pub package: String,
    pub names: PackageNames,
    /// the uniques the package named its own things with
    pub uniques: (usize, usize),
    /// the package's version, if it has one, for the symbols a back end
    /// names things by
    pub version: Option<String>,
    /// whether the root module, whose expression is the program's body, is
    /// linear
    pub linear_body: bool,
    /// the definitions `@terminating` vouches for: they need not say
    /// `! Div`, though the checker cannot see that they end
    pub terminating: std::collections::HashSet<usize>,
    /// the definitions `@hint` offers to the search for an auto argument
    pub hints: std::collections::HashSet<usize>,
    /// each trait's superclasses, as the projections of its record that
    /// give them, by the record type: this package's and those it depends on
    pub supers: std::collections::HashMap<usize, Vec<usize>>,
    /// each trait's methods, by the method's unique: the trait's record, the
    /// method's field in it, and how many parameters the trait has (the
    /// method's implicit arguments before the record found for it)
    pub methods: std::collections::HashMap<usize, (usize, usize, usize)>,
    /// Every name with more than one meaning in scope where it is written,
    /// by the node it was written at (a [`TermKind::Var`]'s term, or a
    /// [`PatKind::Con`]'s pattern): what it may mean, the first of which
    /// the node holds. The checker chooses by type.
    pub overloads: std::collections::HashMap<NodeId, Vec<usize>>,
}

/// What a package gives the packages that depend on it, as their renamers
/// see it.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PackageNames {
    /// each trait's methods with a default, the default's unique, and
    /// whether the method is a function, by the trait's record type
    #[serde(default)]
    pub defaults: Vec<(usize, Vec<(String, usize, bool)>)>,
    /// each trait's superclass projections, by the trait's record type
    #[serde(default)]
    pub supers: Vec<(usize, Vec<usize>)>,
    /// each trait's methods: see [`Module::methods`]
    #[serde(default)]
    pub methods: Vec<(usize, (usize, usize, usize))>,
    /// its modules' paths, the root's empty
    pub modules: Vec<Vec<String>>,
    /// each module's names: module, name, unique, and how visible it is
    pub exports: Vec<(Vec<String>, String, usize, idyllic_ast::Visibility)>,
    /// its data types, by unique, with their names and constructors
    pub types: Vec<(usize, String, Members)>,
    /// its effects' operations, by the effect's unique
    pub effect_ops: Vec<(usize, Members)>,
    /// its records' fields, in order, by the record type's unique: each
    /// one's projection
    #[serde(default)]
    pub fields: Vec<(usize, Members)>,
}

/// A type's constructors, or an effect's operations: each name, with its
/// unique.
pub type Members = Vec<(String, usize)>;

/// Side table: node id to source span.  Node ids are dense, so this is just a
/// vector indexed by them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpanTable {
    spans: Vec<Span>,
}

impl SpanTable {
    pub fn push(&mut self, span: Span) -> NodeId {
        self.spans.push(span);
        NodeId(self.spans.len() - 1)
    }

    pub fn get(&self, id: NodeId) -> Option<Span> {
        self.spans.get(id.0).copied()
    }

    /// How many nodes have been allocated; every `NodeId` is below this.
    pub fn len(&self) -> usize {
        self.spans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }
}
