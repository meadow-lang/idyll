//! Core: the elaborated, fully explicit term language the type checker works
//! with.
//!
//! Unlike [`idyllic_hir`], Core uses de Bruijn *indices* and carries no node
//! ids — it is produced by elaboration and by quoting values back from the
//! semantic domain ([`super::value`]).  Because binders are nameless,
//! structural equality on [`Core`] *is* alpha-equality: the name in `Var` /
//! `Lam` / `Pi` / `Let` is only a display hint and is ignored by `PartialEq`.
//!
//! A Core term is closed up to the context it was elaborated in: every index
//! refers either to a binder inside the term (`Lam`, `Pi`, `Let`) or to an
//! entry of that context.  In particular a `let` is a real binder here, so a
//! term containing one can be evaluated anywhere its context is available.
//!
//! Grades are Core terms too, of type `Grade`: literals, `+`, `*`, `..`, and
//! anything else of that type (a variable, a metavariable).  Implicit
//! arguments are not distinguished in `App` and `Lam`: by the time a term is
//! Core every argument is there.

use idyllic_ast::{Icit, Lit};
use idyllic_builtins::{Builtin, Prim};
use crate::grade::Grade;
use std::fmt;
use std::rc::Rc;

/// de Bruijn index: 0 is the innermost binder.  Used in [`Core`].
pub type Idx = usize;
/// de Bruijn level: 0 is the outermost binder.  Used in values.
pub type Lvl = usize;
/// Identifies an entry in the metacontext.
pub type MetaVar = usize;

#[derive(Clone, Debug)]
pub enum Core {
    /// The sort.  This calculus is `Type : Type`.
    Type,
    /// A primitive type constant (`Int`, `Bool`, `Grade`, ...).
    LitTy(Builtin),
    Lit(Lit),
    /// A primitive operation (`+`), unapplied.  Arguments arrive by `App`.
    Prim(Prim),
    /// A variable, by de Bruijn index, plus a name hint for display.
    Var(Idx, String),
    /// A recursive definition, by the unique of its binder.  It never
    /// unfolds, so it needs no context slot: it is a constant.
    Opaque(usize, String),
    /// A metavariable: a not-yet-known term, to be solved by unification.
    /// Metas are always *closed*; the elaborator inserts them applied to the
    /// bound variables in scope, so this constructor never captures.
    Meta(MetaVar),
    /// `(x : (grade, in_type) dom) -> cod`: a function using its argument
    /// `grade` times, whose result type mentions it `in_type` times.  The
    /// hint is `"_"` for a non-dependent arrow.
    Pi { name: String, icit: Icit, grade: Rc<Core>, in_type: Rc<Core>, dom: Rc<Core>, cod: Rc<Core> },
    Lam(String, Rc<Core>),
    App(Rc<Core>, Rc<Core>),
    /// The graded modality `a [r]`: grade, then type.
    BoxTy(Rc<Core>, Rc<Core>),
    /// Promotion `[e]`.
    BoxIntro(Rc<Core>),
    /// The eliminator behind `let [x] = e in ..`, which elaborates to
    /// `let x = unbox e in ..`.
    Unbox(Rc<Core>),
    /// `let x = e in body`; index 0 in `body` is `x`.
    Let(String, Rc<Core>, Rc<Core>),
    /// A constant grade, `2` or `0..1`.
    GradeLit(Grade),
    GradeAdd(Rc<Core>, Rc<Core>),
    GradeMul(Rc<Core>, Rc<Core>),
    /// `lo..hi` between grades that are not (yet) constants.
    GradeRange(Rc<Core>, Rc<Core>),
    /// `case e of { C x y -> a; .. }`.  Each alternative's body binds its
    /// variables, the last innermost.
    Case(Rc<Core>, Rc<Vec<CaseAlt>>),
    /// A note for whoever runs the program, invisible to the type theory:
    /// the term means exactly what it would without it.
    Runtime(Note, Rc<Core>),
    /// Stands in for a subterm that failed to elaborate, so that one error
    /// does not cascade into a hundred.
    Error,
}

/// What an alternative matches: a declared constructor, by the unique of its
/// name, or one of the two `Bool` literals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Head {
    Con(usize),
    Bool(bool),
}

#[derive(Clone, Debug)]
pub struct CaseAlt {
    pub head: Head,
    /// the constructor's name, for display
    pub name: String,
    /// hints for the variables the body binds, one per field
    pub vars: Vec<String>,
    pub body: Rc<Core>,
}

