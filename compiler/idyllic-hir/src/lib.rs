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
    /// The sort `Type`.
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
    /// `case e of ..`
    Case(Box<Term>, Vec<Alt>),
}

/// One alternative of a `case`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alt {
    pub id: NodeId,
    pub head: AltHead,
    pub vars: Vec<Ident>,
    pub body: Term,
}

/// What an alternative matches.  `True` and `False` are literals rather than
/// declared constructors, so they are told apart here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AltHead {
    Con(Ident),
    Bool(bool),
}

/// `data T (a : k) .. = C t .. | ..`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataDecl {
    pub id: NodeId,
    pub name: Ident,
    /// each with its kind, where one was written (otherwise `Type`)
    pub params: Vec<(Ident, Option<Term>)>,
    pub ctors: Vec<CtorDecl>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtorDecl {
    pub id: NodeId,
    pub name: Ident,
    pub fields: Vec<Term>,
}

/// The grades written on a binder: how often the function uses it, and how
/// often the rest of the type mentions it.  Omitted ones take the defaults in
/// `idyllic_typecheck::grade::Grade`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GradeAnn {
    pub subject: Box<Term>,
    pub in_type: Option<Box<Term>>,
}

/// A single binding, `x [: a] = e`.  Function sugar `f p1 p2 = e` is desugared
/// by the renamer into `f = \p1 p2 -> e`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bind {
    pub id: NodeId,
    pub name: Ident,
    pub ty: Option<Term>,
    pub rhs: Term,
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
    pub groups: Vec<BindGroup>,
    pub body: Option<Term>,
}

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
