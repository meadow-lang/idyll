//! The bidirectional elaborator proper: `infer`, `check`, and binding groups.
//!
//! Everything self-describing lives in `infer`; everything that wants to be
//! told its type (a lambda, a grade literal) is handled specially in `check`;
//! and the two meet in `check`'s fallback, which infers and then unifies.
//!
//! Both modes return the term's [`Usage`] alongside its Core.  The rules are
//! Granule's, read bottom-up:
//!
//! * a variable uses itself once;
//! * subterms' usages add;
//! * `f e` with `f : (x : [r] a) -> b` uses what `e` uses `r` times over;
//! * `[e] : a [r]` uses what `e` uses `r` times over (promotion);
//! * `let [x] = e in body` gives `x` the box's grade;
//! * leaving a binder, the variable's count must fit its grade.
//!
//! A type former is a term like any other, so it has a usage too, and
//! `(x : (s, r) a) -> b` holds `b` to using `x` `r` times.  But a term is
//! never charged for its *types*: signatures, annotations and grades are
//! elaborated for their meaning and their usage dropped.

use super::core::{lit_builtin, CaseAlt, Core, Head, Level, Note};
use super::value::{vvar, Closure, Poly, VTy, Value};
use super::positivity::{DataShape, Field};
use super::{Checker, CtorInfo, Ctx, HoleInfo, MetaOrigin, RecDef, TypeErrorKind, Usage};
use idyllic_ast::{Icit, Lit};
use idyllic_builtins::{Builtin, Prim};
use crate::grade::Grade;
use idyllic_hir::{self as hir, NodeId, TermKind};
use std::rc::Rc;

/// What a binding group still owes once its scope has been checked: the
/// usage of each right-hand side, to be charged as often as the binding was
/// actually used.
pub(super) enum Pending {
    NonRec { unique: Option<usize>, rhs: Usage },
    Rec { uniques: Vec<Option<usize>>, rhs: Usage },
}

/// A transparent definition, to be re-bound around the elaborated body so the
/// Core stays closed over the context it started in.
pub(super) type LetWrap = Option<(String, Rc<Core>)>;

/// An elaborated grade: as written back, and as a value.
type ElabGrade = (Rc<Core>, Rc<Value>);

fn grade_ty() -> VTy {
    Rc::new(Value::LitTy(Builtin::Grade))
}

fn constant_grade(g: Grade) -> ElabGrade {
    (Core::grade(g), Poly::constant(g).into_value())
}

/// `a + b` or `a * b`, with the operator resolved to the primitive (so not
/// to a user's own `+`).  The other primitives mean nothing on grades.
fn arithmetic(t: &hir::Term) -> Option<(Prim, &hir::Term, &hir::Term)> {
    let TermKind::App(f, b) = &t.kind else { return None };
    let TermKind::App(op, a) = &f.kind else { return None };
    let TermKind::Var(op) = &op.kind else { return None };
    match Prim::from_unique(op.unique?)? {
        p @ (Prim::IntAdd | Prim::IntMul) => Some((p, a, b)),
        Prim::IntSub | Prim::IntLt | Prim::IntEq => None,
    }
}

impl Checker {
    /// Create a fresh metavariable, already applied to the bound variables of
    /// the context, and return the Core term standing for it.
    fn fresh_meta_with(&mut self, ctx: &Ctx, node: NodeId, origin: MetaOrigin) -> Rc<Core> {
        let m = self.new_meta(node, origin);
        ctx.bound_var_cores().fold(Rc::new(Core::Meta(m)), |acc, v| Rc::new(Core::App(acc, v)))
    }

    fn fresh_meta(&mut self, ctx: &Ctx, node: NodeId, desc: impl Into<String>) -> Rc<Core> {
        self.fresh_meta_with(ctx, node, MetaOrigin::Inferred(desc.into()))
    }

    /// Elaborate an explicit `_`: a meta for its value, remembered so it can
    /// be reported once unification has had a chance to fill it in.
    fn elab_hole(&mut self, ctx: &Ctx, node: NodeId, ty: VTy) -> Rc<Core> {
        let c = self.fresh_meta_with(ctx, node, MetaOrigin::Hole);
        let value = self.eval(&ctx.env, &c);
        self.holes.push(HoleInfo { lvl: ctx.lvl, node, value, ty });
        c
    }

    /// Check a term *as the type of something*: a signature, an annotation.
    /// Its usage is dropped — the something is not charged for its type.
    fn check_type(&mut self, ctx: &Ctx, t: &hir::Term) -> (Rc<Core>, VTy) {
        let (core, value, _level, _not_charged) = self.infer_type(ctx, t);
        (core, value)
    }

    /// Elaborate a term that has to be a type, and say which universe it is
    /// in.  A type whose universe is not known yet (a hole, a variable whose
    /// type is a meta) is taken to be in `Type`.  Unlike [`Self::check_type`]
    /// this keeps the usage, for the places where a type is the subject
    /// rather than a description of one: the domain and codomain of a
    /// function type being formed.
    fn infer_type(&mut self, ctx: &Ctx, t: &hir::Term) -> (Rc<Core>, VTy, Level, Usage) {
        let (core, ty, usage) = self.infer(ctx, t);
        let (core, ty) = self.insert(ctx, t.id, core, ty);
        let ty = self.force(ty);
        let level = match &*ty {
            Value::Type(n) => Some(*n),
            Value::Error => Some(0),
            Value::Flex(..) => self.unify(ctx.lvl, ty.clone(), Rc::new(Value::Type(0))).then_some(0),
            _ => None,
        };
        let Some(level) = level else {
            let expected = Rc::new(Core::Type(0));
            let actual = self.quote(ctx.lvl, ty);
            self.error(t.id, TypeErrorKind::Mismatch { expected, actual });
            return (Rc::new(Core::Error), Rc::new(Value::Error), 0, Usage::unknown());
        };
        let value = self.eval(&ctx.env, &core);
        (core, value, level, usage)
    }

