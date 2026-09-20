//! Bidirectional type checking / elaboration for the CoC core, with graded
//! (quantitative) usage tracking.
//!
//! The checker is the standard two-mode pair: `infer` synthesises a type from
//! a term, `check` verifies a term against a known type, and the two meet in
//! `check`'s fallback, which infers and then unifies (see [`elab`]).  Both
//! modes elaborate to [`Core`] as they go, because building a Pi value needs
//! a first-order closure over a term, not just a value.
//!
//! **Grades.**  Every binder has a grade saying how many times it may be
//! used, after Granule's graded modal types; the base is linear.  Both modes
//! also return a [`Usage`]: how many times the term uses each variable.
//! Usages add across subterms, are scaled by a function's grade at an
//! application and by a box's grade at a promotion, and are compared against
//! the binder's grade where the binder is introduced.
//!
//! **Grades in types.**  A type is a term, and using a variable in one is a
//! use like any other, as in Graded Modal Dependent Type Theory (Moon, Eades
//! & Orchard).  A binder therefore has *two* grades, `(x : (s, r) a) -> b`:
//! `s` for the function's body and `r` for `b`, checked where the function
//! type is formed.  What stops this from making every type variable linear
//! is that the two worlds stay apart: a signature or annotation is not part
//! of the computation, so what it uses is never charged to the term it
//! describes.  `{a : Type} -> a -> a` takes an `a` it uses `0` times,
//! and mentions it as a type as often as it likes (`r` defaults to `0..∞`).
//!
//! **Grade polymorphism.**  Grades are terms of type `Grade`, so they can be
//! variables, bound like any other (`{r : Grade} -> a [r] -> ..`).
//! Arithmetic on them is normalised to polynomials ([`value::Poly`]), which
//! is how `r + r` gets to be the same grade as `2 * r`, and a usage fits a
//! grade when it does so monomial by monomial.  That is sound — `+` and `*`
//! are monotone — but not complete; Granule proper asks an SMT solver.
//!
//! **Implicit arguments.**  A binder in braces, `{a : Type} -> ..`, is implicit
//! (and erased unless graded otherwise): applying a function whose type starts
//! with one inserts a metavariable for it, and checking
//! against such a type inserts the lambda.
//!
//! **Metavariables.**  Wherever the elaborator needs a type it has not been
//! given — the domain of an unannotated lambda, an explicit `_` hole — it
//! invents a fresh metavariable and lets [`unify`] work out what it must have
//! been.  Metas are created *closed*: a meta made in context Γ is immediately
//! applied to the bound variables of Γ, so a solution is a closed term that
//! can be substituted anywhere.  Any meta still unsolved at the end is
//! reported.  (The usage of a hole's eventual solution is not counted.)
//!
//! **Side tables.**  Every node's type is written to a table keyed by its
//! [`NodeId`], mirroring the renamer's span table, so a later pass — or an
//! editor — can ask for the type at a source position.
//!
//! Deliberate limitations for now:
//!
//! * The sort rule is `Type : Type`, which is convenient and logically
//!   inconsistent.  Universe levels are the obvious next step.
//!
//! * Members of a recursive group must carry a signature, and are *opaque
//!   constants* that never unfold during unification.  That is what keeps
//!   unification terminating, and it is exactly why the SCC pass runs first:
//!   only genuinely recursive definitions pay the price, while non-recursive
//!   definitions stay transparent.  A recursive definition nested under a
//!   binder is identified by its binding site, so two instantiations of it
//!   are not told apart.
//!
//! * There is one grade semiring (see [`crate::grade`]), and no approximation
//!   between function types: `a [0..1] -> b` is not accepted where
//!   `a [1] -> b` is wanted.  A grade metavariable is solved by the first
//!   equation that mentions it rather than by collecting constraints.
//!
//! * There is no syntax for giving an implicit argument explicitly.

pub mod core;
mod elab;
mod eval;
mod unify;
pub mod value;

