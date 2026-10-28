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
//! **Universes.**  `Type n : Type (n + 1)`, with `Type` meaning `Type 0`.  A
//! function type lives in the larger of its sides' universes, so one that
//! quantifies over `Type` is not in `Type`: that, with strict positivity for
//! data types (see [`positivity`]), is what stops a term with no recursion
//! from looping, as Girard's paradox does under `Type : Type`.  Universes
//! are cumulative, but only at the top: a `Type` is accepted where a
//! `Type 1` is wanted, not a function returning one where a function
//! returning the other is.
//!
//! **Effects.**  A function type's codomain can carry a row, `a -> b ! e`:
//! a *computation type*, which is what calling the function has.  Each term's
//! effects travel with its [`Usage`], and where a lambda ends they must fit
//! the row of its type (see [`elab`]).  Rows are erased terms of type
//! `Effect`, normalised to sets like grades to polynomials.  `Div`, the
//! effect of not terminating, is what the termination checker requires of
//! a recursion it cannot prove; nothing with effects is computed in a type,
//! passed where a result type depends on it, or passed at grade `0`, so
//! everything the types see, and everything erased, is total.  The
//! program's own effects are declared with `effect` and discharged with
//! `handle` (see [`handlers`]).  What is left at the top of a program must
//! be among the prelude's `Eff`, the effects the runtime handles, and so
//! must a `main`'s, which is what running a program with no expression of
//! its own runs.
//!
//! Deliberate limitations for now:
//!
//! * Levels are numbers: there is no universe polymorphism, and a data type
//!   lives in `Type`.  A metavariable's solution is checked against the
//!   metavariable's type only as far as universes go (see [`MetaTy`]); that
//!   is the one way two sides of an equation can differ in type, since
//!   unification only ever compares like with like otherwise.
//!
//! * Members of a recursive group must carry a signature, and are *opaque
//!   constants* while the group is checked.  Afterwards a group the
//!   termination checker passes unfolds, where its case tree can choose a
//!   branch (see [`termination`]); one it does not pass stays a constant
//!   for good.  That is what keeps unification terminating, and it is why
//!   the SCC pass runs first: only genuinely recursive definitions pay the
//!   price, while non-recursive definitions stay transparent.  A recursive
//!   definition nested under a binder is identified by its binding site,
//!   so two instantiations of it are not told apart, and it never unfolds.
//!
//! * Pattern matching is dependent (see [`matching`]), except that a
//!   `case` with no expected type has a result type that does not depend
//!   on what it matched.
//!
//! * There is one grade semiring (see [`crate::grade`]), and no approximation
//!   between function types: `a [0..1] -> b` is not accepted where
//!   `a [1] -> b` is wanted.  A grade metavariable is solved by the first
//!   equation that mentions it rather than by collecting constraints.
//!
//! * There is no syntax for giving an implicit argument explicitly.

mod auto;
pub mod core;
pub mod grade;
mod elab;
mod eval;
pub mod handlers;
mod linear;
mod literals;
mod matching;
pub mod natives;
mod overloads;
pub mod packages;
pub mod prune;
mod positivity;
mod sequences;
mod termination;
mod tests;
pub mod typer;
mod unify;
pub mod value;

use self::core::{Core, Lvl, MetaVar};
use self::value::{vvar, Env, Poly, VTy, Value};
use idyllic_ast::Icit;
use idyllic_builtins::{Builtin, Prim};
use idyllic_diagnostics::Diagnostic;
use crate::grade::Grade;
use idyllic_hir::{self as hir, NodeId, SpanTable};
use idyllic_span::Span;
use std::collections::{BTreeMap, HashMap, HashSet};
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
    /// What the term does besides compute a value: the rows of the calls it
    /// makes, as they came, made into one row where it is needed (see
    /// [`elab`]).  Effects happen once, where the term is evaluated, so they
    /// are not scaled by a grade: using a result twice does not run the
    /// computation twice.
    effects: Vec<Rc<Value>>,
}

impl Usage {
    pub fn none() -> Usage {
        Usage::default()
    }

    /// One use of one variable.
    pub fn var(unique: usize) -> Usage {
        Usage { counts: BTreeMap::from([(unique, Poly::constant(Grade::ONE))]), ..Usage::default() }
    }

    /// The usage of a term that failed to elaborate.
    pub fn unknown() -> Usage {
        Usage { unknown: true, ..Usage::default() }
    }

    /// Having the effects in a row.
    pub fn effect(row: Rc<Value>) -> Usage {
        Usage { effects: vec![row], ..Usage::default() }
    }