    /// Check a term as a grade.  Grades are erased, so likewise.
    fn check_grade(&mut self, ctx: &Ctx, t: &hir::Term) -> ElabGrade {
        let (core, _erased) = self.check(ctx, t, grade_ty());
        let value = self.eval(&ctx.env, &core);
        (core, value)
    }

    /// The two grades of a binder, with the defaults for what is not written.
    fn binder_grades(
        &mut self,
        ctx: &Ctx,
        ann: Option<&hir::GradeAnn>,
        default: Grade,
    ) -> (ElabGrade, ElabGrade) {
        let subject = match ann {
            Some(ann) => self.check_grade(ctx, &ann.subject),
            None => constant_grade(default),
        };
        let in_type = match ann.and_then(|ann| ann.in_type.as_deref()) {
            Some(r) => self.check_grade(ctx, r),
            None => constant_grade(Grade::DEFAULT_IN_TYPE),
        };
        (subject, in_type)
    }

    /// The grade, if it is known outright (not a variable, not still a meta).
    fn constant(&self, grade: &Rc<Value>) -> Option<Grade> {
        self.to_poly(grade.clone()).as_constant()
    }

    /// A lambda, noting for the backend how often `x` may be used.
    fn lam(&self, name: String, grade: &Rc<Value>, body: Rc<Core>) -> Rc<Core> {
        let lam = Rc::new(Core::Lam(name, body));
        match self.constant(grade) {
            Some(g) => Rc::new(Core::Runtime(Note::Binds(g), lam)),
            None => lam,
        }
    }

    /// Something supplied `grade` times.  Zero times means it is never looked
    /// at, and the backend is told not to compute it: this is where erasure
    /// stops being a promise and becomes a fact.
    fn argument(&self, grade: &Rc<Value>, arg: Rc<Core>) -> Rc<Core> {
        match self.constant(grade) {
            Some(g) if g.is_zero() => Rc::new(Core::Runtime(Note::Erased, arg)),
            _ => arg,
        }
    }

    /// Apply a term to fresh metas for as long as its type asks for implicit
    /// arguments.
    fn insert(&mut self, ctx: &Ctx, node: NodeId, mut core: Rc<Core>, mut ty: VTy) -> (Rc<Core>, VTy) {
        loop {
            ty = self.force(ty);
            let Value::Pi { name, icit: Icit::Implicit, grade, cod, .. } = &*ty else { return (core, ty) };
            let arg = self.fresh_meta(ctx, node, format!("the implicit argument {name}"));
            let arg_v = self.eval(&ctx.env, &arg);
            core = Rc::new(Core::App(core, self.argument(grade, arg)));
            ty = self.apply(cod, arg_v);
        }
    }

    // -----------------------------------------------------------------------
    // Usage against grades
    // -----------------------------------------------------------------------

    /// `used ⊑ allowed`, for grades that may mention variables.
    ///
    /// Monomial by monomial: since `+` and `*` are monotone, a usage fits if
    /// each of its coefficients fits the corresponding one (an absent
    /// monomial having coefficient `0`).  So `r + 1 ⊑ r + 0..1`, whatever
    /// `r` is.  When that fails and a metavariable is involved, making the
    /// two equal is tried instead, which is how `[x]`'s grade gets inferred
    /// from how `x` may be used.
    fn grade_fits(&mut self, ctx: &Ctx, used: &Poly, allowed: Rc<Value>) -> bool {
        let used = self.renormalise(used);
        let allowed = self.to_poly(allowed);
        if allowed.as_constant() == Some(Grade::ANY) {
            return true;
        }
        let atoms = || used.terms().chain(allowed.terms()).flat_map(|(atoms, _)| atoms.iter());
        // an ill-formed grade has been reported already
        if atoms().any(|a| matches!(**a, Value::Error)) {
            return true;
        }
        let fits = used.monomials().chain(allowed.monomials()).all(|m| used.coefficient(m).fits(allowed.coefficient(m)));
        if fits {
            return true;
        }
        let flexible = atoms().any(|a| matches!(**a, Value::Flex(..)));
        flexible && self.unify(ctx.lvl, used.clone().into_value(), allowed.clone().into_value())
    }

    /// Leaving `x`'s binder: its uses must fit its grade.  `ctx` is the
    /// context inside the binder.
    fn leave_binder(
        &mut self,
        ctx: &Ctx,
        node: NodeId,
        x: &hir::Ident,
        allowed: Rc<Value>,
        usage: &mut Usage,
        in_type: bool,
    ) {
        let used = usage.take(x.unique);
        if usage.is_unknown() || self.grade_fits(ctx, &used, allowed.clone()) {
            return;
        }
        let used = self.quote(ctx.lvl, used.into_value());
        let allowed = self.quote(ctx.lvl, allowed);
        self.error(node, TypeErrorKind::UsageMismatch { name: x.text.clone(), used, allowed, in_type });
    }

    // -----------------------------------------------------------------------
    // Bindings
    // -----------------------------------------------------------------------

    /// Check a definition against its signature.  The variables the
    /// signature quantifies over are in scope in the definition (the renamer
    /// saw to that), so the implicit lambdas they call for are bound to those
    /// very names, which is what lets the body's annotations mention them.
    fn check_against_sig(&mut self, ctx: &Ctx, rhs: &hir::Term, sig: &hir::Term, vsig: VTy) -> (Rc<Core>, Usage) {
        let vsig = self.force(vsig);
        if let (
            TermKind::Pi(Some(x), Icit::Implicit, _, _, sig_cod),
            Value::Pi { name, icit: Icit::Implicit, grade, dom, cod, .. },
        ) = (&sig.kind, &*vsig)
        {
            let ctx2 = ctx.bind(Some(x), name, dom.clone());
            let cod_v = self.apply(cod, vvar(ctx.lvl, name.clone()));
            let (body, mut usage) = self.check_against_sig(&ctx2, rhs, sig_cod, cod_v);
            self.leave_binder(&ctx2, rhs.id, x, grade.clone(), &mut usage, false);
            return (self.lam(name.clone(), grade, body), usage);
        }
        self.check(ctx, rhs, vsig)
    }