use self::core::{Core, Lvl, MetaVar};
use self::value::{vvar, Env, Poly, VTy, Value};
use crate::ast::Icit;
use crate::builtins::{Builtin, Prim};
use crate::diagnostics::Diagnostic;
use crate::grade::Grade;
use crate::hir::{self, NodeId, SpanTable};
use crate::span::Span;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Usage
// ---------------------------------------------------------------------------

/// How many times a term uses each variable, keyed by the variable's unique.
/// Absent means zero.  This is Granule's context of graded assumptions, read
/// as an *output* of checking rather than an input.
#[derive(Clone, Debug, Default)]
pub struct Usage {
    /// symbolic, since a grade can be a variable: `f x` with
    /// `f : a [r] -> b` uses `x` `r` times
    counts: BTreeMap<usize, Poly>,
    /// Some subterm failed to elaborate, so what it would have used is not
    /// known.  Counts are then a lower bound at best, and holding binders to
    /// them would only pile derivative errors onto the real one.
    unknown: bool,
}

impl Usage {
    pub fn none() -> Usage {
        Usage::default()
    }

    /// One use of one variable.
    pub fn var(unique: usize) -> Usage {
        Usage { counts: BTreeMap::from([(unique, Poly::constant(Grade::ONE))]), unknown: false }
    }

    /// The usage of a term that failed to elaborate.
    pub fn unknown() -> Usage {
        Usage { counts: BTreeMap::new(), unknown: true }
    }

    pub fn is_unknown(&self) -> bool {
        self.unknown
    }

    /// Scalar multiplication: everything here happens `g` times.
    pub fn scale(mut self, g: &Poly) -> Usage {
        self.counts.values_mut().for_each(|v| *v = &*v * g);
        self.counts.retain(|_, v| !v.is_zero());
        self
    }

    /// The usage of a `case`, from those of two of its alternatives: only
    /// one of them runs, so each variable is used as one or the other does.
    pub fn join(mut self, other: Usage) -> Usage {
        let mut theirs = other.counts;
        // (a set: a variable both use must be joined once, not once and then
        // again with "absent")
        let keys: std::collections::BTreeSet<usize> = self.counts.keys().chain(theirs.keys()).copied().collect();
        for k in keys {
            let a = self.counts.remove(&k).unwrap_or_default();
            let b = theirs.remove(&k).unwrap_or_default();
            self.counts.insert(k, a.join(b));
        }
        self.unknown |= other.unknown;
        self
    }

    /// Remove a variable (its binder is being left), returning its count.
    pub fn take(&mut self, unique: Option<usize>) -> Poly {
        unique.and_then(|u| self.counts.remove(&u)).unwrap_or_default()
    }
}

/// Context addition: the uses of two subterms.
impl std::ops::Add for Usage {
    type Output = Usage;

    fn add(mut self, other: Usage) -> Usage {
        for (k, g) in other.counts {
            let slot = self.counts.entry(k).or_default();
            *slot = std::mem::take(slot) + g;
        }
        self.unknown |= other.unknown;
        self
    }
}

// ---------------------------------------------------------------------------
// Metavariables and holes
// ---------------------------------------------------------------------------

/// Why a metavariable was created.
#[derive(Clone, Debug)]
pub enum MetaOrigin {
    /// Stands for an explicit `_`; reported by the hole pass instead of the
    /// generic unsolved-meta pass, so it is never reported twice.
    Hole,
    /// Invented by the elaborator, with a description of what it stands for.
    Inferred(String),
}

/// Where a metavariable came from, so an unsolved one can be reported at a
/// sensible place with a sensible message.
#[derive(Clone, Debug)]
pub struct MetaInfo {
    pub node: NodeId,
    pub origin: MetaOrigin,
}

#[derive(Clone, Debug)]
pub enum MetaEntry {
    Unsolved(MetaInfo),
    Solved(Rc<Value>, MetaInfo),
}