    /// Remove the effects, where a lambda (or a definition) ends.
    pub fn take_effects(&mut self) -> Vec<Rc<Value>> {
        std::mem::take(&mut self.effects)
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
        // either alternative's effects may happen
        self.effects.extend(other.effects);
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
        self.effects.extend(other.effects);
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
    /// what the metavariable's solution must be a member of, where that is
    /// known
    pub ty: Option<MetaTy>,
}

/// The type of a metavariable, in the context it was made in.  A meta is
/// solved as a closed term abstracted over that context's bound variables,
/// so checking a solution means applying it to them again and asking about
/// the result there.
#[derive(Clone, Debug)]
pub struct MetaTy {
    /// the name and type of every slot of the context, by level
    pub types: Rc<Vec<(String, VTy)>>,
    /// the levels of the bound variables the meta was applied to
    pub spine: Vec<Lvl>,
    pub ty: VTy,
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
    /// what else to say: where each of two types of one name is from, when
    /// both are in what the error shows
    pub notes: Vec<String>,
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
    /// an implicit argument given by hand, `f {a}`, to something whose type
    /// has no implicit binder next
    NoImplicitArgument(Rc<Core>),
    /// nothing in scope, no auto binder and no hint, gives an auto argument
    /// of this type; `deep` when the search went too deep looking
    NoInstance { wanted: Rc<Core>, deep: bool },
    /// more than one hint gives it
    AmbiguousInstance { wanted: Rc<Core>, candidates: Vec<String> },
    /// what an auto argument is to be was never known well enough to look
    UnknownInstance(Rc<Core>),
    /// an auto argument given by hand, `f @{d}`, to something whose type
    /// has no auto binder next
    NoAutoArgument(Rc<Core>),
    /// an instance of a trait of another package, for a type of another
    /// package: the trait, and the type
    OrphanInstance { instance: Rc<Core>, trait_name: String },
    /// two instances whose heads unify: which one a search should take
    /// cannot be told
    OverlappingInstances { instance: Rc<Core>, other: String },
    /// no meaning of an overloaded name fits where it is used: its
    /// spelling, the type wanted there if one was, and each meaning
    NoMeaning { name: String, wanted: Option<Rc<Core>>, candidates: Vec<String> },
    /// more than one meaning of an overloaded name still fits once its
    /// definition is checked
    AmbiguousName { name: String, wanted: Option<Rc<Core>>, fitting: usize, candidates: Vec<String>, example: String },
    /// an explicit `_` that unification filled in
    HoleSolved { ty: Rc<Core>, solution: Rc<Core> },
    /// an explicit `_` that nothing determined
    HoleUnsolved(Rc<Core>),
    /// a variable used a different number of times than its grade allows;
    /// `in_type` when it is a function type's uses of its own binder
    UsageMismatch { name: String, used: Rc<Core>, allowed: Rc<Core>, in_type: bool },
    /// a variable of a linear type used more than once, whatever its
    /// grade says
    LinearUsage { name: String, ty: Rc<Core>, used: Rc<Core> },
    /// a value of a linear type boxed, to be used more than once
    LinearBox(Rc<Core>),
    /// a linear type given for a type parameter of something that may hold
    /// a value of that type more than once
    LinearInstance { ty: Rc<Core>, param: String },
    /// a top-level definition of a linear type, which every use would share
    LinearTopLevel { name: String, ty: Rc<Core> },
    /// a field a pattern bound used a different number of times than the
    /// grade of what it came out of allows
    FieldUsage { name: String, used: Rc<Core>, allowed: Rc<Core>, whole: Option<String> },
    /// a lambda binder's written grade disagrees with the expected function
    /// type's
    BinderGradeMismatch { name: String, expected: Rc<Core>, given: Rc<Core> },
    /// a type-level grade on a lambda whose type is being inferred: only a
    /// function *type* can be held to one
    UncheckedInTypeGrade(String),
    /// a grade literal that does not fit in a machine word
    GradeTooLarge,
    /// an integer literal that does not fit in the sized type it is read at
    NumberTooLarge { number: String, ty: String },
    /// `lo..hi` with `lo > hi`
    EmptyRange { lo: Grade, hi: Grade },
    /// `let [x] = e` where `e` is not a box
    NotABox(Rc<Core>),
    /// `case e of ..` where `e`'s type is not a data type
    NotADataType(Rc<Core>),
    /// a pattern naming something that is not a constructor
    NotAConstructor(String),
    /// a pattern for a constructor of some other type
    NotAConstructorOf { ctor: String, data: Rc<Core> },
    /// a constructor pattern with the wrong number of fields
    WrongFieldCount { ctor: String, expected: usize, given: usize },
    /// the cases no equation or alternative covers, as patterns
    MissingCases(Vec<String>),
    /// an equation or alternative that can never be reached
    UnreachableClause,
    /// an equation with a different number of patterns from the first
    ClauseArity { expected: usize, given: usize },
    /// more patterns than the type has arguments
    TooManyPatterns(Rc<Core>),
    /// a pattern for something matching cannot take apart: not a variable,
    /// not a constructor
    CannotSplit(Rc<Core>),
    /// matching a constructor needs two indices to be equal, and they are
    /// neither plainly equal nor plainly different
    IndexUnificationStuck { lhs: Rc<Core>, rhs: Rc<Core> },
    /// `impossible`, for patterns some value does match
    NotImpossible,
    /// a dot pattern that is not what matching worked out
    DotMismatch { expected: Rc<Core>, given: Rc<Core> },
    /// a string literal as a pattern
    StringPattern,
    /// `{p}` or `@{p}` where no implicit (or auto) argument is to be had
    MisplacedImplicitPattern,
    /// a data type whose kind does not end in `Type`
    DataKindNotType(Rc<Core>),
    /// a constructor whose type does not end in its data type, applied to
    /// its parameters
    CtorResult { ctor: String, data: String },
    /// a constructor with an implicit argument after an explicit one, or
    /// one that is not erased
    CtorImplicit(String),
    /// a data type with indices, declared without constructor types
    IndexedNeedsSignatures(String),
    /// `Type n` where `n` is not a number
    LevelNotANumber,
    /// a constructor field whose type is in a larger universe than its data
    /// type: `data U = U Type`
    LargeField { data: String, level: core::Level },
    /// a data type appearing where it is not strictly positive
    NotStrictlyPositive { data: String, ctor: String, why: Positivity },
    /// a metavariable that must be a type in `Type allowed` solved with one
    /// in `Type actual`: `id Type`, with `id : {a : Type} -> a -> a`
    UniverseTooLarge { what: String, solution: Rc<Core>, allowed: core::Level, actual: core::Level },
    /// a computation with effects its type does not allow
    EffectNotAllowed { effects: Rc<Core>, allowed: Rc<Core> },
    /// a type whose computation has effects
    ImpureType(Rc<Core>),
    /// an argument with effects, where one has to be pure: erased, or one
    /// the result type depends on
    ImpureArgument { effects: Rc<Core>, erased: bool },
    /// the type of a `let`'s body mentions a variable whose definition has
    /// effects, and so has no value the type could mean
    EffectfulLetEscapes(String),
    /// a recursive definition the termination checker cannot see ending,
    /// whose type does not say `! Div`
    MayNotTerminate(String),
    /// `b ! e` anywhere but after an arrow or as a definition's type
    EffectOutsideArrow,
    /// effects left over at the top of the program
    UnhandledEffects(Rc<Core>),
    /// an operation's type that is not `a -> b`
    OpSignature(String),
    /// `@native` on a name no back end knows
    UnknownNative(String),
    /// a native declared with a type of the wrong shape, and what it needs
    NativeSignature(String, String),
    /// a handler clause for something that is not an operation
    NotAnOperation(String),
    /// a handler with clauses for two effects' operations
    HandlerMixesEffects,
    /// a handler with no operation clause
    HandlerHandlesNothing,
    /// a handler without a clause for each of its effect's operations
    MissingOperations(Vec<String>),
    DuplicateOperation(String),
    /// a clause with the wrong number of variables
    ClauseParams { op: String, expected: usize },
    /// a handler's effect twice over in the body's row, or a row the
    /// resumption cannot be given
    AmbiguousHandler { effect: String, row: Rc<Core> },
    /// a clause resumes more than once an operation not declared `@many`
    ResumesTooOften { op: String, effect: String },
    /// a row variable of kind `Effect` given effects that may be resumed
    /// more than once
    ManyEffectsInRow { what: String, row: Rc<Core> },
    /// a sequence literal, `#[a, b]`, where a type with no `@seq` is wanted
    NotASequence(Rc<Core>),
    /// an `@seq` whose constructors cannot build sequences of its type
    BadSeq { data: String, why: String },
    /// a `main` that is not `() -> () ! Eff`, or has effects `Eff` does not
    /// have
    MainType(Rc<Core>),
    /// a `@test` that is not `() -> ()` with the runtime's effects
    TestType { name: String, ty: Rc<Core> },
    /// tests to run, and no `Std.Test` to run them with
    NoTestRunner,
}

/// Where a data type appears that makes it fail strict positivity.
#[derive(Clone, Debug)]
pub enum Positivity {
    /// in the domain of a function type: `Bad (Bad -> Int)`
    LeftOfArrow,
    /// in an argument of itself: `Bad (Bad (Bad a))`
    OwnArgument,
    /// in an argument of another data type, for a parameter that type does
    /// not use strictly positively
    Nested(String),
    /// somewhere nothing is known about, such as under a type variable
    Unknown,
}

/// A row as a phrase: `Div`, `{Log, Div}`, or "none".
fn row_or_none(row: &Core) -> String {
    match row {
        Core::Row(labels, tails) if labels.is_empty() && tails.is_empty() => "none".into(),
        _ => row.to_string(),
    }
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

/// What is shown of a metavariable is its number in what is shown: `?0`,
/// `?1`, ... in the order they first appear in `text`, `seen` holding those
/// numbered so far. (The checker's own numbers count every metavariable the
/// prelude made too, which says nothing to the reader.)
pub fn number_metas(seen: &mut HashMap<String, usize>, text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        out.push(c);
        if c != '?' {
            continue;
        }
        let mut end = i + 1;
        while let Some(&(j, d)) = chars.peek() {
            if !d.is_ascii_digit() {
                break;
            }
            end = j + 1;
            chars.next();
        }
        if end > i + 1 {
            let next = seen.len();
            let n = *seen.entry(text[i + 1..end].to_string()).or_insert(next);
            out.push_str(&n.to_string());
        }
    }
    out
}

impl TypeError {
    pub fn to_diagnostic(&self, spans: &SpanTable) -> Diagnostic {
        let mut d = self.notes.iter().fold(self.diagnostic(spans), |d, note| d.help(note.clone()));
        let mut seen = HashMap::new();
        d.message = number_metas(&mut seen, &d.message);
        for l in &mut d.labels {
            l.message = number_metas(&mut seen, &l.message);
        }
        for h in &mut d.help {
            *h = number_metas(&mut seen, h);
        }
        d
    }