    /// Check one dependency-ordered group, returning the context extended
    /// with its bindings, what it owes, and how to re-bind it in Core.
    fn check_group(&mut self, ctx: &Ctx, group: &hir::BindGroup) -> (Ctx, Pending, LetWrap) {
        match group {
            hir::BindGroup::NonRec(b) => {
                let (rhs, ty, usage) = match &b.ty {
                    // no signature: infer, and let the definition be transparent
                    None => self.infer(ctx, &b.rhs),
                    Some(sig) => {
                        let (_, vsig) = self.check_type(ctx, sig);
                        let (rhs, usage) = self.check_against_sig(ctx, &b.rhs, sig, vsig.clone());
                        (rhs, vsig, usage)
                    }
                };
                let quoted = self.quote(ctx.lvl, ty.clone());
                self.record(b.id, quoted);
                let v = self.eval(&ctx.env, &rhs);
                (
                    ctx.define(&b.name, ty, v),
                    Pending::NonRec { unique: b.name.unique, rhs: usage },
                    Some((b.name.text.clone(), rhs)),
                )
            }
            hir::BindGroup::Rec(bs) => {
                // 1. every member of a recursive group needs a signature;
                //    without one there is nothing to check the recursive
                //    occurrences against.
                let sigs: Vec<VTy> = bs
                    .iter()
                    .map(|b| match &b.ty {
                        None => {
                            self.error(b.id, TypeErrorKind::RecursiveBindingNeedsSignature(b.name.text.clone()));
                            Rc::new(Value::Error)
                        }
                        Some(sig) => self.check_type(ctx, sig).1,
                    })
                    .collect();
                // 2. declare all of them as opaque constants, so unification
                //    cannot unfold a recursive definition forever.
                let ctx_rec =
                    bs.iter().zip(&sigs).fold(ctx.clone(), |c, (b, ty)| c.declare_opaque(&b.name, ty.clone()));
                // 3. check each right-hand side against its own signature.
                let mut rhs = Usage::none();
                for (b, ty) in bs.iter().zip(&sigs) {
                    let (core, usage) = match &b.ty {
                        Some(sig) => self.check_against_sig(&ctx_rec, &b.rhs, sig, ty.clone()),
                        None => self.check(&ctx_rec, &b.rhs, ty.clone()),
                    };
                    if let Some(unique) = b.name.unique {
                        let name = b.name.text.clone();
                        self.rec_defs.push(RecDef { unique, name, lvl: ctx_rec.lvl, core });
                    }
                    rhs = rhs + usage;
                    let quoted = self.quote(ctx_rec.lvl, ty.clone());
                    self.record(b.id, quoted);
                }
                let uniques = bs.iter().map(|b| b.name.unique).collect();
                (ctx_rec, Pending::Rec { uniques, rhs }, None)
            }
        }
    }

    pub(super) fn enter_groups(&mut self, ctx: &Ctx, groups: &[hir::BindGroup]) -> (Ctx, Vec<Pending>, Vec<LetWrap>) {
        let mut ctx = ctx.clone();
        let (mut pending, mut wraps) = (Vec::new(), Vec::new());
        for g in groups {
            let (next, p, w) = self.check_group(&ctx, g);
            ctx = next;
            pending.push(p);
            wraps.push(w);
        }
        (ctx, pending, wraps)
    }

    /// Charge each definition's right-hand side for the uses its scope made
    /// of it: `let y = e in body` uses what `e` uses once per use of `y`.  A
    /// recursive group can call itself any number of times, so if it is used
    /// at all, whatever it captures is used without restriction.
    fn leave_groups(usage: Usage, pending: Vec<Pending>) -> Usage {
        pending.into_iter().rev().fold(usage, |mut usage, p| match p {
            Pending::NonRec { unique, rhs } => {
                let n = usage.take(unique);
                usage + rhs.scale(&n)
            }
            Pending::Rec { uniques, mut rhs } => {
                let mut used = false;
                for u in uniques {
                    used |= !usage.take(u).is_zero();
                    rhs.take(u);
                }
                usage + rhs.scale(&Poly::constant(if used { Grade::ANY } else { Grade::ZERO }))
            }
        })
    }

    pub(super) fn wrap_lets(body: Rc<Core>, wraps: Vec<LetWrap>) -> Rc<Core> {
        wraps.into_iter().rev().flatten().fold(body, |acc, (name, rhs)| Rc::new(Core::Let(name, rhs, acc)))
    }

    // -----------------------------------------------------------------------
    // Inference
    // -----------------------------------------------------------------------