/// What the elaborator knew, and erasure would otherwise forget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// An argument to a grade-`0` parameter, or the contents of a grade-`0`
    /// box: never used at run time, so never computed.
    Erased,
    /// On a lambda or an unboxing: the grade of the variable it binds.
    Binds(Grade),
}

impl Core {
    pub fn grade(g: Grade) -> Rc<Core> {
        Rc::new(Core::GradeLit(g))
    }

    fn is_grade(&self, g: Grade) -> bool {
        matches!(self, Core::GradeLit(h) if *h == g)
    }
}

/// Alpha-equality: name hints are display-only and do not participate.
impl PartialEq for Core {
    fn eq(&self, other: &Self) -> bool {
        use Core::*;
        match (self, other) {
            (Type, Type) | (Error, Error) => true,
            (LitTy(a), LitTy(b)) => a == b,
            (Lit(a), Lit(b)) => a == b,
            (Prim(a), Prim(b)) => a == b,
            (Var(i, _), Var(j, _)) => i == j,
            (Opaque(a, _), Opaque(b, _)) => a == b,
            (Meta(a), Meta(b)) => a == b,
            (
                Pi { icit, grade, in_type, dom, cod, .. },
                Pi { icit: icit2, grade: grade2, in_type: in_type2, dom: dom2, cod: cod2, .. },
            ) => icit == icit2 && grade == grade2 && in_type == in_type2 && dom == dom2 && cod == cod2,
            (Lam(_, b), Lam(_, b2)) => b == b2,
            (GradeLit(a), GradeLit(b)) => a == b,
            (App(f, a), App(g, b))
            | (BoxTy(f, a), BoxTy(g, b))
            | (GradeAdd(f, a), GradeAdd(g, b))
            | (GradeMul(f, a), GradeMul(g, b))
            | (GradeRange(f, a), GradeRange(g, b)) => f == g && a == b,
            (BoxIntro(a), BoxIntro(b)) | (Unbox(a), Unbox(b)) => a == b,
            (Runtime(n, a), Runtime(m, b)) => n == m && a == b,
            (Case(s, alts), Case(s2, alts2)) => {
                s == s2
                    && alts.len() == alts2.len()
                    && alts.iter().zip(alts2.iter()).all(|(a, b)| a.head == b.head && a.body == b.body)
            }
            (Let(_, a, b), Let(_, a2, b2)) => a == a2 && b == b2,
            _ => false,
        }
    }
}

impl Eq for Core {}

/// The primitive type of a literal.
pub fn lit_builtin(l: &Lit) -> Builtin {
    match l {
        Lit::Unit => Builtin::Unit,
        Lit::Int(_) => Builtin::Int,
        Lit::Bool(_) => Builtin::Bool,
        Lit::Str(_) => Builtin::Str,
    }
}