    fn diagnostic(&self, spans: &SpanTable) -> Diagnostic {
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
                // (the renamer found it, so it is declared, but not yet where
                // this is checked)
                Diagnostic::error(format!("not in scope: {name}")).label(here, "unbound here").help(format!(
                    "{name} is declared, but after this needs it: a data type's own constructor in its fields is one, and a `use` of its constructors hides any type of the same name, so bring in only those it does not clash with, as in `use T (A, B)`"
                ))
            }
            TypeErrorKind::NoMeaning { name, wanted, candidates } => {
                let needed = match wanted {
                    Some(w) => format!("no `{name}` in scope has the type needed here, `{w}`"),
                    None => format!("no `{name}` in scope fits here"),
                };
                Diagnostic::error(needed)
                    .label(here, "used here")
                    .help(format!("candidates in scope:\n  {}", candidates.join("\n  ")))
            }
            TypeErrorKind::AmbiguousName { name, wanted, fitting, candidates, example } => {
                let fit = match wanted {
                    Some(w) => format!("fit `{w}`"),
                    None => "fit here".to_string(),
                };
                Diagnostic::error(format!("ambiguous `{name}`: {fitting} of the candidates in scope {fit}"))
                    .label(here, "which one is meant?")
                    .help(format!(
                        "candidates in scope:\n  {}\nsay which with a type annotation, or write it in full, as `{example}`",
                        candidates.join("\n  ")
                    ))
            }
            TypeErrorKind::NoInstance { wanted, deep } => {
                let wanted = wanted.to_string();
                let article = if wanted.starts_with(['A', 'E', 'I', 'O', 'U']) { "an" } else { "a" };
                let d = Diagnostic::error(format!("nothing in scope gives {article} {wanted}")).label(here, "an auto argument is wanted here");
                if *deep {
                    d.help("the search went round too many hints that want auto arguments of their own")
                } else {
                    d.help("an auto binder, {auto d : ..} or .. =>, or a definition marked @hint, of that type would")
                }
            }
            TypeErrorKind::AmbiguousInstance { wanted, candidates } => {
                Diagnostic::error(format!("more than one hint gives a {wanted}"))
                    .label(here, "an auto argument is wanted here")
                    .help(format!("the hints that do:\n  {}", candidates.join("\n  ")))
            }
            TypeErrorKind::OrphanInstance { instance, trait_name } => {
                Diagnostic::error(format!("an instance {instance} cannot be here"))
                    .label(here, format!("neither {trait_name} nor the type it is for is this package's"))
                    .help("an instance lives with its trait or with its type, so that no two packages give the same one; a named instance, `instance name : ..`, may live anywhere, and is given by hand, `f @{name}`")
            }
            TypeErrorKind::OverlappingInstances { instance, other } => {
                Diagnostic::error(format!("this instance {instance} overlaps another"))
                    .label(here, format!("{other} is for the same types"))
                    .help("two instances of a trait may not be for types that unify: a search could not tell which to take; name one, `instance name : ..`, to give it by hand")
            }
            TypeErrorKind::NoAutoArgument(ty) => Diagnostic::error("no auto argument is taken here")
                .label(here, format!("given by hand, but what it is given to has type {ty}"))
                .help("`f @{d}` gives `f` the auto argument its type binds next, as `{auto d : ..} ->` or `.. =>`"),
            TypeErrorKind::UnknownInstance(wanted) => Diagnostic::error("cannot tell what to look for")
                .label(here, format!("an auto argument of type {wanted} is wanted here, and what it is is never worked out"))
                .help("say more of the type, with an annotation"),
            TypeErrorKind::NoImplicitArgument(ty) => Diagnostic::error("no implicit argument is taken here")
                .label(here, format!("given by hand, but what it is given to has type {ty}"))
                .help("`f {a}` gives `f` the implicit argument its type binds next, as `{a : Type} -> ..`"),
            TypeErrorKind::UnsolvedMeta(desc) => Diagnostic::error("unsolved metavariable")
                .label(here, format!("could not infer {desc}"))
                .help("add a type annotation to pin it down"),
            TypeErrorKind::HoleSolved { ty, solution } => {
                Diagnostic::warning("found hole").label(here, format!("_ : {ty}  =  {solution}"))
            }
            TypeErrorKind::HoleUnsolved(ty) => Diagnostic::error("unsolved hole")
                .label(here, format!("_ : {ty}, and nothing determines it")),
            TypeErrorKind::FieldUsage { name, used, allowed, whole } => {
                let (whole, example) = match whole {
                    Some(w) => (w.clone(), w.clone()),
                    None => ("the argument it came out of".to_string(), "x".to_string()),
                };
                Diagnostic::error("usage mismatch")
                    .label(here, format!("{name} has grade {allowed}, but is used {}", times(used)))
                    .help(format!(
                        "a field has the grade of what it came out of; to use {name} {}, give {whole} a grade that allows it, as in ({example} : [0..1] ..)",
                        times(used)
                    ))
            }
            TypeErrorKind::LinearUsage { name, ty, used } => Diagnostic::error("a linear value used more than once")
                .label(here, format!("{name} is of type {ty}, which is linear, but is used {}", times(used)))
                .help(format!("a value of {ty} is changed in place, so it is never shared: each use must give it back for the next, as a write does")),
            TypeErrorKind::LinearBox(ty) => Diagnostic::error("a linear value boxed")
                .label(here, format!("this is of type {ty}, which is linear, so it may not be boxed to be used more than once")),
            TypeErrorKind::LinearInstance { ty, param } => Diagnostic::error("a linear type for a type parameter that is shared")
                .label(here, format!("{ty} is linear, but this may hold a value of type {param} more than once"))
                .help(format!("{ty} is changed in place, so it is never shared: it may be given only for a parameter whose values are used at most once")),
            TypeErrorKind::LinearTopLevel { name, ty } => Diagnostic::error("a linear value at the top level")
                .label(here, format!("{name} is of type {ty}, which is linear, and every use of a top-level definition shares it"))
                .help("make it a function, so that each call makes one of its own"),
            TypeErrorKind::UsageMismatch { name, used, allowed, in_type: true } => {
                Diagnostic::error("usage mismatch").label(
                    here,
                    format!("{name} has grade {allowed} in this type, but the type mentions it {}", times(used)),
                )
            }
            TypeErrorKind::UsageMismatch { name, used, allowed, in_type: false } => {
                let d = Diagnostic::error("usage mismatch")
                    .label(here, format!("{name} has grade {allowed}, but is used {}", times(used)));
                let linear = matches!(&**allowed, Core::GradeLit(g) if *g == Grade::ONE);
                match &**used {
                    _ if !linear => d,
                    Core::GradeLit(g) if g.is_zero() => d.help(format!(
                        "{name} is linear; write ({name} : [0] ..) if it is meant to be unused"
                    )),
                    _ => d.help(format!(
                        "{name} is linear, being written [1] or in a module that is @linear; write ({name} : [{used}] ..) to allow this, or ({name} : [] ..) for any number of uses"
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
            TypeErrorKind::NumberTooLarge { number, ty } => Diagnostic::error(format!("{number} does not fit in an {ty}"))
                .label(here, format!("an {ty} is from {}", number_range(ty))),
            TypeErrorKind::GradeTooLarge => {
                Diagnostic::error("grade is too large").label(here, "grades are counts that fit a machine word")
            }
            TypeErrorKind::EmptyRange { lo, hi } => Diagnostic::error("empty grade range")
                .label(here, format!("{lo} is greater than {hi}, so no number of uses lies between them")),
            TypeErrorKind::NotABox(ty) => Diagnostic::error("this is not a box")
                .label(here, format!("it has type {ty}, so it cannot be unboxed")),
            TypeErrorKind::NotADataType(ty) => Diagnostic::error("this cannot be matched on")
                .label(here, format!("it has type {ty}, which is not a data type")),
            TypeErrorKind::NotAConstructor(name) => Diagnostic::error(format!("{name} is not a constructor"))
                .label(here, "only a constructor can be matched on"),
            TypeErrorKind::NotAConstructorOf { ctor, data } => {
                Diagnostic::error(format!("{ctor} is not a constructor of this type"))
                    .label(here, format!("what is being matched has type {data}"))
            }
            TypeErrorKind::WrongFieldCount { ctor, expected, given } => {
                Diagnostic::error(format!("{ctor} has {expected} fields, not {given}"))
                    .label(here, "one pattern for each field")
            }
            TypeErrorKind::MissingCases(cases) => Diagnostic::error("the patterns do not cover every case")
                .label(here, format!("missing: {}", cases.join(", "))),
            TypeErrorKind::UnreachableClause => {
                Diagnostic::warning("unreachable").label(here, "the cases before this one cover everything it matches")
            }
            TypeErrorKind::ClauseArity { expected, given } => {
                Diagnostic::error("equations with different numbers of arguments")
                    .label(here, format!("this one has {given}, but the first has {expected}"))
            }
            TypeErrorKind::TooManyPatterns(ty) => Diagnostic::error("too many patterns")
                .label(here, format!("what is left to match has type {ty}, which is not a function")),
            TypeErrorKind::CannotSplit(v) => Diagnostic::error("this cannot be matched on")
                .label(here, format!("it is {v}, which is neither a variable nor a constructor")),
            TypeErrorKind::IndexUnificationStuck { lhs, rhs } => {
                Diagnostic::error("cannot tell whether this pattern applies")
                    .label(here, format!("it needs {lhs} and {rhs} to be equal, and they are neither the same nor different"))
            }
            TypeErrorKind::NotImpossible => Diagnostic::error("not impossible")
                .label(here, "some values match these patterns")
                .help("give this equation a right-hand side"),
            TypeErrorKind::DotMismatch { expected, given } => Diagnostic::error("dot pattern does not match")
                .label(here, format!("matching has worked out that this is {expected}, not {given}")),
            TypeErrorKind::MisplacedImplicitPattern => Diagnostic::error("no implicit argument is here to match")
                .label(here, "`{p}` matches an implicit argument, and `@{p}` an auto one, where the type has one next")
                .help("an implicit pattern goes where its binder is, in the order the type gives them"),
            TypeErrorKind::StringPattern => Diagnostic::error("strings cannot be matched on yet")
                .label(here, "use a guard with a comparison instead"),
            TypeErrorKind::DataKindNotType(kind) => Diagnostic::error("a data type's kind ends in Type")
                .label(here, format!("this is {kind}")),
            TypeErrorKind::CtorResult { ctor, data } => Diagnostic::error(format!("{ctor} does not build a {data}"))
                .label(here, format!("a constructor's type ends in {data}, applied to its parameters and then its indices")),
            TypeErrorKind::CtorImplicit(ctor) => Diagnostic::error(format!("unsupported implicit argument of {ctor}"))
                .label(here, "a constructor's implicit arguments come first, and are erased"),
            TypeErrorKind::IndexedNeedsSignatures(data) => {
                Diagnostic::error(format!("{data} has indices, so its constructors need types"))
                    .label(here, "declare them with where, as in  C : .. -> T i")
            }
            TypeErrorKind::LevelNotANumber => Diagnostic::error("a universe level is a number")
                .label(here, "level variables are not supported yet"),
            TypeErrorKind::LargeField { data, level } => Diagnostic::error("field too large for its data type")
                .label(here, format!("this field's type lives in Type {level}, but {data} lives in Type"))
                .help("a data type lives in Type, so its fields must be types in Type, unless its kind says it lives higher, as a record's or a trait's with a field that is a type does"),
            TypeErrorKind::UniverseTooLarge { what, solution, allowed, actual } => {
                Diagnostic::error("universe too large").label(
                    here,
                    format!("{what} must be a type in {}, but here it would be {solution}, which is in {}", Core::Type(*allowed), Core::Type(*actual)),
                )
            }
            TypeErrorKind::EffectNotAllowed { effects, allowed } => {
                let d = Diagnostic::error("effects not allowed")
                    .label(here, format!("this has the effects {effects}, but its type allows {}", row_or_none(allowed)));
                if let Core::Row(labels, tails) = &**allowed {
                    if labels.is_empty() && tails.is_empty() {
                        return d.help(format!("a type with no row means none; say so with ! {effects}"));
                    }
                }
                d
            }
            TypeErrorKind::ImpureType(effects) => Diagnostic::error("a type must be pure")
                .label(here, format!("computing this type has the effects {effects}"))
                .help("only total code runs in types; a function that may not terminate has the effect Div"),
            TypeErrorKind::ImpureArgument { effects, erased: true } => {
                Diagnostic::error("an erased argument must be pure").label(
                    here,
                    format!("this has the effects {effects}, but it is never run, so they could not happen"),
                )
            }
            TypeErrorKind::ImpureArgument { effects, erased: false } => {
                Diagnostic::error("a type cannot depend on this").label(
                    here,
                    format!("the result type mentions this argument, which has the effects {effects}"),
                ).help("bind it with let first, so that the types see a variable")
            }
            TypeErrorKind::EffectfulLetEscapes(name) => Diagnostic::error("a type cannot depend on this")
                .label(here, format!("the type of this depends on {name}, whose definition has effects"))
                .help("give the whole a type annotation that does not mention it"),
            TypeErrorKind::MayNotTerminate(name) => {
                Diagnostic::error(format!("{name} may not terminate")).label(
                    here,
                    "no argument gets smaller on every round of its recursion, so it has the effect Div",
                ).help(format!("say so in its type, as in {name} : .. -> .. ! Div"))
            }
            TypeErrorKind::EffectOutsideArrow => Diagnostic::error("effects belong to a function type")
                .label(here, "write this after an arrow, as in a -> b ! e"),
            TypeErrorKind::OpSignature(op) => Diagnostic::error(format!("{op} is not an operation's type"))
                .label(here, "an operation takes arguments and gives a result, as in a -> b; use () for none"),
            TypeErrorKind::UnknownNative(name) => Diagnostic::error(format!("there is no native called {name}"))
                .label(here, "@native declares something every back end implements, and this is not one of them"),
            TypeErrorKind::NativeSignature(name, why) => {
                Diagnostic::error(format!("the type of the native {name} does not fit it")).label(here, why.clone())
            }
            TypeErrorKind::NotAnOperation(name) => Diagnostic::error(format!("{name} is not an operation"))
                .label(here, "a handler's clauses are for the operations of an effect, and return"),
            TypeErrorKind::HandlerMixesEffects => Diagnostic::error("one handler, one effect")
                .label(here, "this operation belongs to another effect than the clauses before it"),
            TypeErrorKind::HandlerHandlesNothing => Diagnostic::error("this handler handles nothing")
                .label(here, "give it a clause for each operation of the effect it is for"),
            TypeErrorKind::MissingOperations(ops) => Diagnostic::error("the handler does not handle every operation")
                .label(here, format!("missing: {}", ops.join(", "))),
            TypeErrorKind::DuplicateOperation(op) => {
                Diagnostic::error(format!("{op} is handled twice")).label(here, "this clause can never be taken")
            }
            TypeErrorKind::ClauseParams { op, expected: 1 } => Diagnostic::error("a return clause names one thing")
                .label(here, format!("{op} x -> .., for the value it is given")),
            TypeErrorKind::ClauseParams { op, expected } => {
                let args: Vec<&str> = ["x", "y", "z", "w"].into_iter().cycle().take(expected - 1).collect();
                let what = if args.len() == 1 { "argument" } else { "arguments" };
                Diagnostic::error(format!("{op}'s clause names {expected} things"))
                    .label(here, format!("{op} {} k -> .., for its {what} and the resumption", args.join(" ")))
            }
            TypeErrorKind::AmbiguousHandler { effect, row } => Diagnostic::error(format!("which {effect}?"))
                .label(here, format!("the handled computation has the effects {row}")),
            TypeErrorKind::ManyEffectsInRow { what, row } => Diagnostic::error("these effects may be resumed more than once")
                .label(here, format!("{what} stands for effects resumed at most once, and {row} has an operation declared @many"))
                .help("a row variable takes effects resumed at most once unless it says otherwise: bind it as {e : Effect []}"),
            TypeErrorKind::ResumesTooOften { op, effect } => Diagnostic::error(format!("{op} is resumed more than once"))
                .label(here, format!("this clause may resume {op} more than once, and {effect} declares it to be resumed at most once"))
                .help(format!("to let a handler resume it more than once, declare it `@many {op} : ..` in `effect {effect}`")),
            TypeErrorKind::UnhandledEffects(effects) => Diagnostic::error("unhandled effects")
                .label(here, format!("the program has the effects {effects}, and nothing handles them"))
                .help("at the top of a program, only the effects in Eff are handled, by the runtime"),
            TypeErrorKind::NotASequence(ty) => Diagnostic::error("a sequence literal needs a sequence type")
                .label(here, format!("the type wanted here is {ty}, which #[..] cannot build"))
                .help("a type #[..] can build is declared with @seq(nil = .., cons = ..), as List is"),
            TypeErrorKind::BadSeq { data, why } => {
                Diagnostic::error(format!("{data} cannot be built with #[..]")).label(here, why.clone())
            }
            TypeErrorKind::TestType { name, ty } => Diagnostic::error("a test has the wrong type")
                .label(here, format!("{name} is {ty}"))
                .help("a test takes () and gives (), with at most the effects the runtime handles, as main does: t : () -> () ! Eff"),
            TypeErrorKind::NoTestRunner => Diagnostic::error("nothing to run the tests with")
                .help("idyll test runs them with Std.Test's testsStart, testCase and testsDone"),
            TypeErrorKind::MainType(ty) => Diagnostic::error("main has the wrong type")
                .label(here, format!("main is {ty}"))
                .help("main takes () and gives (), with at most the effects the runtime handles: main : () -> () ! Eff"),
            TypeErrorKind::NotStrictlyPositive { data, ctor, why } => {
                let why = match why {
                    Positivity::LeftOfArrow => "it appears to the left of an arrow".to_string(),
                    Positivity::OwnArgument => "it appears in an argument of itself".to_string(),
                    Positivity::Nested(outer) => {
                        format!("it appears inside {outer}, whose parameter is not strictly positive,")
                    }
                    Positivity::Unknown => "it appears under a type it is not known to be positive in,".to_string(),
                };
                Diagnostic::error(format!("{data} is not strictly positive"))
                    .label(here, format!("{why} in this field of {ctor}"))
                    .help(
                        "a data type may appear in its own constructors' fields only to the right of every \
                         arrow; otherwise it can express a term that loops without recursion",
                    )
            }
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
    /// Defined, but by a computation with effects: at run time it has a
    /// value like any definition, but the types may not see it, since two
    /// runs of the same computation need not agree.  To the types it is as
    /// good as bound.
    Opaque,
}

impl BinderInfo {
    /// Whether the types know nothing of the slot's value.
    fn unknown(self) -> bool {
        matches!(self, BinderInfo::Bound | BinderInfo::Opaque)
    }
}

/// One slot of the context.
#[derive(Clone)]
struct Binder {
    info: BinderInfo,
    /// for display
    name: String,
    ty: VTy,
    /// The unique a use of this slot is charged to.  Usually that of the
    /// name that bound it; a pattern variable standing for a slot the match
    /// compiler made is charged to that slot's own key instead, so the grade
    /// is held to all its uses together, whichever name they went by.
    key: Option<usize>,
    /// How often it may be used, where that is known: matching takes a
    /// value apart into fields of the same grade.
    grade: Option<Rc<Value>>,
    /// Whether it is an auto binder, which a search for an auto argument
    /// may find (see [`auto`]).
    auto: bool,
}

/// How a name is referred to.
#[derive(Clone)]
enum Entry {
    /// A context slot, by level.  Several names can share one: see
    /// [`Binder::key`].
    Local(Lvl),
    /// A recursive definition: a constant, with no slot.
    Opaque(VTy),
}

/// The typing context.
///
/// `env` holds a neutral variable for an opaque binding (a lambda/Pi binder)
/// and the binding's value for a transparent definition, which then unfolds
/// during unification.  Matching can turn the one into the other: once a
/// variable is known to be `S n`, it is defined as that (see [`matching`]).
#[derive(Clone)]
pub struct Ctx {
    lvl: Lvl,
    env: Env,
    /// outermost first, so the index is the level
    binders: Vec<Binder>,
    /// by name unique
    names: HashMap<usize, Entry>,
    /// The slots matching has split, with the constructor each turned out to
    /// be and the slots of that constructor's arguments.
    splits: HashMap<Lvl, (usize, Vec<Lvl>)>,
}

impl Ctx {
    /// The context as it was when it had `lvl` variables: the outermost
    /// `lvl` of these.
    pub(crate) fn prefix(&self, lvl: Lvl) -> Ctx {
        if lvl >= self.lvl {
            return self.clone();
        }
        let mut binders = self.binders.clone();
        binders.truncate(lvl);
        let splits = self.splits.iter().filter(|(l, _)| **l < lvl).map(|(l, s)| (*l, s.clone())).collect();
        Ctx { lvl, env: self.env.outer(self.lvl - lvl), binders, names: self.names.clone(), splits }
    }

    /// The context every module starts in: the primitive type constants (each
    /// a transparent definition of sort `Type`) and the primitive operations
    /// (each a transparent definition at its function type), at the uniques
    /// the renamer reserved for them.
    fn initial(tc: &Checker) -> Ctx {
        let mut ctx =
            Ctx { lvl: 0, env: Env::empty(), binders: Vec::new(), names: HashMap::new(), splits: HashMap::new() };
        for b in Builtin::ALL {
            // `Div` is an effect; the rest are types
            let ty = match b {
                Builtin::Div => Value::LitTy(Builtin::Effect),
                _ => Value::Type(0),
            };
            ctx = ctx.extend(Some(b.unique()), b.name(), Rc::new(ty), BinderInfo::Defined, Rc::new(Value::LitTy(b)));
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
        next.binders.push(Binder { info, name: hint.to_string(), ty, key: unique, grade: None, auto: false });
        if let Some(u) = unique {
            next.names.insert(u, Entry::Local(self.lvl));
        }
        next
    }

    /// Extend with an opaque binder.  `None` is the anonymous binder of a
    /// non-dependent arrow: it still occupies a slot so that indices line up,
    /// it just is not nameable.
    fn bind(&self, x: Option<&hir::Ident>, hint: &str, ty: VTy) -> Ctx {
        self.extend(x.and_then(|x| x.unique), hint, ty, BinderInfo::Bound, vvar(self.lvl, hint))
    }

    /// Extend with an opaque binder that no name refers to yet, charged to
    /// `key`.
    fn bind_keyed(&self, key: usize, hint: &str, ty: VTy, grade: Option<Rc<Value>>) -> Ctx {
        let mut next = self.extend(None, hint, ty, BinderInfo::Bound, vvar(self.lvl, hint));
        next.binders[self.lvl].key = Some(key);
        next.binders[self.lvl].grade = grade;
        next
    }

    /// Record the grade of the innermost slot.
    fn graded(mut self, grade: &Rc<Value>) -> Ctx {
        if let Some(b) = self.binders.last_mut() {
            b.grade = Some(grade.clone());
        }
        self
    }

    /// The last binder, as an auto binder: what a search may find.
    fn auto_bound(mut self) -> Ctx {
        if let Some(b) = self.binders.last_mut() {
            b.auto = true;
        }
        self
    }

    /// Extend with a transparent definition, which unfolds during unification.
    fn define(&self, x: &hir::Ident, ty: VTy, value: Rc<Value>) -> Ctx {
        self.extend(x.unique, &x.text, ty, BinderInfo::Defined, value)
    }

    /// Extend with a definition whose value the types may not know.
    fn define_opaque(&self, x: &hir::Ident, ty: VTy) -> Ctx {
        self.extend(x.unique, &x.text, ty, BinderInfo::Opaque, vvar(self.lvl, &x.text))
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

    /// Let `x` name the slot at `lvl`.
    fn alias(&mut self, x: &hir::Ident, lvl: Lvl) {
        if let Some(u) = x.unique {
            self.names.insert(u, Entry::Local(lvl));
        }
    }

    /// Look up a name, returning the Core reference to use for it, its type,
    /// and the unique its uses are charged to.
    fn lookup(&self, x: &hir::Ident) -> Option<(Rc<Core>, VTy, usize)> {
        let unique = x.unique?;
        Some(match self.names.get(&unique)? {
            Entry::Local(lvl) => {
                let b = &self.binders[*lvl];
                (Rc::new(Core::Var(self.lvl - lvl - 1, x.text.clone())), b.ty.clone(), b.key.unwrap_or(unique))
            }
            Entry::Opaque(ty) => (Rc::new(Core::Opaque(unique, x.text.clone())), ty.clone(), unique),
        })
    }

    /// The `Bound` slots of the context, by level.  A freshly created meta
    /// is applied to these, which is what keeps metas closed.
    fn bound_levels(&self) -> impl Iterator<Item = Lvl> + '_ {
        self.binders.iter().enumerate().filter(|(_, b)| b.info.unknown()).map(|(lvl, _)| lvl)
    }

    /// The `Bound` slots of the context as Core variables, in level order.
    fn bound_var_cores(&self) -> impl Iterator<Item = Rc<Core>> + '_ {
        self.bound_levels().map(|lvl| Rc::new(Core::Var(self.lvl - lvl - 1, self.binders[lvl].name.clone())))
    }

    /// The value in a slot.
    fn value(&self, lvl: Lvl) -> Rc<Value> {
        self.env.get(self.lvl - lvl - 1).expect("a slot of this context")
    }

    /// The slot a name refers to, if it is a variable nothing has defined.
    fn bound_slot(&self, x: &hir::Ident) -> Option<Lvl> {
        match self.names.get(&x.unique?)? {
            Entry::Local(lvl) if self.binders[*lvl].info.unknown() => Some(*lvl),
            _ => None,
        }
    }

    /// The name and type of every slot, by level.
    fn types(&self) -> Vec<(String, VTy)> {
        self.binders.iter().map(|b| (b.name.clone(), b.ty.clone())).collect()
    }
}

// ---------------------------------------------------------------------------
// The checker
// ---------------------------------------------------------------------------

/// All of the checker's mutable state.  The metacontext is the important
/// one: solving a meta must be visible immediately to every value that
/// already mentions it, which `&mut self` gives us for free.  It is cloned
/// to try what an overloaded name means (see [`overloads`]).
#[derive(Clone)]
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
    /// for each data type, which of its parameters it uses strictly
    /// positively (see [`positivity`]); and for each native type, all
    positive_params: HashMap<usize, Vec<bool>>,
    /// every declared native, by its unique
    natives: HashMap<usize, natives::NativeInfo>,
    /// the definitions `@terminating` vouches for
    terminating: std::collections::HashSet<usize>,
    /// whether what is being checked was written in a linear module: what
    /// has no grade there is used once, not as often as wanted
    linear: bool,
    /// the type of every constant (data type, constructor, recursive
    /// definition), by its unique
    constants: HashMap<usize, VTy>,
    /// which data types are linear, as far as worked out (see [`linear`])
    linear_datas: HashMap<usize, bool>,
    /// the type parameters given what may be a linear type, to check once
    /// it is known (see [`linear`])
    linear_wants: Vec<linear::LinearWant>,
    /// the next key for a slot the checker makes itself (see
    /// [`Binder::key`]); counts down from the top, far from the renamer's
    /// uniques
    next_key: usize,
    /// the recursive definitions that are known to terminate, and so unfold
    /// (see [`termination`])
    transparent: HashMap<usize, termination::Unfold>,
    /// the program's effects and their operations, by unique
    effects: HashMap<usize, handlers::EffectInfo>,
    ops: HashMap<usize, handlers::OpInfo>,
    /// the data types a sequence literal can build, with the constructors
    /// it builds them with: `nil` and `cons`, by unique
    seqs: HashMap<usize, (usize, usize)>,
    /// every name the prelude declares (see [`hir::Module::prelude`])
    prelude: HashMap<String, usize>,
    /// what other packages' transparent definitions are, by unique: a
    /// constant of theirs evaluates to its value (see [`packages`])
    globals: HashMap<usize, Rc<Value>>,
    /// how many `let`s in the checker is, which makes a recursive group
    /// defined there local (see [`RecDef::local`])
    lets: usize,
    /// What a term had to fit but could not yet, its expected type stuck on
    /// a meta nothing had solved (see [`Checker::postpone`]): tried again
    /// once the definition it is in has been checked.
    postponed: Vec<Postponed>,
    /// The row variables made to keep a solved row open for more (see
    /// [`Checker::sibling`]): closed to the empty row when the top-level
    /// definition they were made in has been checked, if nothing filled
    /// them. Each with how many arguments it is applied to.
    opened: Vec<(MetaVar, usize)>,
    /// The metas a solution met applied to variables it may not mention,
    /// with which of their arguments it may (see [`Checker::solve`]): to be
    /// pruned of the others, and the solution tried again.
    prunes: std::cell::RefCell<Vec<(MetaVar, Vec<bool>)>>,
    /// The grades being worked out, of the parameters that say none in a
    /// module that is not linear (see [`Checker::settle_grade`]).
    inferred: HashMap<MetaVar, Inferred>,
    /// The joins of two alternatives' uses that mention a grade still being
    /// worked out, each standing for `join(p, q)` until it can be worked out
    /// (see [`Checker::join_usage`]), in the order they were made.
    joins: Vec<(MetaVar, Poly, Poly)>,
    /// How many of the explicit arrows ahead are a definition's parameters,
    /// on its signature's spine (or its lambdas, with none): those that say
    /// no grade have theirs worked out.
    infer_spine: usize,
    /// The names with more than one meaning where they are written, by
    /// node (see [`hir::Module::overloads`]), and the meaning chosen for
    /// each so far.
    overloads: HashMap<NodeId, Vec<usize>>,
    forced: HashMap<NodeId, usize>,
    /// The constructor chosen for each overloaded pattern.
    pattern_choice: HashMap<NodeId, usize>,
    /// The terms put off until their definition has been checked (see
    /// [`overloads`]), and each placeholder's meaning once chosen, with the
    /// level it was elaborated at.
    deferred: Vec<overloads::Deferred>,
    chosen: HashMap<MetaVar, (Rc<Core>, Lvl)>,
    /// the placeholders, whose solutions are no closed terms
    placeholders: HashSet<MetaVar>,
    /// For messages about an overloaded name: how it is written, by node;
    /// and each meaning, how it is written in full and where it is from.
    overload_names: HashMap<NodeId, String>,
    written_as: HashMap<usize, String>,
    origin: HashMap<usize, String>,
    /// where each type and constructor is from, by unique: its package, for
    /// a message about two of one name
    homes: HashMap<usize, String>,
    /// the uniques of this package's earlier parts (see `idyllic::parts`),
    /// which are its own as much as the module's are
    kin: Vec<std::ops::Range<usize>>,
    /// The definitions offered to the search for an auto argument, this
    /// package's and those it depends on, with their names for messages;
    /// and the searches waiting for their types (see [`auto`]).
    hints: HashSet<usize>,
    hint_names: HashMap<usize, String>,
    deferred_autos: Vec<auto::DeferredAuto>,
    /// the number literals waiting for their types (see [`literals`])
    deferred_literals: Vec<literals::DeferredLiteral>,
    /// the traits' records, this package's and those it depends on
    traits: HashSet<usize>,
    /// each trait's superclass projections, by its record type
    supers: HashMap<usize, Vec<usize>>,
    /// each trait's methods (see [`hir::Module::methods`]), and each
    /// instance with no parameters, with those of its fields that are a
    /// constant or a primitive: a method of it is that (see [`auto`])
    methods: HashMap<usize, (usize, usize, usize)>,
    instance_fields: HashMap<usize, Vec<Option<Rc<Core>>>>,
    /// a signature's implicit binders that equations naming one before
    /// them left to their matching, by name (see `check_against_sig`)
    quantified: Vec<hir::Ident>,
    /// the instances being unfolded (see `Checker::opened`): one whose
    /// default reads another of its fields reads it once it is there
    opening: std::cell::RefCell<Vec<usize>>,
    /// that a metavariable's solution is being read off what it was
    /// equated with keeping definitions by their names (see `Checker::solve`)
    naming: std::cell::Cell<bool>,
    /// the data types that live above `Type`, and where
    large: HashMap<usize, core::Level>,
    /// that the application being inferred is the head of `f x @{d}`, so
    /// that its auto argument is left for `d`
    auto_given: bool,
}

/// A parameter's grade being worked out: how many variables its meta is
/// applied to, and the constraints on it that waited for it, to be checked
/// once it is settled.
#[derive(Clone)]
struct Inferred {
    args: usize,
    waiting: Vec<Waiting>,
}

/// A use that had to fit a grade still being worked out.
#[derive(Clone)]
struct Waiting {
    ctx: Ctx,
    node: NodeId,
    name: String,
    used: Poly,
    allowed: Rc<Value>,
    /// a field's, with what it is a field of; `None` for a binder's
    field: Option<Option<String>>,
}

/// What is known about a constructor.  At run time it is its `tag` and its
/// `fields`; the type parameters it is also applied to are erased.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CtorInfo {
    pub name: String,
    /// the unique of its data type
    pub data: usize,
    /// its position among the type's constructors
    pub tag: usize,
    /// how many (implicit, erased) type parameters come before the fields
    pub params: usize,
    pub fields: usize,
    /// which of its fields a type may read off a known instance (see
    /// `Checker::opened`): a record's fields that are types, and a trait's
    /// superclasses
    #[serde(default)]
    pub opens: Vec<bool>,
}

/// An elaborated recursive definition.  In Core a recursive definition is a
/// constant ([`Core::Opaque`]) with no binding site, so that unification
/// never unfolds it; this is where its body is kept for whoever wants to
/// *run* the program.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RecDef {
    /// the unique of its binder, as in `Core::Opaque`
    pub unique: usize,
    pub name: String,
    /// The size of the context it was defined in: `core` may refer to that
    /// many enclosing variables, and every use of the definition sees the
    /// same ones at the same levels.
    pub lvl: Lvl,
    pub core: Rc<Core>,
    /// Whether it is defined inside another definition, in a `let`, rather
    /// than at the top level.  Its level alone cannot say: a definition
    /// inside the top-level one with two parameters before it is at the
    /// level of the top-level definition two after.
    pub local: bool,
}

/// A term's type, and the type it was to have, that could not be unified
/// yet: at `lvl`, for the term at `node`.
#[derive(Clone)]
struct Postponed {
    lvl: Lvl,
    actual: Rc<Value>,
    expected: Rc<Value>,
    node: NodeId,
}

/// One name a program can use, for whoever lists or searches them.
#[derive(Clone, Debug)]
pub struct Declared {
    /// where it is declared
    pub node: NodeId,
    pub unique: Option<usize>,
    /// as a program writes it: `double`, `Nat.Z` (a constructor lives under
    /// its type), `Math.double` (a name of a module other than the root)
    pub name: String,
    pub kind: DeclKind,
    /// its type; for a data type or an effect, its kind
    pub ty: Rc<Core>,
    /// whether the prelude declares it
    pub prelude: bool,
}

/// What a [`Declared`] name is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclKind {
    /// a definition: a function, a value, a type alias, an effect alias
    Value,
    Constructor,
    /// a data type
    Type,
    Effect,
    /// an effect's operation
    Operation,
}

pub struct TypeCheckResult {
    /// The type of the module's trailing expression, if it has one.
    pub body_type: Option<Rc<Core>>,
    /// What compiling the package made, for its dependents, for a build to
    /// keep, and for linking (see [`packages`]).
    pub compiled: Option<packages::CompiledPackage>,
    /// The prelude's own top-level definitions, as `let`s: the outermost of
    /// any program, linked or not.
    pub prelude_lets: Vec<(String, Rc<Core>)>,
    /// Whether the expression is `main ()`, the module having none of its
    /// own (see [`check_module`]).
    pub ran_main: bool,
    /// The types of the top-level definitions, in source order.
    /// With the node each is declared at, which says where in the source.
    pub decl_types: Vec<(NodeId, String, Rc<Core>)>,
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
    /// The declared effects' operations, by the unique of their names.
    pub ops: HashMap<usize, handlers::OpInfo>,
    /// The uniques of the declared data types, and of the native types.  As
    /// terms they are types, and so are erased.
    pub type_constructors: Vec<usize>,
    /// The natives, by unique: what each is at run time.
    pub natives: HashMap<usize, natives::NativeInfo>,
    /// Side table: node id to type, for every node the checker visited.
    pub node_types: Vec<Option<Rc<Core>>>,
    /// The final metacontext, so callers can inspect what was solved.
    pub metas: Vec<MetaEntry>,
    pub errors: Vec<TypeError>,
    /// Every name the program, the prelude and (once linked) the packages it
    /// uses declare, by unique: the package it is in, and its path there
    /// (`Math.double`, `Nat.Z`).
    pub symbols: HashMap<usize, (String, String)>,
    /// The unique of each top-level `let` of `program`, outermost first.
    pub toplevel: Vec<usize>,
    /// Every name the program and the prelude declare, with its type, in
    /// the order they were declared: what the REPL lists and searches.
    pub catalog: Vec<Declared>,
    /// Definitions every back end keeps, used or not (see [`prune`]): none,
    /// unless whoever lowers the program says otherwise.
    pub roots: HashSet<usize>,
    /// The top-level definitions whose values compute with no effects,
    /// which a back end may leave out when nothing uses them (see [`prune`])
    pub pure: HashSet<usize>,
    /// The checker, kept to work out the types of the program's terms
    /// again for whoever lowers it (see [`typer`]).
    pub typer: typer::Typer,
}

/// Type check a renamed, SCC-ordered module.  Never fails: every problem
/// becomes a [`TypeError`] and checking continues with an error value
/// standing in for the offending subterm.  `node_count` sizes the type table
/// (it is the length of the renamer's span table).
/// The prelude's declarations are not reported among the program's own.
///
/// With `entry`, a module with a `main` and no expression of its own runs
/// `main ()`, once `main` has been found to be `() -> () ! Eff`.
///
/// The module is checked against the compiled packages it depends on, which
/// come in an order where each follows those it depends on.
pub fn check_module(m: &hir::Module, node_count: usize, entry: bool, deps: &[&packages::CompiledPackage]) -> TypeCheckResult {
    // the types of what is checked, and of the answer, are printed as the
    // root module reads them (a group of another mode says so as it goes)
    core::print_linear(m.linear_body);
    let mut tc = Checker {
        metas: Vec::new(),
        node_types: vec![None; node_count],
        errors: Vec::new(),
        holes: Vec::new(),
        rec_defs: Vec::new(),
        ctors: HashMap::new(),
        datas: HashMap::new(),
        positive_params: HashMap::new(),
        natives: HashMap::new(),
        terminating: m.terminating.clone(),
        linear: false,
        constants: HashMap::new(),
        linear_datas: HashMap::new(),
        linear_wants: Vec::new(),
        next_key: usize::MAX,
        transparent: HashMap::new(),
        effects: HashMap::new(),
        ops: HashMap::new(),
        seqs: HashMap::new(),
        prelude: m.prelude.clone(),
        globals: HashMap::new(),
        lets: 0,
        postponed: Vec::new(),
        opened: Vec::new(),
        prunes: Default::default(),
        inferred: HashMap::new(),
        joins: Vec::new(),
        infer_spine: 0,
        overloads: m.overloads.clone(),
        forced: HashMap::new(),
        pattern_choice: HashMap::new(),
        deferred: Vec::new(),
        chosen: HashMap::new(),
        placeholders: HashSet::new(),
        overload_names: HashMap::new(),
        written_as: HashMap::new(),
        origin: HashMap::new(),
        homes: HashMap::new(),
        kin: deps.iter().filter(|p| p.name == m.package).map(|p| p.uniques.clone()).collect(),
        hints: m.hints.clone(),
        hint_names: HashMap::new(),
        deferred_autos: Vec::new(),
        deferred_literals: Vec::new(),
        traits: m.datas.iter().filter(|d| d.is_trait).filter_map(|d| d.name.unique).collect(),
        supers: m.supers.clone(),
        methods: m.methods.clone(),
        instance_fields: HashMap::new(),
        quantified: Vec::new(),
        opening: Default::default(),
        naming: Default::default(),
        large: HashMap::new(),
        auto_given: false,
    };
    for b in m.groups.iter().flat_map(hir::BindGroup::binds) {
        if let Some(u) = b.name.unique.filter(|u| m.hints.contains(u)) {
            let name = match m.qualifiers.get(&u) {
                Some(q) => format!("{q}.{}", b.name.text),
                None => b.name.text.clone(),
            };
            tc.hint_names.insert(u, name);
        }
    }
    for p in deps {
        tc.hints.extend(p.hints.iter().copied());
        tc.traits.extend(p.traits.iter().copied());
        tc.instance_fields.extend(p.instance_fields.iter().cloned());
        for (u, package, path) in &p.symbols {
            if p.hints.contains(u) {
                tc.hint_names.insert(*u, format!("{path}, from `{package}`"));
            }
        }
    }
    // (a package is known by its name and version: `Shapes@0.1.0`)
    let this = match &m.version {
        Some(v) => format!("`{}@{v}`", m.package),
        None => format!("`{}`", m.package),
    };
    let in_prelude: HashSet<usize> = m.prelude.values().copied().collect();
    for d in &m.datas {
        for u in std::iter::once(&d.name).chain(d.ctors.iter().map(|c| &c.name)).filter_map(|x| x.unique) {
            let home = if in_prelude.contains(&u) { "the prelude".to_string() } else { this.clone() };
            tc.homes.insert(u, home);
        }
    }
    for p in deps {
        for (u, package, _) in &p.symbols {
            tc.homes.insert(*u, format!("`{package}`"));
        }
    }
    if !m.overloads.is_empty() {
        tc.name_meanings(m, deps);
    }
    let ctx = Ctx::initial(&tc);
    let ctx = tc.import(&ctx, deps);
    // native types first, which the data types and effects may mention; and
    // native operations once those are known, as their types may mention them
    let ctx = tc.declare_natives(&ctx, &m.natives, true);
    let ctx = tc.declare_effects(&ctx, &m.effects);
    // the data types that mention no definition (see the SCC pass); the
    // rest once what they mention is
    let after = |i: usize| m.data_after.get(i).copied().unwrap_or(0);
    let first = (0..m.datas.len()).take_while(|&i| after(i) == 0).count();
    let ctx = tc.declare_datas(&ctx, &m.datas[..first]);
    let ctx = tc.declare_ops(&ctx, &m.effects);
    let ctx = tc.declare_natives(&ctx, &m.natives, false);
    // Top-level definitions are closed, so whatever they and the entry
    // expression use is only ever a definition or a primitive: there is no
    // binder left to hold the final usage to account.
    let (mut ctx, mut owed, mut lets) = (ctx, Vec::new(), Vec::new());
    let (mut g, mut d) = (0, first);
    while g < m.groups.len() || d < m.datas.len() {
        let upto = if d < m.datas.len() { after(d).min(m.groups.len()) } else { m.groups.len() };
        let (next, o, l) = tc.enter_groups(&ctx, &m.groups[g..upto.max(g)]);
        ctx = next;
        owed.extend(o);
        lets.extend(l);
        g = upto.max(g);
        let stop = (d..m.datas.len()).take_while(|&i| after(i) <= g).count();
        if stop > 0 {
            ctx = tc.declare_datas(&ctx, &m.datas[d..d + stop]);
            d += stop;
        } else if g >= m.groups.len() {
            // (data types the groups do not reach, declared last)
            ctx = tc.declare_datas(&ctx, &m.datas[d..]);
            d = m.datas.len();
        }
    }
    let ctx = ctx;
    // What is left over at the top is run there, where the runtime handles
    // the effects in the prelude's `Eff`, and nothing else.
    let world = tc.world(&ctx);
    for (group, pending) in m.groups.iter().zip(&owed) {
        if let (hir::BindGroup::NonRec(b), elab::Pending::NonRec { rhs, .. }) = (group, pending) {
            tc.unhandled(&ctx, b.rhs.id, rhs, &world);
        }
    }
    let prelude: HashSet<usize> = m.prelude.values().copied().collect();
    let main = m.groups.iter().flat_map(hir::BindGroup::binds).find(|b| b.name.unique.is_some() && b.name.unique == m.main);
    let main_fits = main.map(|b| tc.check_main(&ctx, b, world.clone()));
    // what the program uses of its top-level definitions: its expression's
    // uses, or `main`, used once
    let mut uses: Option<Usage> = main.map(|b| b.name.unique.map_or_else(Usage::none, Usage::var));
    let body = match (&m.body, main.zip(main_fits)) {
        (Some(body), _) => {
            // in the root module's mode
            tc.linear = m.linear_body;
            let mark = tc.rec_defs.len();
            let (core, ty, usage) = tc.infer(&ctx, body);
            uses = Some(usage.clone());
            tc.settle_deferred();
            let core = tc.resolve(&core, ctx.lvl);
            tc.resolve_rec_defs(mark);
            let core = tc.specialize(&ctx, &core, ctx.lvl);
            tc.specialize_rec_defs(&ctx, mark);
            // (a `main` that is wrong has said so already)
            if main_fits != Some(false) {
                tc.unhandled(&ctx, body.id, &usage, &world);
            }
            Some((core, tc.quote(ctx.lvl, ty)))
        }
        // `main ()`
        (None, Some((main, true))) if entry => ctx.lookup(&main.name).map(|(f, _, _)| {
            let call = Rc::new(Core::App(f, Rc::new(Core::Lit(idyllic_ast::Lit::Unit))));
            (call, Rc::new(Core::LitTy(Builtin::Unit)))
        }),
        (None, _) => None,
    };
    // `idyll test`: the tests, in place of the expression or `main`
    let body = if m.testing && entry {
        uses = Some(m.tests.iter().fold(Usage::none(), |u, t| u + Usage::var(*t)));
        tc.tests_program(&ctx, m, deps, world.clone()).map(|c| (c, Rc::new(Core::LitTy(Builtin::Unit))))
    } else {
        body
    };
    let ran_main = m.body.is_none() && body.is_some();
    let program = body.as_ref().map(|(core, _)| Checker::wrap_lets(core.clone(), lets.clone()));
    let body_type = body.as_ref().map(|(_, ty)| ty.clone());
    // instances of traits: where they live, and that no two overlap
    tc.check_instances(&ctx, m);
    tc.report_holes();
    tc.retry_postponed(true);
    tc.close_opened();
    tc.close_rows();
    tc.check_linear_wants();
    tc.linear_top_level(&ctx, &m.groups, &owed, uses);
    tc.report_unsolved_metas();
    // the prelude's own definitions, which come first
    let prelude_lets: Vec<(String, Rc<Core>)> = m
        .groups
        .iter()
        .zip(&lets)
        .filter(|(g, _)| g.binds().iter().all(|b| b.name.unique.is_some_and(|u| prelude.contains(&u))))
        .filter_map(|(_, w)| w.clone())
        .collect();
    // what the program declares: data types with their constructors, and
    // definitions (data types are renamed first, so for the order they were
    // written in, ask the span table)
    // (a constructor is reported under its type, where it lives, and a name
    // of a module other than the root under that module)
    // (the prelude's are kept apart, for the catalog: a prelude type's
    // constructors are the prelude's, `use`d or not)
    let mut declared: Vec<(NodeId, String, Option<usize>, DeclKind, bool)> = Vec::new();
    let qualified = |x: &hir::Ident| match x.unique.and_then(|u| m.qualifiers.get(&u)) {
        Some(q) => format!("{q}.{}", x.text),
        None => x.text.clone(),
    };
    let in_prelude = |x: &hir::Ident| x.unique.is_some_and(|u| prelude.contains(&u));
    let named = |id: NodeId, x: &hir::Ident, kind| (id, qualified(x), x.unique, kind, in_prelude(x));
    for d in &m.datas {
        declared.push(named(d.id, &d.name, DeclKind::Type));
        let ctor = |c: &hir::CtorDecl| {
            (c.id, format!("{}.{}", qualified(&d.name), c.name.text), c.name.unique, DeclKind::Constructor, in_prelude(&d.name))
        };
        declared.extend(d.ctors.iter().map(ctor));
    }
    for e in &m.effects {
        declared.push(named(e.id, &e.name, DeclKind::Effect));
        declared.extend(e.ops.iter().map(|op| named(op.id, &op.name, DeclKind::Operation)));
    }
    declared.extend(m.groups.iter().flat_map(hir::BindGroup::binds).map(|b| named(b.id, &b.name, DeclKind::Value)));
    declared.sort_by_key(|(id, ..)| *id);
    let catalog: Vec<Declared> = declared
        .iter()
        .filter_map(|(id, name, unique, kind, prelude)| {
            let ty = tc.node_types.get(id.0)?.clone()?;
            Some(Declared { node: *id, unique: *unique, name: name.clone(), kind: *kind, ty, prelude: *prelude })
        })
        .collect();
    // every name the program and the prelude declare, by unique, with the
    // package it is in and its path there, for the symbols a back end names
    // things by: `Math.double`, `Nat.Z`, `Console.putStr`
    let mut symbols: HashMap<usize, (String, String)> = HashMap::new();
    {
        let own = |u: usize| (m.uniques.0..m.uniques.1).contains(&u);
        // a package with a version is known by both: `Util@0.1.0`
        let this = match &m.version {
            Some(v) => format!("{}@{v}", m.package),
            None => m.package.clone(),
        };
        let mut add = |x: &hir::Ident, path: String| {
            if let Some(u) = x.unique {
                let package = if own(u) { this.clone() } else { "Prelude".to_string() };
                symbols.insert(u, (package, path));
            }
        };
        for d in &m.datas {
            add(&d.name, qualified(&d.name));
            for c in &d.ctors {
                add(&c.name, format!("{}.{}", qualified(&d.name), c.name.text));
            }
        }
        for e in &m.effects {
            add(&e.name, qualified(&e.name));
            for op in &e.ops {
                add(&op.name, format!("{}.{}", qualified(&e.name), op.name.text));
            }
        }
        for b in m.groups.iter().flat_map(hir::BindGroup::binds) {
            add(&b.name, qualified(&b.name));
        }
    }
    // the unique of each top-level `let` of the program, in order
    let toplevel: Vec<usize> = m
        .groups
        .iter()
        .zip(&lets)
        .filter(|(_, w)| w.is_some())
        .filter_map(|(g, _)| match g {
            hir::BindGroup::NonRec(b) => b.name.unique,
            hir::BindGroup::Rec(_) => None,
        })
        .collect();
    // (those a definition, not a computation with effects, binds)
    let pure: HashSet<usize> = toplevel
        .iter()
        .copied()
        .filter(|u| matches!(ctx.names.get(u), Some(Entry::Local(level)) if ctx.binders[*level].info == BinderInfo::Defined))
        .collect();
    let decl_types = catalog.iter().filter(|d| !d.prelude).map(|d| (d.node, d.name.clone(), d.ty.clone())).collect();
    let meta_solutions: Vec<Option<Rc<Core>>> = tc
        .metas
        .iter()
        .enumerate()
        .map(|(i, e)| match e {
            // metas are closed, so their solutions make sense in any context;
            // but for an overloaded name's placeholders, which were written
            // over (see [`overloads`])
            MetaEntry::Solved(_, _) if tc.placeholders.contains(&i) => None,
            MetaEntry::Solved(v, _) => Some(tc.quote(0, v.clone())),
            MetaEntry::Unsolved(_) => None,
        })
        .collect();
    let rec_defs = std::mem::take(&mut tc.rec_defs);
    let mut compiled = tc.compile_package(m, &ctx, &lets, body, &rec_defs, meta_solutions.clone());
    compiled.main = ran_main;
    compiled.symbols = symbols
        .iter()
        .filter(|(u, _)| (m.uniques.0..m.uniques.1).contains(*u))
        .map(|(u, (p, path))| (*u, p.clone(), path.clone()))
        .collect();
    compiled.symbols.sort();
    let compiled = Some(compiled);
    let type_constructors = tc
        .datas
        .keys()
        .copied()
        .chain(tc.natives.iter().filter(|(_, n)| n.native.is_type()).map(|(u, _)| *u))
        .collect();
    let natives = tc.natives.clone();
    let ctors = tc.ctors.clone();
    let ops = tc.ops.clone();
    let node_types = std::mem::take(&mut tc.node_types);
    let metas = tc.metas.clone();
    let mut errors = std::mem::take(&mut tc.errors);
    for e in &mut errors {
        if let TypeErrorKind::Mismatch { expected, actual } = &mut e.kind {
            *expected = tc.solved_in(expected);
            *actual = tc.solved_in(actual);
        }
    }
    TypeCheckResult {
        body_type,
        compiled,
        prelude_lets,
        ran_main,
        decl_types,
        program,
        rec_defs,
        meta_solutions,
        ctors,
        ops,
        type_constructors,
        natives,
        node_types,
        metas,
        errors,
        symbols,
        toplevel,
        roots: HashSet::new(),
        pure,
        catalog,
        typer: typer::Typer::new(tc),
    }
}

impl Checker {
    /// A type an error shows, with what its metas came to since it was
    /// reported: a number literal's type, say, which is known only once its
    /// definition is done (see [`literals`]). A meta whose answer mentions
    /// a variable is left as it is.
    fn solved_in(&self, ty: &Rc<Core>) -> Rc<Core> {
        ty.rewrite(0, &|c, _| match c {
            Core::Meta(m) => match &self.metas[*m] {
                MetaEntry::Solved(v, _) => {
                    let answer = self.quote(0, v.clone());
                    let open = std::cell::Cell::new(false);
                    answer.rewrite(0, &|c, d| {
                        if matches!(c, Core::Var(i, _) if *i >= d) {
                            open.set(true);
                        }
                        None
                    });
                    (!open.get()).then(|| self.solved_in(&answer))
                }
                _ => None,
            },
            _ => None,
        })
    }