    /// Synthesise the type of a term, elaborating it to Core on the way.
    /// Implicit arguments are *not* inserted at the end, so a bare `id` keeps
    /// its implicit binders; the places that need a function or a particular type
    /// insert them.
    pub(super) fn infer(&mut self, ctx: &Ctx, t: &hir::Term) -> (Rc<Core>, VTy, Usage) {
        let nid = t.id;
        let err = || (Rc::new(Core::Error), Rc::new(Value::Error), Usage::unknown());
        let universe = |n: Level| Rc::new(Value::Type(n));
        let result: (Rc<Core>, VTy, Usage) = match &t.kind {
            TermKind::Type => (Rc::new(Core::Type(0)), universe(1), Usage::none()),
            // `Type n` is `Type` applied to a level, which is how it parses.
            // (`Type [r]`, the modality on `Type`, is handled with the other
            // modalities below.)
            TermKind::App(f, n) if matches!(f.kind, TermKind::Type) && !matches!(n.kind, TermKind::BoxIntro(_)) => {
                let level = match &n.kind {
                    TermKind::Lit(Lit::Int(n)) => Level::try_from(n).ok().filter(|n| *n < Level::MAX),
                    _ => None,
                };
                match level {
                    Some(level) => (Rc::new(Core::Type(level)), universe(level + 1), Usage::none()),
                    None => {
                        self.error(n.id, TypeErrorKind::LevelNotANumber);
                        err()
                    }
                }
            }
            TermKind::Lit(l) => {
                (Rc::new(Core::Lit(l.clone())), Rc::new(Value::LitTy(lit_builtin(l))), Usage::none())
            }
            // a hole in inference position needs a meta for its type as well
            // as for its value
            TermKind::Hole => {
                let ty_c = self.fresh_meta(ctx, nid, "the type of this hole");
                let ty = self.eval(&ctx.env, &ty_c);
                (self.elab_hole(ctx, nid, ty.clone()), ty, Usage::none())
            }
            TermKind::Var(x) => match (x.unique, ctx.lookup(x)) {
                // the renamer already reported this one; stay quiet
                (None, _) => err(),
                (Some(u), Some((core, ty))) => (core, ty, Usage::var(u)),
                (Some(_), None) => {
                    self.error(nid, TypeErrorKind::UnknownVariable(x.text.clone()));
                    err()
                }
            },
            TermKind::Ann(e, sig) => {
                let (_, vsig) = self.check_type(ctx, sig);
                let (e_c, usage) = self.check(ctx, e, vsig.clone());
                (e_c, vsig, usage)
            }
            TermKind::Inf => (Core::grade(Grade::INF), grade_ty(), Usage::none()),
            TermKind::Range(lo, hi) => {
                let (lo_c, lo_v) = self.check_grade(ctx, lo);
                let (hi_c, hi_v) = self.check_grade(ctx, hi);
                let ends = (self.to_poly(lo_v).as_constant(), self.to_poly(hi_v).as_constant());
                match ends {
                    (Some(lo), Some(hi)) if Grade::range(lo, hi).is_none() => {
                        self.error(nid, TypeErrorKind::EmptyRange { lo, hi });
                        err()
                    }
                    _ => (Rc::new(Core::GradeRange(lo_c, hi_c)), grade_ty(), Usage::none()),
                }
            }
            TermKind::App(f, a) => {
                let (f_c, f_ty, f_usage) = self.infer(ctx, f);
                let (f_c, f_ty) = self.insert(ctx, f.id, f_c, f_ty);
                let pi = match (&*f_ty, &a.kind) {
                    // Juxtaposing brackets to a *type* is the graded modality,
                    // `a [r]`, not an application to a promoted `r`: types
                    // are not functions, so there is nothing to confuse.
                    (Value::Type(n), TermKind::BoxIntro(grade)) => {
                        let n = *n;
                        let (grade_c, _) = self.check_grade(ctx, grade);
                        // boxing a type leaves it in the universe it was in
                        let result = (Rc::new(Core::BoxTy(grade_c, f_c)), universe(n), f_usage);
                        return self.recorded(ctx, nid, result);
                    }
                    (Value::Pi { grade, dom, cod, .. }, _) => Some((grade.clone(), dom.clone(), cod.clone())),
                    (Value::Error, _) => return self.recorded(ctx, nid, err()),
                    // the head's type is not known to be a function yet: if
                    // it is still flexible, force it to be one by unifying
                    // with a fresh Pi
                    (Value::Flex(..), _) => {
                        let dom_c = self.fresh_meta(ctx, f.id, "the argument type of this function");
                        let dom = self.eval(&ctx.env, &dom_c);
                        let ctx_arg = ctx.bind(None, "x", dom.clone());
                        let cod_c = self.fresh_meta(&ctx_arg, f.id, "the result type of this function");
                        let cod = Closure { env: ctx.env.clone(), body: cod_c };
                        let grade = constant_grade(Grade::DEFAULT).1;
                        let pi = Rc::new(Value::Pi {
                            name: "x".into(),
                            icit: Icit::Explicit,
                            grade: grade.clone(),
                            in_type: constant_grade(Grade::DEFAULT_IN_TYPE).1,
                            dom: dom.clone(),
                            cod: cod.clone(),
                        });
                        self.unify(ctx.lvl, f_ty.clone(), pi).then_some((grade, dom, cod))
                    }
                    _ => None,
                };
                match pi {
                    Some((grade, dom, cod)) => {
                        let (a_c, a_usage) = self.check(ctx, a, dom);
                        let a_v = self.eval(&ctx.env, &a_c);
                        // the function uses its argument `grade` times, so
                        // everything the argument uses is used that often
                        let usage = f_usage + a_usage.scale(&self.to_poly(grade.clone()));
                        (Rc::new(Core::App(f_c, self.argument(&grade, a_c))), self.apply(&cod, a_v), usage)
                    }
                    None => {
                        let q = self.quote(ctx.lvl, f_ty.clone());
                        self.error(f.id, TypeErrorKind::NotAFunction(q));
                        err()
                    }
                }
            }
            // Forming a function type uses what its domain and codomain use,
            // and the codomain must use the binder as its second grade says.
            // It lives in the larger of their two universes: a function type
            // quantifying over `Type` is not itself in `Type`, which is what
            // keeps the hierarchy from collapsing into `Type : Type`.
            TermKind::Pi(mx, icit, ann, dom, cod) => {
                let default = if *icit == Icit::Implicit { Grade::ZERO } else { Grade::DEFAULT };
                let ((grade, _), (in_type, in_type_v)) = self.binder_grades(ctx, ann.as_ref(), default);
                let (dom_c, dom_v, dom_level, dom_usage) = self.infer_type(ctx, dom);
                let name = mx.as_ref().map_or("_", |x| x.text.as_str()).to_string();
                let ctx2 = ctx.bind(mx.as_ref(), &name, dom_v);
                let (cod_c, _, cod_level, mut cod_usage) = self.infer_type(&ctx2, cod);
                if let Some(x) = mx {
                    self.leave_binder(&ctx2, nid, x, in_type_v, &mut cod_usage, true);
                }
                let pi = Core::Pi { name, icit: *icit, grade, in_type, dom: dom_c, cod: cod_c };
                (Rc::new(pi), universe(dom_level.max(cod_level)), dom_usage + cod_usage)
            }
            TermKind::Lam(x, ann, ty_ann, body) => {
                let name = x.text.clone();
                // with metavariables an unannotated binder is no longer a
                // dead end: invent its type and let unification discover it
                let dom = match ty_ann {
                    Some(ty_ann) => self.check_type(ctx, ty_ann).1,
                    None => {
                        let c = self.fresh_meta(ctx, nid, format!("the type of the binder {name}"));
                        self.eval(&ctx.env, &c)
                    }
                };
                if ann.as_ref().is_some_and(|ann| ann.in_type.is_some()) {
                    self.error(nid, TypeErrorKind::UncheckedInTypeGrade(name.clone()));
                }
                let ((_, grade), (_, in_type)) = self.binder_grades(ctx, ann.as_ref(), Grade::DEFAULT);
                let ctx2 = ctx.bind(Some(x), &name, dom.clone());
                let (body_c, body_ty, mut usage) = self.infer(&ctx2, body);
                self.leave_binder(&ctx2, nid, x, grade.clone(), &mut usage, false);
                // the codomain may mention the binder, so quote it one level
                // in and close it over the *outer* environment
                let cod = Closure { env: ctx.env.clone(), body: self.quote(ctx2.lvl, body_ty) };
                let lam = self.lam(name.clone(), &grade, body_c);
                let pi = Value::Pi { name, icit: Icit::Explicit, grade, in_type, dom, cod };
                (lam, Rc::new(pi), usage)
            }
            // Promotion with nothing to say how many uses it must provide:
            // leave the grade to be found out.
            TermKind::BoxIntro(e) => {
                let grade_c = self.fresh_meta(ctx, nid, "the grade of this box");
                let grade = self.eval(&ctx.env, &grade_c);
                let (e_c, e_ty, usage) = self.infer(ctx, e);
                let usage = usage.scale(&self.to_poly(grade.clone()));
                (Rc::new(Core::BoxIntro(e_c)), Rc::new(Value::BoxTy(grade, e_ty)), usage)
            }
            TermKind::LetBox(x, boxed, body) => match self.unbox(ctx, x, boxed) {
                Some((ctx2, grade, unboxed, boxed_usage)) => {
                    let (body_c, ty, mut usage) = self.infer(&ctx2, body);
                    self.leave_binder(&ctx2, nid, x, grade, &mut usage, false);
                    let core = Rc::new(Core::Let(x.text.clone(), unboxed, body_c));
                    (core, ty, boxed_usage + usage)
                }
                None => err(),
            },
            TermKind::Let(groups, body) => {
                let (ctx2, pending, wraps) = self.enter_groups(ctx, groups);
                let (body_c, ty, usage) = self.infer(&ctx2, body);
                (Self::wrap_lets(body_c, wraps), ty, Self::leave_groups(usage, pending))
            }
            TermKind::Case(scrutinee, alts) => self.case_of(ctx, nid, scrutinee, alts, None),
        };
        self.recorded(ctx, nid, result)
    }