/// A hole, remembered so it can be reported *after* checking, by which time
/// unification may have worked out what belongs in it.
#[derive(Clone, Debug)]
struct HoleInfo {
    lvl: Lvl,
    node: NodeId,
    /// the meta standing for the hole's value, applied to its spine
    value: Rc<Value>,
    ty: VTy,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A message attributed to the HIR node it was found at; the span comes from
/// the renamer's span table.
#[derive(Clone, Debug)]
pub struct TypeError {
    pub node: NodeId,
    pub kind: TypeErrorKind,
}

#[derive(Clone, Debug)]
pub enum TypeErrorKind {
    Mismatch { expected: Rc<Core>, actual: Rc<Core> },
    NotAFunction(Rc<Core>),
    BinderMismatch { expected: Rc<Core>, given: Rc<Core> },
    RecursiveBindingNeedsSignature(String),
    /// should not happen if the renamer ran; defensive
    UnknownVariable(String),
    /// a metavariable that unification never pinned down
    UnsolvedMeta(String),
    /// an explicit `_` that unification filled in
    HoleSolved { ty: Rc<Core>, solution: Rc<Core> },
    /// an explicit `_` that nothing determined
    HoleUnsolved(Rc<Core>),
    /// a variable used a different number of times than its grade allows;
    /// `in_type` when it is a function type's uses of its own binder
    UsageMismatch { name: String, used: Rc<Core>, allowed: Rc<Core>, in_type: bool },
    /// a lambda binder's written grade disagrees with the expected function
    /// type's
    BinderGradeMismatch { name: String, expected: Rc<Core>, given: Rc<Core> },
    /// a type-level grade on a lambda whose type is being inferred: only a
    /// function *type* can be held to one
    UncheckedInTypeGrade(String),
    /// a grade literal that does not fit in a machine word
    GradeTooLarge,
    /// `lo..hi` with `lo > hi`
    EmptyRange { lo: Grade, hi: Grade },
    /// `let [x] = e` where `e` is not a box
    NotABox(Rc<Core>),
    /// `case e of ..` where `e`'s type is not a data type
    NotADataType(Rc<Core>),
    /// an alternative for a constructor of some other type
    NotAConstructorOf { ctor: String, data: Rc<Core> },
    /// an alternative binding the wrong number of fields
    WrongFieldCount { ctor: String, expected: usize, given: usize },
    DuplicateAlternative(String),
    MissingAlternatives(Vec<String>),
}

/// "never", "once", "3 times", "0..1 times", "r + 1 times".
fn times(used: &Core) -> String {
    match used {
        Core::GradeLit(g) if *g == Grade::ZERO => "never".into(),
        Core::GradeLit(g) if *g == Grade::ONE => "once".into(),
        Core::GradeLit(g) if *g == Grade::ANY => "an unrestricted number of times".into(),
        _ => format!("{used} times"),
    }
}

impl TypeError {
    pub fn to_diagnostic(&self, spans: &SpanTable) -> Diagnostic {
        let here = spans.get(self.node).unwrap_or(Span::at(0));
        match &self.kind {
            TypeErrorKind::Mismatch { expected, actual } => Diagnostic::error("type mismatch")
                .label(here, format!("expected {expected}, but this has type {actual}")),
            TypeErrorKind::NotAFunction(ty) => Diagnostic::error("this is not a function")
                .label(here, format!("it has type {ty}, so it cannot be applied")),
            TypeErrorKind::BinderMismatch { expected, given } => {
                Diagnostic::error("binder annotation does not match the expected argument type")
                    .label(here, format!("expected {expected}, but the binder says {given}"))
            }
            TypeErrorKind::RecursiveBindingNeedsSignature(name) => {
                Diagnostic::error(format!("the recursive binding {name} needs a type signature"))
                    .label(here, "its type cannot be inferred from a recursive definition")
                    .help(format!("give it a signature, as in {name} : .. = .."))
            }
            TypeErrorKind::UnknownVariable(name) => {
                Diagnostic::error(format!("not in scope: {name}")).label(here, "unbound here")
            }
            TypeErrorKind::UnsolvedMeta(desc) => Diagnostic::error("unsolved metavariable")
                .label(here, format!("could not infer {desc}"))
                .help("add a type annotation to pin it down"),
            TypeErrorKind::HoleSolved { ty, solution } => {
                Diagnostic::warning("found hole").label(here, format!("_ : {ty}  =  {solution}"))
            }
            TypeErrorKind::HoleUnsolved(ty) => Diagnostic::error("unsolved hole")
                .label(here, format!("_ : {ty}, and nothing determines it")),
            TypeErrorKind::UsageMismatch { name, used, allowed, in_type: true } => {
                Diagnostic::error("usage mismatch").label(
                    here,
                    format!("{name} has grade {allowed} in this type, but the type mentions it {}", times(used)),
                )
            }
            TypeErrorKind::UsageMismatch { name, used, allowed, in_type: false } => {
                let d = Diagnostic::error("usage mismatch")
                    .label(here, format!("{name} has grade {allowed}, but is used {}", times(used)));
                let linear = matches!(&**allowed, Core::GradeLit(g) if *g == Grade::DEFAULT);
                match &**used {
                    _ if !linear => d,
                    Core::GradeLit(g) if g.is_zero() => d.help(format!(
                        "binders are linear by default; write ({name} : [0] ..) if it is meant to be unused"
                    )),
                    _ => d.help(format!(
                        "binders are linear by default; write ({name} : [{used}] ..) to allow this, or ({name} : [] ..) for any number of uses"
                    )),
                }
            }
            TypeErrorKind::BinderGradeMismatch { name, expected, given } => {
                Diagnostic::error("binder grade does not match the expected function type").label(
                    here,
                    format!("{name} is written with grade {given}, but the type says {expected}"),
                )
            }
            TypeErrorKind::UncheckedInTypeGrade(name) => {
                Diagnostic::error("a type-level grade needs a function type to check it against")
                    .label(here, format!("the type of this function is inferred, so nothing holds it to {name}'s second grade"))
                    .help("give the function a signature")
            }
            TypeErrorKind::GradeTooLarge => {
                Diagnostic::error("grade is too large").label(here, "grades are counts that fit a machine word")
            }
            TypeErrorKind::EmptyRange { lo, hi } => Diagnostic::error("empty grade range")
                .label(here, format!("{lo} is greater than {hi}, so no number of uses lies between them")),
            TypeErrorKind::NotABox(ty) => Diagnostic::error("this is not a box")
                .label(here, format!("it has type {ty}, so it cannot be unboxed")),
            TypeErrorKind::NotADataType(ty) => Diagnostic::error("this cannot be matched on")
                .label(here, format!("it has type {ty}, which is not a data type")),
            TypeErrorKind::NotAConstructorOf { ctor, data } => {
                Diagnostic::error(format!("{ctor} is not a constructor of this type"))
                    .label(here, format!("what is being matched has type {data}"))
            }
            TypeErrorKind::WrongFieldCount { ctor, expected, given } => {
                Diagnostic::error(format!("{ctor} has {expected} fields, not {given}"))
                    .label(here, "one variable for each field")
            }
            TypeErrorKind::DuplicateAlternative(ctor) => {
                Diagnostic::error(format!("{ctor} is matched twice")).label(here, "this alternative can never be taken")
            }
            TypeErrorKind::MissingAlternatives(ctors) => Diagnostic::error("this case does not cover every constructor")
                .label(here, format!("missing: {}", ctors.join(", "))),
        }
    }
}

// ---------------------------------------------------------------------------
// Contexts
// ---------------------------------------------------------------------------

/// Whether a context slot is an opaque binder or a transparent definition.
/// Only `Bound` slots go into a fresh meta's spine: a meta must depend on
/// the variables it could legitimately mention, and a definition is not one
/// of those (it already has a value).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BinderInfo {
    Bound,
    Defined,
}