    fn error(&mut self, node: NodeId, kind: TypeErrorKind) {
        let notes = self.homes_of(&kind);
        self.errors.push(TypeError { node, kind, notes });
    }

    /// For an error that shows two types: each name in them that stands for
    /// two different types, and where each is from, as Meadow says it.
    fn homes_of(&self, kind: &TypeErrorKind) -> Vec<String> {
        let (a, b) = match kind {
            TypeErrorKind::Mismatch { expected, actual } => (expected, actual),
            TypeErrorKind::BinderMismatch { expected, given } => (expected, given),
            _ => return Vec::new(),
        };
        let (ours, theirs) = (core::opaques(a), core::opaques(b));
        let mut notes = Vec::new();
        for (u, name) in &ours {
            for (v, other) in &theirs {
                if name == other && u != v && !notes.iter().any(|n: &String| n.starts_with(&format!("`{name}` "))) {
                    let home = |u: &usize| self.homes.get(u).cloned().unwrap_or_else(|| "somewhere else".into());
                    notes.push(format!("`{name}` is two types here: the one expected is from {}, this one from {}", home(u), home(v)));
                }
            }
        }
        notes
    }

    fn record(&mut self, node: NodeId, ty: Rc<Core>) {
        if let Some(slot) = self.node_types.get_mut(node.0) {
            *slot = Some(ty);
        }
    }