    fn recorded(&mut self, ctx: &Ctx, nid: NodeId, result: (Rc<Core>, VTy, Usage)) -> (Rc<Core>, VTy, Usage) {
        let quoted = self.quote(ctx.lvl, result.1.clone());
        self.record(nid, quoted);
        result
    }

    /// The shared half of `let [x] = boxed in ..`: elaborate the box, and
    /// extend the context with `x`, *defined* as `unbox boxed`.  Being a
    /// definition rather than an opaque binder is what lets the body's type
    /// mention `x` without it escaping its scope.
    fn unbox(
        &mut self,
        ctx: &Ctx,
        x: &hir::Ident,
        boxed: &hir::Term,
    ) -> Option<(Ctx, Rc<Value>, Rc<Core>, Usage)> {
        let (boxed_c, boxed_ty, boxed_usage) = self.infer(ctx, boxed);
        let (boxed_c, boxed_ty) = self.insert(ctx, boxed.id, boxed_c, boxed_ty);
        let parts = match &*boxed_ty {
            Value::BoxTy(grade, a) => Some((grade.clone(), a.clone())),
            Value::Error => return None,
            // not known to be a box yet (`\[x] -> ..` with no signature):
            // make it one.  The new metas take over the old one's spine, so
            // they depend on exactly what it could: metas made in *this*
            // context could mention the box itself, and then the old one —
            // made outside its binder — could not be solved with them.
            Value::Flex(_, sp) => {
                let mut part = |desc: &str| {
                    let m = self.new_meta(boxed.id, MetaOrigin::Inferred(desc.into()));
                    Rc::new(Value::Flex(m, sp.clone()))
                };
                let (grade, a) = (part("the grade of this box"), part("the type inside this box"));
                let box_ty = Rc::new(Value::BoxTy(grade.clone(), a.clone()));
                self.unify(ctx.lvl, boxed_ty.clone(), box_ty).then_some((grade, a))
            }
            _ => None,
        };
        let Some((grade, a)) = parts else {
            let q = self.quote(ctx.lvl, boxed_ty.clone());
            self.error(boxed.id, TypeErrorKind::NotABox(q));
            return None;
        };
        let unboxed = Rc::new(Core::Unbox(boxed_c));
        let unboxed = match self.constant(&grade) {
            Some(g) => Rc::new(Core::Runtime(Note::Binds(g), unboxed)),
            None => unboxed,
        };
        let value = self.eval(&ctx.env, &unboxed);
        Some((ctx.define(x, a, value), grade, unboxed, boxed_usage))
    }

    // -----------------------------------------------------------------------
    // Checking
    // -----------------------------------------------------------------------