/// How a name is referred to.
#[derive(Clone)]
enum Entry {
    /// A context slot: the level it was bound at, and its type.
    Local(Lvl, VTy),
    /// A recursive definition: a constant, with no slot.
    Opaque(VTy),
}

/// The typing context.
///
/// `env` holds a neutral variable for an opaque binding (a lambda/Pi binder)
/// and the binding's value for a transparent definition, which then unfolds
/// during unification.
#[derive(Clone)]
pub struct Ctx {
    lvl: Lvl,
    env: Env,
    /// outermost first, so the index is the level
    binders: Vec<(BinderInfo, String)>,
    /// by name unique
    names: HashMap<usize, Entry>,
}

impl Ctx {
    /// The context every module starts in: the primitive type constants (each
    /// a transparent definition of sort `Type`) and the primitive operations
    /// (each a transparent definition at its function type), at the uniques
    /// the renamer reserved for them.
    fn initial(tc: &Checker) -> Ctx {
        let mut ctx = Ctx { lvl: 0, env: Env::empty(), binders: Vec::new(), names: HashMap::new() };
        for b in Builtin::ALL {
            ctx = ctx.extend(Some(b.unique()), b.name(), Rc::new(Value::Type), BinderInfo::Defined, Rc::new(Value::LitTy(b)));
        }
        for p in Prim::ALL {
            // A primitive consumes each argument exactly once, so the default
            // (linear) arrow is the right one: `Int -> Int -> Int`.
            let (args, ret) = p.signature();
            let ty = args.iter().rev().fold(Rc::new(Core::LitTy(ret)), |acc, a| {
                Rc::new(Core::Pi {
                    name: "_".into(),
                    icit: Icit::Explicit,
                    grade: Core::grade(Grade::ONE),
                    in_type: Core::grade(Grade::DEFAULT_IN_TYPE),
                    dom: Rc::new(Core::LitTy(*a)),
                    cod: acc,
                })
            });
            let ty = tc.eval(&Env::empty(), &ty);
            ctx = ctx.extend(Some(p.unique()), p.name(), ty, BinderInfo::Defined, Rc::new(Value::Prim(p, Vec::new())));
        }
        ctx
    }