impl fmt::Display for Core {
    /// Render for error messages, in the surface syntax.  Binders are shown
    /// using their name hints, so shadowed names can look ambiguous; that is
    /// a display nicety only.  Grades are shown only when they differ from
    /// the defaults, the way they are written.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        /// `x : a`, `x : [s] a` or `x : (s, r) a`.
        fn binder(
            x: &str,
            (grade, in_type): (&Core, &Core),
            default: Grade,
            dom: &Core,
            f: &mut fmt::Formatter<'_>,
        ) -> fmt::Result {
            write!(f, "{x} : ")?;
            if !in_type.is_grade(Grade::DEFAULT_IN_TYPE) {
                write!(f, "(")?;
                go(grade, 0, f)?;
                write!(f, ", ")?;
                go(in_type, 0, f)?;
                write!(f, ") ")?;
            } else if grade.is_grade(Grade::ANY) {
                write!(f, "[] ")?;
            } else if !grade.is_grade(default) {
                write!(f, "[")?;
                go(grade, 0, f)?;
                write!(f, "] ")?;
            }
            go(dom, 0, f)
        }
        fn infix(
            (a, op, b): (&Core, &str, &Core),
            level: u8,
            prec: u8,
            f: &mut fmt::Formatter<'_>,
        ) -> fmt::Result {
            paren(prec > level, f, |f| {
                go(a, level, f)?;
                write!(f, "{op}")?;
                go(b, level + 1, f)
            })
        }
        fn go(c: &Core, prec: u8, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match c {
                Core::Type => write!(f, "Type"),
                Core::LitTy(b) => write!(f, "{}", b.name()),
                Core::Lit(l) => write!(f, "{l}"),
                Core::Var(_, n) | Core::Opaque(_, n) => write!(f, "{n}"),
                Core::Meta(m) => write!(f, "?{m}"),
                Core::Error => write!(f, "?!"),
                Core::Runtime(_, inner) => go(inner, prec, f),
                Core::Case(scrutinee, alts) => paren(prec > 0, f, |f| {
                    write!(f, "case ")?;
                    go(scrutinee, 0, f)?;
                    write!(f, " of {{ ")?;
                    for (i, alt) in alts.iter().enumerate() {
                        if i > 0 {
                            write!(f, "; ")?;
                        }
                        write!(f, "{}", alt.name)?;
                        for x in &alt.vars {
                            write!(f, " {x}")?;
                        }
                        write!(f, " -> ")?;
                        go(&alt.body, 0, f)?;
                    }
                    write!(f, " }}")
                }),
                Core::BoxIntro(e) => {
                    write!(f, "[")?;
                    go(e, 0, f)?;
                    write!(f, "]")
                }
                // the modality is postfix, and binds like an application
                Core::BoxTy(g, a) => paren(prec > 10, f, |f| {
                    go(a, 10, f)?;
                    if g.is_grade(Grade::ANY) {
                        return write!(f, " []");
                    }
                    write!(f, " [")?;
                    go(g, 0, f)?;
                    write!(f, "]")
                }),
                Core::Unbox(e) => paren(prec > 10, f, |f| {
                    write!(f, "unbox ")?;
                    go(e, 11, f)
                }),
                Core::GradeLit(g) => paren(!g.is_exact() && prec > 5, f, |f| write!(f, "{g}")),
                Core::GradeAdd(a, b) => infix((a, " + ", b), 6, prec, f),
                Core::GradeMul(a, b) => infix((a, " * ", b), 7, prec, f),
                Core::GradeRange(a, b) => paren(prec > 5, f, |f| {
                    go(a, 6, f)?;
                    write!(f, "..")?;
                    go(b, 6, f)
                }),
                // a bare or partially applied operator is written sectioned
                Core::Prim(p) => write!(f, "({})", p.name()),
                // a saturated binary primitive reads infix: left associative,
                // tighter than `->` and `:`, looser than application
                Core::App(g, b) if matches!(&**g, Core::App(h, _) if matches!(**h, Core::Prim(_))) => {
                    let Core::App(h, a) = &**g else { unreachable!() };
                    let Core::Prim(p) = **h else { unreachable!() };
                    infix((a, &format!(" {} ", p.name()), b), p.precedence(), prec, f)
                }
                Core::App(g, a) => paren(prec > 10, f, |f| {
                    go(g, 10, f)?;
                    write!(f, " ")?;
                    go(a, 11, f)
                }),
                Core::Lam(x, b) => paren(prec > 0, f, |f| {
                    write!(f, "\\{x} -> ")?;
                    go(b, 0, f)
                }),
                Core::Let(x, e, b) => paren(prec > 0, f, |f| {
                    write!(f, "let {x} = ")?;
                    go(e, 0, f)?;
                    write!(f, " in ")?;
                    go(b, 0, f)
                }),
                // an implicit binder is written like an explicit one, in
                // braces; it is erased unless it says otherwise
                Core::Pi { name, icit: Icit::Implicit, grade, in_type, dom, cod } => paren(prec > 0, f, |f| {
                    write!(f, "{{")?;
                    binder(name, (grade, in_type), Grade::ZERO, dom, f)?;
                    write!(f, "}} -> ")?;
                    go(cod, 0, f)
                }),
                // an anonymous, default-graded binder is just an arrow
                Core::Pi { name, grade, in_type, dom, cod, .. }
                    if name == "_"
                        && grade.is_grade(Grade::DEFAULT)
                        && in_type.is_grade(Grade::DEFAULT_IN_TYPE) =>
                {
                    paren(prec > 0, f, |f| {
                        go(dom, 1, f)?;
                        write!(f, " -> ")?;
                        go(cod, 0, f)
                    })
                }
                Core::Pi { name, grade, in_type, dom, cod, .. } => paren(prec > 0, f, |f| {
                    write!(f, "(")?;
                    binder(name, (grade, in_type), Grade::DEFAULT, dom, f)?;
                    write!(f, ") -> ")?;
                    go(cod, 0, f)
                }),
            }
        }
        fn paren(
            yes: bool,
            f: &mut fmt::Formatter<'_>,
            inner: impl FnOnce(&mut fmt::Formatter<'_>) -> fmt::Result,
        ) -> fmt::Result {
            if yes {
                write!(f, "(")?;
                inner(f)?;
                write!(f, ")")
            } else {
                inner(f)
            }
        }
        go(self, 0, f)
    }
}