    /// Check a term against an expected type, elaborating it to Core.
    pub(super) fn check(&mut self, ctx: &Ctx, t: &hir::Term, expected: VTy) -> (Rc<Core>, Usage) {
        let nid = t.id;
        let expected = self.force(expected);
        // Working out what this term should be already failed, and was
        // reported.  There is nothing to check it against, and elaborating it
        // anyway only invents metavariables nothing can solve and piles
        // derivative errors onto the real one.
        if matches!(*expected, Value::Error) {
            return (Rc::new(Core::Error), Usage::unknown());
        }
        let result = match (&t.kind, &*expected) {
            // An implicit function is wanted, and the surface language has no
            // implicit lambda to be one: bind the argument here.  It has no
            // name in the term, so it cannot be used, only inferred.
            (_, Value::Pi { name, icit: Icit::Implicit, grade, dom, cod, .. }) => {
                let ctx2 = ctx.bind(None, name, dom.clone());
                let cod_v = self.apply(cod, vvar(ctx.lvl, name.clone()));
                let (body_c, usage) = self.check(&ctx2, t, cod_v);
                (self.lam(name.clone(), grade, body_c), usage)
            }
            // a lambda gets its binder's type and grade from the expected Pi
            (TermKind::Lam(x, ann, ty_ann, body), Value::Pi { grade: pi_grade, in_type: pi_in_type, dom, cod, .. }) => {
                if let Some(ty_ann) = ty_ann {
                    let (_, ann_v) = self.check_type(ctx, ty_ann);
                    if !self.unify(ctx.lvl, ann_v.clone(), dom.clone()) {
                        let expected = self.quote(ctx.lvl, dom.clone());
                        let given = self.quote(ctx.lvl, ann_v);
                        self.error(ty_ann.id, TypeErrorKind::BinderMismatch { expected, given });
                    }
                }
                // If the binder's written grade disagrees with the type, say
                // so once, and then hold the body to what was *written*: that
                // is what its author counted against, so it avoids a second,
                // derivative complaint.
                let mut allowed = pi_grade.clone();
                if let Some(ann) = ann {
                    let mut written = vec![(&ann.subject, pi_grade, true)];
                    written.extend(ann.in_type.as_ref().map(|r| (r, pi_in_type, false)));
                    for (term, wanted, is_subject) in written {
                        let (_, given) = self.check_grade(ctx, term);
                        if self.unify(ctx.lvl, given.clone(), wanted.clone()) {
                            continue;
                        }
                        let name = x.text.clone();
                        let expected = self.quote(ctx.lvl, wanted.clone());
                        let kind = TypeErrorKind::BinderGradeMismatch {
                            name,
                            expected,
                            given: self.quote(ctx.lvl, given.clone()),
                        };
                        self.error(nid, kind);
                        if is_subject {
                            allowed = given;
                        }
                    }
                }
                let name = x.text.clone();
                let ctx2 = ctx.bind(Some(x), &name, dom.clone());
                let cod_v = self.apply(cod, vvar(ctx.lvl, name.clone()));
                let (body_c, mut usage) = self.check(&ctx2, body, cod_v);
                let lam = self.lam(name, &allowed, body_c);
                self.leave_binder(&ctx2, nid, x, allowed, &mut usage, false);
                (lam, usage)
            }
            // promotion: to provide `e` `grade` times, everything `e` uses
            // must be available `grade` times over
            (TermKind::BoxIntro(e), Value::BoxTy(grade, a)) => {
                let (e_c, usage) = self.check(ctx, e, a.clone());
                // a box nobody may open need not be filled
                (Rc::new(Core::BoxIntro(self.argument(grade, e_c))), usage.scale(&self.to_poly(grade.clone())))
            }
            // Where a grade is wanted, numerals and arithmetic are grades.
            // Nothing else is overloaded: a grade *variable* is an ordinary
            // variable, and gets here by the fallback.
            (TermKind::Lit(Lit::Int(n)), Value::LitTy(Builtin::Grade)) => match u64::try_from(n) {
                Ok(n) => (Core::grade(Grade::exact(n)), Usage::none()),
                Err(_) => {
                    self.error(nid, TypeErrorKind::GradeTooLarge);
                    (Rc::new(Core::Error), Usage::unknown())
                }
            },
            (TermKind::App(..), Value::LitTy(Builtin::Grade)) if arithmetic(t).is_some() => {
                let (op, a, b) = arithmetic(t).expect("guarded");
                let (a_c, _) = self.check_grade(ctx, a);
                let (b_c, _) = self.check_grade(ctx, b);
                let core = match op {
                    Prim::IntAdd => Core::GradeAdd(a_c, b_c),
                    Prim::IntMul => Core::GradeMul(a_c, b_c),
                    Prim::IntSub | Prim::IntLt | Prim::IntEq => unreachable!("not grade arithmetic"),
                };
                (Rc::new(core), Usage::none())
            }
            (TermKind::Case(scrutinee, alts), _) => {
                let (core, _, usage) = self.case_of(ctx, nid, scrutinee, alts, Some(expected.clone()));
                (core, usage)
            }
            // a hole is exactly what a metavariable is for: its type is
            // whatever was expected here, so nothing needs to be inferred
            (TermKind::Hole, _) => (self.elab_hole(ctx, nid, expected.clone()), Usage::none()),
            (TermKind::LetBox(x, boxed, body), _) => match self.unbox(ctx, x, boxed) {
                Some((ctx2, grade, unboxed, boxed_usage)) => {
                    let (body_c, mut usage) = self.check(&ctx2, body, expected.clone());
                    self.leave_binder(&ctx2, nid, x, grade, &mut usage, false);
                    (Rc::new(Core::Let(x.text.clone(), unboxed, body_c)), boxed_usage + usage)
                }
                None => (Rc::new(Core::Error), Usage::unknown()),
            },
            (TermKind::Let(groups, body), _) => {
                let (ctx2, pending, wraps) = self.enter_groups(ctx, groups);
                let (body_c, usage) = self.check(&ctx2, body, expected.clone());
                (Self::wrap_lets(body_c, wraps), Self::leave_groups(usage, pending))
            }
            // Everything else: infer, fill in any implicit arguments, then
            // unify.
            //
            // Unless nothing is known about the expected type yet.  Filling
            // in `id`'s implicit argument commits to using it at *some* one
            // type, and with no expected type to say which, that choice can
            // only end up unsolved: `id id` would be `id {?0} (id {?1})` with
            // nothing to determine `?1`.  So an unknown expected type takes
            // the term as it is, implicit binders and all — `?0 := {a : Type} -> a -> a`
            // — which is the most general thing it could be.  (Implicit
            // function types are ordinary types here, so that is allowed.)
            _ => {
                let (core, actual, usage) = self.infer(ctx, t);
                let (core, actual) = match &*expected {
                    Value::Flex(..) => (core, actual),
                    _ => self.insert(ctx, nid, core, actual),
                };
                if self.subtype(ctx.lvl, actual.clone(), expected.clone()) {
                    (core, usage)
                } else {
                    let e = self.quote(ctx.lvl, expected.clone());
                    let a = self.quote(ctx.lvl, actual);
                    self.error(nid, TypeErrorKind::Mismatch { expected: e, actual: a });
                    // An ill-typed term is never evaluated.  A definition's
                    // value is computed eagerly, and one that fails to check
                    // may not terminate: a universe mismatch is exactly how
                    // a looping term like Girard's paradox gets stopped.
                    (Rc::new(Core::Error), usage)
                }
            }
        };
        // record the *expected* type; it is the more informative one, and it
        // overwrites whatever `infer` may have just written for this node
        let quoted = self.quote(ctx.lvl, expected);
        self.record(nid, quoted);
        result
    }
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

impl Checker {
    /// Declare the module's data types.  All the types first and then all the
    /// constructors, so that a constructor may mention any of them.
    ///
    /// `data T (a : k) .. = C t .. | ..` declares two kinds of constant:
    ///
    /// ```text
    /// T : (a : k) -> .. -> Type
    /// C : {a : k} -> .. -> t -> .. -> T a ..
    /// ```
    ///
    /// A constructor takes the type's parameters implicitly and erased, and
    /// its fields linearly: building a value uses each part once, and taking
    /// it apart gives each part back once.  (A field to be used otherwise is
    /// a box, `List (a [])`.)
    ///
    /// A data type lives in `Type`, so each field must be a type in `Type`:
    /// a type with a field of type `Type` would be a set of all sets, which
    /// is a paradox.  And each type must be strictly positive in its own
    /// fields (see [`super::positivity`]), which is checked once every
    /// constructor has been elaborated.
    pub(super) fn declare_datas(&mut self, ctx: &Ctx, datas: &[hir::DataDecl]) -> Ctx {
        let explicit = |name: &str, grade: Grade, icit: Icit, dom: Rc<Core>, cod: Rc<Core>| {
            Rc::new(Core::Pi {
                name: name.to_string(),
                icit,
                grade: Core::grade(grade),
                in_type: Core::grade(Grade::DEFAULT_IN_TYPE),
                dom,
                cod,
            })
        };
        let mut ctx = ctx.clone();
        // the kind of each parameter, under the parameters before it
        let mut kinds: Vec<Vec<Rc<Core>>> = Vec::new();
        for d in datas {
            let mut under = ctx.clone();
            let mut ks = Vec::new();
            for (x, kind) in &d.params {
                let (k_c, k_v) = match kind {
                    Some(kind) => self.check_type(&under, kind),
                    None => (Rc::new(Core::Type(0)), Rc::new(Value::Type(0))),
                };
                ks.push(k_c);
                under = under.bind(Some(x), &x.text, k_v);
            }
            let ty = d.params.iter().zip(&ks).rev().fold(Rc::new(Core::Type(0)), |acc, ((x, _), k)| {
                explicit(&x.text, Grade::DEFAULT, Icit::Explicit, k.clone(), acc)
            });
            self.record(d.id, ty.clone());
            let ty = self.eval(&ctx.env, &ty);
            ctx = ctx.declare_opaque(&d.name, ty);
            if let Some(u) = d.name.unique {
                self.datas.insert(u, d.ctors.iter().filter_map(|c| c.name.unique).collect());
            }
            kinds.push(ks);
        }
        let mut shapes = Vec::new();
        for (d, ks) in datas.iter().zip(&kinds) {
            // the parameters again, now that the type itself is in scope
            let mut params = ctx.clone();
            for ((x, _), k) in d.params.iter().zip(ks) {
                let k_v = self.eval(&params.env, k);
                params = params.bind(Some(x), &x.text, k_v);
            }
            let mut shape = DataShape {
                unique: d.name.unique.unwrap_or(usize::MAX),
                params_at: ctx.lvl,
                params: d.params.len(),
                fields: Vec::new(),
            };
            for (tag, c) in d.ctors.iter().enumerate() {
                // each field under the parameters and the (anonymous) fields
                // before it, which is where it will sit in the type
                let mut under = params.clone();
                let mut fields = Vec::new();
                for f in &c.fields {
                    let (f_c, f_v, level, _not_charged) = self.infer_type(&under, f);
                    if level > 0 {
                        self.error(f.id, TypeErrorKind::LargeField { data: d.name.text.clone(), level });
                    }
                    let field = Field { node: f.id, ctor: c.name.text.clone(), lvl: under.lvl, ty: f_v.clone() };
                    shape.fields.push(field);
                    fields.push(f_c);
                    under = under.bind(None, "_", f_v);
                }
                let (n, m) = (d.params.len(), fields.len());
                let result = (0..n).fold(Rc::new(Core::Opaque(d.name.unique.unwrap_or(usize::MAX), d.name.text.clone())), |acc, j| {
                    Rc::new(Core::App(acc, Rc::new(Core::Var(n + m - j - 1, d.params[j].0.text.clone()))))
                });
                let ty = fields.into_iter().rev().fold(result, |acc, f| explicit("_", Grade::ONE, Icit::Explicit, f, acc));
                let ty = d
                    .params
                    .iter()
                    .zip(ks)
                    .rev()
                    .fold(ty, |acc, ((x, _), k)| explicit(&x.text, Grade::ZERO, Icit::Implicit, k.clone(), acc));
                self.record(c.id, ty.clone());
                let ty = self.eval(&ctx.env, &ty);
                ctx = ctx.declare_opaque(&c.name, ty);
                if let (Some(u), Some(data)) = (c.name.unique, d.name.unique) {
                    let info = CtorInfo { name: c.name.text.clone(), data, tag, params: n, fields: m };
                    self.ctors.insert(u, info);
                }
            }
            shapes.push(shape);
        }
        self.check_positivity(&shapes);
        ctx
    }