    fn extend(&self, unique: Option<usize>, hint: &str, ty: VTy, info: BinderInfo, value: Rc<Value>) -> Ctx {
        let mut next = self.clone();
        next.lvl = self.lvl + 1;
        next.env = self.env.push(value);
        next.binders.push((info, hint.to_string()));
        if let Some(u) = unique {
            next.names.insert(u, Entry::Local(self.lvl, ty));
        }
        next
    }

    /// Extend with an opaque binder.  `None` is the anonymous binder of a
    /// non-dependent arrow: it still occupies a slot so that indices line up,
    /// it just is not nameable.
    fn bind(&self, x: Option<&hir::Ident>, hint: &str, ty: VTy) -> Ctx {
        self.extend(x.and_then(|x| x.unique), hint, ty, BinderInfo::Bound, vvar(self.lvl, hint))
    }

    /// Extend with a transparent definition, which unfolds during unification.
    fn define(&self, x: &hir::Ident, ty: VTy, value: Rc<Value>) -> Ctx {
        self.extend(x.unique, &x.text, ty, BinderInfo::Defined, value)
    }

    /// Declare a recursive definition: nameable, but with no slot and no
    /// value.
    fn declare_opaque(&self, x: &hir::Ident, ty: VTy) -> Ctx {
        let mut next = self.clone();
        if let Some(u) = x.unique {
            next.names.insert(u, Entry::Opaque(ty));
        }
        next
    }

    /// The type a constant was declared with.
    fn constant_type(&self, unique: usize) -> Option<VTy> {
        match self.names.get(&unique)? {
            Entry::Opaque(ty) => Some(ty.clone()),
            Entry::Local(..) => None,
        }
    }

    /// Look up a name, returning the Core reference to use for it and its type.
    fn lookup(&self, x: &hir::Ident) -> Option<(Rc<Core>, VTy)> {
        let unique = x.unique?;
        Some(match self.names.get(&unique)? {
            Entry::Local(lvl, ty) => (Rc::new(Core::Var(self.lvl - lvl - 1, x.text.clone())), ty.clone()),
            Entry::Opaque(ty) => (Rc::new(Core::Opaque(unique, x.text.clone())), ty.clone()),
        })
    }