    /// The effects the runtime handles, at the top of a program: the
    /// prelude's `Eff` (whatever the program means by `Eff`), or failing
    /// that, `Div`.
    fn world(&mut self, ctx: &Ctx) -> Rc<Value> {
        let eff = self.prelude.get("Eff").map(|&u| hir::Ident { unique: Some(u), text: "Eff".into() });
        match eff.and_then(|x| ctx.lookup(&x)) {
            Some((core, _, _)) => self.eval(&ctx.env, &core),
            None => self.row(std::iter::once(Rc::new(Value::LitTy(Builtin::Div)))),
        }
    }

    /// Report the effects in `usage` that nothing will handle: all but the
    /// `world`'s.
    fn unhandled(&mut self, ctx: &Ctx, node: NodeId, usage: &Usage, world: &Rc<Value>) {
        let Some(effects) = self.effects_of(usage) else { return };
        let parts = self.row_parts(&effects);
        let handled = self.row_parts(world);
        // a row variable nothing has said anything about, at the top, where
        // nothing more will: no more effects (an effect-polymorphic function
        // given one with none, say)
        let open: Vec<Rc<Value>> =
            parts.tails.iter().filter(|(_, v)| matches!(*self.force(v.clone()), Value::Flex(..))).map(|(_, v)| v.clone()).collect();
        for tail in open {
            let none = value::empty_row();
            self.unify(ctx.lvl, tail, none);
        }
        let parts = self.row_parts(&effects);
        let left: Vec<Rc<Value>> = parts
            .labels
            .iter()
            .filter(|(k, _)| !handled.labels.iter().any(|(k2, _)| k2 == k))
            .chain(&parts.tails)
            .map(|(_, v)| v.clone())
            .collect();
        // (an effect that is an error has been reported already)
        if left.iter().any(|v| matches!(*self.force(v.clone()), Value::Error)) {
            return;
        }
        if !left.is_empty() {
            let row = self.row(left.into_iter());
            let row = self.quote(ctx.lvl, row);
            self.error(node, TypeErrorKind::UnhandledEffects(row));
        }
    }