    /// What a `case` on something of type `ty` has to cover: each head, its
    /// name, and the types of its fields.
    fn alternatives_of(&mut self, ctx: &Ctx, ty: &VTy) -> Option<Vec<(Head, String, Vec<VTy>)>> {
        match &**ty {
            Value::LitTy(Builtin::Bool) => Some(vec![
                (Head::Bool(true), "True".into(), Vec::new()),
                (Head::Bool(false), "False".into(), Vec::new()),
            ]),
            Value::Opaque(data, _, spine) => {
                let ctors = self.datas.get(data)?.clone();
                let mut out = Vec::new();
                for c in ctors {
                    let info = self.ctors.get(&c)?.clone();
                    // instantiate the constructor's type at this type's
                    // arguments, then read off its fields
                    let mut cty = ctx.constant_type(c)?;
                    for arg in spine {
                        let super::value::Elim::App(arg) = arg else { return None };
                        let Value::Pi { cod, .. } = &*self.force(cty.clone()) else { return None };
                        cty = self.apply(cod, arg.clone());
                    }
                    let mut fields = Vec::new();
                    for i in 0..info.fields {
                        let forced = self.force(cty.clone());
                        let Value::Pi { dom, cod, .. } = &*forced else { return None };
                        fields.push(dom.clone());
                        // fields do not depend on one another
                        cty = self.apply(cod, vvar(ctx.lvl + i, "_"));
                    }
                    out.push((Head::Con(c), info.name, fields));
                }
                Some(out)
            }
            _ => None,
        }
    }