    /// The `Bound` slots of the context as Core variables, in level order.
    /// This is the spine a freshly created meta is applied to, which is what
    /// keeps metas closed.
    fn bound_var_cores(&self) -> impl Iterator<Item = Rc<Core>> + '_ {
        self.binders
            .iter()
            .enumerate()
            .filter(|(_, (info, _))| *info == BinderInfo::Bound)
            .map(|(lvl, (_, name))| Rc::new(Core::Var(self.lvl - lvl - 1, name.clone())))
    }
}

// ---------------------------------------------------------------------------
// The checker
// ---------------------------------------------------------------------------

/// All of the checker's mutable state.  The metacontext is the important
/// one: solving a meta must be visible immediately to every value that
/// already mentions it, which `&mut self` gives us for free.
pub struct Checker {
    metas: Vec<MetaEntry>,
    /// node id to that node's type, quoted in the node's context
    node_types: Vec<Option<Rc<Core>>>,
    errors: Vec<TypeError>,
    holes: Vec<HoleInfo>,
    rec_defs: Vec<RecDef>,
    /// every declared constructor, by the unique of its name
    ctors: HashMap<usize, CtorInfo>,
    /// every declared data type, by the unique of its name: its
    /// constructors, in order
    datas: HashMap<usize, Vec<usize>>,
}

/// What is known about a constructor.  At run time it is its `tag` and its
/// `fields`; the type parameters it is also applied to are erased.
#[derive(Clone, Debug)]
pub struct CtorInfo {
    pub name: String,
    /// the unique of its data type
    pub data: usize,
    /// its position among the type's constructors
    pub tag: usize,
    /// how many (implicit, erased) type parameters come before the fields
    pub params: usize,
    pub fields: usize,
}

/// An elaborated recursive definition.  In Core a recursive definition is a
/// constant ([`Core::Opaque`]) with no binding site, so that unification
/// never unfolds it; this is where its body is kept for whoever wants to
/// *run* the program.
#[derive(Clone, Debug)]
pub struct RecDef {
    /// the unique of its binder, as in `Core::Opaque`
    pub unique: usize,
    pub name: String,
    /// The size of the context it was defined in: `core` may refer to that
    /// many enclosing variables, and every use of the definition sees the
    /// same ones at the same levels.
    pub lvl: Lvl,
    pub core: Rc<Core>,
}

pub struct TypeCheckResult {
    /// The type of the module's trailing expression, if it has one.
    pub body_type: Option<Rc<Core>>,
    /// The types of the top-level definitions, in source order.
    pub decl_types: Vec<(String, Rc<Core>)>,
    /// The elaborated program: the trailing expression under the top-level
    /// (non-recursive) definitions as `let`s.  Its context is the initial
    /// one: the builtins, then the primitives.
    pub program: Option<Rc<Core>>,
    /// The recursive definitions `program` (and each other) refer to.
    pub rec_defs: Vec<RecDef>,
    /// What each metavariable was solved with, as a closed term.
    pub meta_solutions: Vec<Option<Rc<Core>>>,
    /// The declared constructors, by the unique of their names.
    pub ctors: HashMap<usize, CtorInfo>,
    /// The uniques of the declared data types.  As terms they are types, and
    /// so are erased.
    pub type_constructors: Vec<usize>,
    /// Side table: node id to type, for every node the checker visited.
    pub node_types: Vec<Option<Rc<Core>>>,
    /// The final metacontext, so callers can inspect what was solved.
    pub metas: Vec<MetaEntry>,
    pub errors: Vec<TypeError>,
}