    /// `main : () -> () ! e`, where the runtime handles every effect in
    /// `e`: it is what running the program runs.  Whether it is.
    fn check_main(&mut self, ctx: &Ctx, main: &hir::Bind, world: Rc<Value>) -> bool {
        let Some((_, ty, _)) = ctx.lookup(&main.name) else { return false };
        let unit = || Rc::new(Value::LitTy(Builtin::Unit));
        let fits = match &*self.force(ty.clone()) {
            Value::Pi { name, icit: Icit::Explicit, dom, cod, .. } => {
                let (result, effects) = self.split_comp(self.apply(cod, vvar(ctx.lvl, name.clone())));
                self.unify(ctx.lvl, dom.clone(), unit())
                    && self.unify(ctx.lvl + 1, result, unit())
                    && self.effects_fit(ctx.lvl + 1, effects, world)
            }
            Value::Error => true,
            _ => false,
        };
        if !fits {
            let ty = self.quote(ctx.lvl, ty);
            self.error(main.id, TypeErrorKind::MainType(ty));
        }
        fits
    }

    fn fresh_key(&mut self) -> usize {
        self.next_key -= 1;
        self.next_key
    }

    fn new_meta(&mut self, node: NodeId, origin: MetaOrigin, ty: Option<MetaTy>) -> MetaVar {
        self.metas.push(MetaEntry::Unsolved(MetaInfo { node, origin, ty }));
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

    /// A row variable nothing ever constrained stands for no more effects:
    /// there is nothing else it could be found to be.  Solved so, rather
    /// than reported unsolved.
    /// Close the row variables made to keep rows open (see
    /// [`Checker::sibling`]) that nothing filled: a definition's effects are
    /// then what it was found to do, and no more.
    fn close_opened(&mut self) {
        for (m, n) in std::mem::take(&mut self.opened) {
            let MetaEntry::Unsolved(info) = &self.metas[m] else { continue };
            let info = info.clone();
            let none = (0..n).fold(Rc::new(Core::Row(Vec::new(), Vec::new())), |body, _| Rc::new(Core::Lam("_".into(), body)));
            let solution = self.eval(&Default::default(), &none);
            self.metas[m] = MetaEntry::Solved(solution, info);
        }
    }

    fn close_rows(&mut self) {
        for m in 0..self.metas.len() {
            let MetaEntry::Unsolved(info) = &self.metas[m] else { continue };
            let Some(mt) = &info.ty else { continue };
            if self.row_kind(&mt.ty).is_none() {
                continue;
            }
            let info = info.clone();
            let n = info.ty.as_ref().map_or(0, |mt| mt.spine.len());
            let none = (0..n).fold(Rc::new(Core::Row(Vec::new(), Vec::new())), |body, _| Rc::new(Core::Lam("_".into(), body)));
            let solution = self.eval(&Default::default(), &none);
            self.metas[m] = MetaEntry::Solved(solution, info);
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
                MetaEntry::Unsolved(MetaInfo { node, origin: MetaOrigin::Inferred(desc), .. }) => {
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

/// The values a sized integer type has, `-128 to 127`.
fn number_range(ty: &str) -> String {
    match idyllic_builtins::Width::ALL.into_iter().find(|w| w.name() == ty) {
        Some(w) => format!("{} to {}", w.value(if w.signed() { 1 << (w.bits() - 1) } else { 0 }), w.value(if w.signed() { (1u64 << (w.bits() - 1)) - 1 } else { u64::MAX })),
        None => "what it holds".to_string(),
    }
}