    /// `case scrutinee of alts`, against `expected` if there is one and
    /// otherwise at the type of the first alternative.
    ///
    /// The scrutinee is consumed and each field comes out linear.  Only one
    /// alternative runs, so the usage of the whole is the *join* of theirs:
    /// a variable used once in one alternative and not at all in another is
    /// used `0..1` times.
    ///
    /// The type of the result does not depend on the scrutinee (there is no
    /// motive), so matching teaches the types nothing: a `case` cannot refine
    /// an index.
    fn case_of(
        &mut self,
        ctx: &Ctx,
        nid: NodeId,
        scrutinee: &hir::Term,
        alts: &[hir::Alt],
        mut expected: Option<VTy>,
    ) -> (Rc<Core>, VTy, Usage) {
        let err = || (Rc::new(Core::Error), Rc::new(Value::Error), Usage::unknown());
        let (s_c, s_ty, s_usage) = self.infer(ctx, scrutinee);
        let (s_c, s_ty) = self.insert(ctx, scrutinee.id, s_c, s_ty);
        if matches!(*s_ty, Value::Error) {
            return err();
        }
        let Some(wanted) = self.alternatives_of(ctx, &s_ty) else {
            let q = self.quote(ctx.lvl, s_ty);
            self.error(scrutinee.id, TypeErrorKind::NotADataType(q));
            return err();
        };
        let mut covered: Vec<Head> = Vec::new();
        let mut elaborated: Vec<CaseAlt> = Vec::new();
        let mut usage: Option<Usage> = None;
        for alt in alts {
            let head = match &alt.head {
                hir::AltHead::Bool(b) => Head::Bool(*b),
                // the renamer reported an unknown name already
                hir::AltHead::Con(c) => match c.unique {
                    Some(u) => Head::Con(u),
                    None => continue,
                },
            };
            let name = match &alt.head {
                hir::AltHead::Bool(b) => if *b { "True" } else { "False" }.to_string(),
                hir::AltHead::Con(c) => c.text.clone(),
            };
            let Some((_, _, fields)) = wanted.iter().find(|(h, ..)| *h == head) else {
                let data = self.quote(ctx.lvl, s_ty.clone());
                self.error(alt.id, TypeErrorKind::NotAConstructorOf { ctor: name, data });
                continue;
            };
            if covered.contains(&head) {
                self.error(alt.id, TypeErrorKind::DuplicateAlternative(name));
                continue;
            }
            covered.push(head);
            if fields.len() != alt.vars.len() {
                let kind = TypeErrorKind::WrongFieldCount { ctor: name, expected: fields.len(), given: alt.vars.len() };
                self.error(alt.id, kind);
                continue;
            }
            let inner = alt.vars.iter().zip(fields).fold(ctx.clone(), |c, (x, ty)| c.bind(Some(x), &x.text, ty.clone()));
            let (body, mut body_usage) = match &expected {
                Some(ty) => self.check(&inner, &alt.body, ty.clone()),
                None => {
                    let (body, ty, body_usage) = self.infer(&inner, &alt.body);
                    expected = Some(ty);
                    (body, body_usage)
                }
            };
            let linear = constant_grade(Grade::ONE).1;
            for x in alt.vars.iter().rev() {
                self.leave_binder(&inner, alt.id, x, linear.clone(), &mut body_usage, false);
            }
            usage = Some(match usage {
                None => body_usage,
                Some(u) => u.join(body_usage),
            });
            elaborated.push(CaseAlt { head, name, vars: alt.vars.iter().map(|x| x.text.clone()).collect(), body });
        }
        let missing: Vec<String> =
            wanted.iter().filter(|(h, ..)| !covered.contains(h)).map(|(_, name, _)| name.clone()).collect();
        if !missing.is_empty() {
            self.error(nid, TypeErrorKind::MissingAlternatives(missing));
        }
        let ty = expected.unwrap_or_else(|| Rc::new(Value::Error));
        let usage = s_usage + usage.unwrap_or_else(Usage::unknown);
        (Rc::new(Core::Case(s_c, Rc::new(elaborated))), ty, usage)
    }
}