/// Type check a renamed, SCC-ordered module.  Never fails: every problem
/// becomes a [`TypeError`] and checking continues with an error value
/// standing in for the offending subterm.  `node_count` sizes the type table
/// (it is the length of the renamer's span table).
/// The first `prelude_datas` data declarations are the prelude's, and are not
/// reported among the program's own.
pub fn check_module(m: &hir::Module, node_count: usize, prelude_datas: usize) -> TypeCheckResult {
    let mut tc = Checker {
        metas: Vec::new(),
        node_types: vec![None; node_count],
        errors: Vec::new(),
        holes: Vec::new(),
        rec_defs: Vec::new(),
        ctors: HashMap::new(),
        datas: HashMap::new(),
    };
    let ctx = Ctx::initial(&tc);
    let ctx = tc.declare_datas(&ctx, &m.datas);
    // Top-level definitions are closed, so whatever they and the entry
    // expression use is only ever a definition or a primitive: there is no
    // binder left to hold the final usage to account.
    let (ctx, _owed, lets) = tc.enter_groups(&ctx, &m.groups);
    let body = m.body.as_ref().map(|body| {
        let (core, ty, _usage) = tc.infer(&ctx, body);
        (core, tc.quote(ctx.lvl, ty))
    });
    let (program, body_type) = match body {
        Some((core, ty)) => (Some(Checker::wrap_lets(core, lets)), Some(ty)),
        None => (None, None),
    };
    tc.report_holes();
    tc.report_unsolved_metas();
    // what the program declares, in source order: data types with their
    // constructors, and definitions
    let mut declared: Vec<(NodeId, &str)> = Vec::new();
    for d in m.datas.iter().skip(prelude_datas) {
        declared.push((d.id, &d.name.text));
        declared.extend(d.ctors.iter().map(|c| (c.id, c.name.text.as_str())));
    }
    declared.extend(m.groups.iter().flat_map(hir::BindGroup::binds).map(|b| (b.id, b.name.text.as_str())));
    declared.sort_by_key(|(id, _)| *id);
    let decl_types = declared
        .into_iter()
        .filter_map(|(id, name)| Some((name.to_string(), tc.node_types.get(id.0)?.clone()?)))
        .collect();
    let meta_solutions = tc
        .metas
        .iter()
        .map(|e| match e {
            // metas are closed, so their solutions make sense in any context
            MetaEntry::Solved(v, _) => Some(tc.quote(0, v.clone())),
            MetaEntry::Unsolved(_) => None,
        })
        .collect();
    let rec_defs = std::mem::take(&mut tc.rec_defs);
    let type_constructors = tc.datas.keys().copied().collect();
    let ctors = std::mem::take(&mut tc.ctors);
    TypeCheckResult { body_type, decl_types, program, rec_defs, meta_solutions, ctors, type_constructors, node_types: tc.node_types, metas: tc.metas, errors: tc.errors }
}

impl Checker {
    fn error(&mut self, node: NodeId, kind: TypeErrorKind) {
        self.errors.push(TypeError { node, kind });
    }

    fn record(&mut self, node: NodeId, ty: Rc<Core>) {
        if let Some(slot) = self.node_types.get_mut(node.0) {
            *slot = Some(ty);
        }
    }

    fn new_meta(&mut self, node: NodeId, origin: MetaOrigin) -> MetaVar {
        self.metas.push(MetaEntry::Unsolved(MetaInfo { node, origin }));
        self.metas.len() - 1
    }

    /// Report every `_` in the source.  Run at the very end, so a hole that
    /// unification filled in reports the term it was filled with rather than
    /// just its type.
    fn report_holes(&mut self) {
        for h in std::mem::take(&mut self.holes) {
            let ty = self.quote(h.lvl, h.ty);
            let v = self.force(h.value);
            let kind = match &*v {
                Value::Flex(..) => TypeErrorKind::HoleUnsolved(ty),
                _ => TypeErrorKind::HoleSolved { ty, solution: self.quote(h.lvl, v.clone()) },
            };
            self.error(h.node, kind);
        }
    }

    /// Anything unification never pinned down is a genuine gap in the
    /// program.  Run once, at the very end, so a meta solved late still
    /// counts as solved.  Holes report themselves, so they are skipped here.
    fn report_unsolved_metas(&mut self) {
        let unsolved: Vec<(NodeId, String)> = self
            .metas
            .iter()
            .filter_map(|e| match e {
                MetaEntry::Unsolved(MetaInfo { node, origin: MetaOrigin::Inferred(desc) }) => {
                    Some((*node, desc.clone()))
                }
                _ => None,
            })
            .collect();
        for (node, desc) in unsolved {
            self.error(node, TypeErrorKind::UnsolvedMeta(desc));
        }
    }
}
