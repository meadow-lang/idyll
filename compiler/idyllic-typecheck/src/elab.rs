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

use super::core::{lit_builtin, Core, Level, Lvl, Note};
use super::value::{vvar, Closure, Elim, Poly, VTy, Value};
use super::positivity::{DataShape, Field};
use super::matching::mentions;
use super::{BinderInfo, Checker, CtorInfo, Ctx, HoleInfo, MetaOrigin, MetaTy, RecDef, TypeErrorKind, Usage};
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
        Prim::IntSub | Prim::IntDiv | Prim::IntMod | Prim::IntLt | Prim::IntEq | Prim::StrAppend => None,
    }
}

impl Checker {
    /// Create a fresh metavariable, already applied to the bound variables of
    /// the context, and return the Core term standing for it.
    /// The meta's type is `ty` where that is known: see [`MetaTy`].
    pub(super) fn fresh_meta_with(&mut self, ctx: &Ctx, node: NodeId, origin: MetaOrigin, ty: Option<VTy>) -> Rc<Core> {
        let ty = ty.map(|ty| MetaTy { types: Rc::new(ctx.types()), spine: ctx.bound_levels().collect(), ty });
        let m = self.new_meta(node, origin, ty);
        ctx.bound_var_cores().fold(Rc::new(Core::Meta(m)), |acc, v| Rc::new(Core::App(acc, v)))
    }

    pub(super) fn fresh_meta(&mut self, ctx: &Ctx, node: NodeId, desc: impl Into<String>) -> Rc<Core> {
        self.fresh_meta_with(ctx, node, MetaOrigin::Inferred(desc.into()), None)
    }

    /// Declare a constant: a data type, a constructor or a recursive
    /// definition.
    pub(super) fn declare_constant(&mut self, ctx: &Ctx, x: &hir::Ident, ty: VTy) -> Ctx {
        if let Some(u) = x.unique {
            self.constants.insert(u, ty.clone());
        }
        ctx.declare_opaque(x, ty)
    }

    /// Elaborate an explicit `_`: a meta for its value, remembered so it can
    /// be reported once unification has had a chance to fill it in.
    fn elab_hole(&mut self, ctx: &Ctx, node: NodeId, ty: VTy) -> Rc<Core> {
        let c = self.fresh_meta_with(ctx, node, MetaOrigin::Hole, Some(ty.clone()));
        let value = self.eval(&ctx.env, &c);
        self.holes.push(HoleInfo { lvl: ctx.lvl, node, value, ty });
        c
    }

    /// Check a term *as the type of something*: a signature, an annotation.
    /// Its usage is dropped — the something is not charged for its type.
    /// And computing it must have no effects: only total code runs in types.
    pub(super) fn check_type(&mut self, ctx: &Ctx, t: &hir::Term) -> (Rc<Core>, VTy) {
        let (core, value, _level, usage) = self.infer_type(ctx, t);
        // a type that is not one is not used, so it causes nothing further
        if !self.pure(ctx, t.id, &usage) {
            return (Rc::new(Core::Error), Rc::new(Value::Error));
        }
        (core, value)
    }

    // -----------------------------------------------------------------------
    // Effects
    // -----------------------------------------------------------------------

    /// The effects a usage records, as one row; `None` if there are none.
    pub(super) fn effects_of(&self, usage: &Usage) -> Option<Rc<Value>> {
        if usage.is_unknown() {
            return None;
        }
        let row = self.row(usage.effects.iter().cloned());
        match &*row {
            Value::Row(r) if r.is_empty() => None,
            _ => Some(row),
        }
    }

    /// Report a type (or anything else that is never run) whose
    /// computation has effects.
    pub(super) fn pure(&mut self, ctx: &Ctx, node: NodeId, usage: &Usage) -> bool {
        let Some(effects) = self.effects_of(usage) else { return true };
        let effects = self.quote(ctx.lvl, effects);
        self.error(node, TypeErrorKind::ImpureType(effects));
        false
    }

    /// Where a lambda ends: its body's effects are what calling it does, and
    /// must be among those its type allows.  They are taken out of the usage:
    /// making a function has no effects.
    pub(super) fn close_effects(&mut self, ctx: &Ctx, node: NodeId, usage: &mut Usage, allowed: Rc<Value>) {
        let effects = self.row(usage.take_effects().into_iter());
        if usage.is_unknown() || self.effects_fit(ctx.lvl, effects.clone(), allowed.clone()) {
            return;
        }
        let (effects, allowed) = (self.shown_row(effects), self.shown_row(allowed));
        let (effects, allowed) = (self.quote(ctx.lvl, effects), self.quote(ctx.lvl, allowed));
        self.error(node, TypeErrorKind::EffectNotAllowed { effects, allowed });
    }

    /// `effects ⊆ allowed`.  A row variable of `allowed` that is still a
    /// metavariable takes whatever is missing, which is how the row of a
    /// function passed to an effect-polymorphic one is learned; and it stays
    /// open for more (`?e := {Log | ?t}`), so that what else is passed may
    /// add to it: `compose logged addAsk` is at `{Ask, Log}`.
    pub(super) fn effects_fit(&mut self, l: Lvl, effects: Rc<Value>, allowed: Rc<Value>) -> bool {
        // a row with an error in it has been reported already
        if self.row_has_error(&effects) || self.row_has_error(&allowed) {
            return true;
        }
        let (e, a) = (self.row_parts(&effects), self.row_parts(&allowed));
        // an effect with parameters is the allowed one of the same head,
        // if there is just one (`Throw String` is the `Throw ?e` allowed):
        // a row has one of each
        let mut paired = false;
        for (k, v) in &e.labels {
            if a.labels.iter().any(|(k2, _)| k2 == k) {
                continue;
            }
            let Some(head) = self.effect_head(v) else { continue };
            let same: Vec<Rc<Value>> =
                a.labels.iter().filter(|(_, w)| self.effect_head(w) == Some(head)).map(|(_, w)| w.clone()).collect();
            if let [w] = same.as_slice() {
                if self.unify(l, v.clone(), w.clone()) {
                    paired = true;
                }
            }
        }
        let (e, a) = if paired { (self.row_parts(&effects), self.row_parts(&allowed)) } else { (e, a) };
        let missing: Vec<Rc<Value>> = e
            .labels
            .iter()
            .filter(|(k, _)| !a.labels.iter().any(|(k2, _)| k2 == k))
            .chain(e.tails.iter().filter(|(k, _)| !a.tails.iter().any(|(k2, _)| k2 == k)))
            .map(|(_, v)| v.clone())
            .collect();
        if missing.is_empty() {
            return true;
        }
        let mut flex: Vec<Rc<Value>> =
            a.tails.iter().filter(|(_, v)| matches!(**v, Value::Flex(..))).map(|(_, v)| v.clone()).collect();
        // several row variables of `allowed` still to be found are made one,
        // which is enough to take what is missing
        if flex.len() > 1 {
            let first = flex[0].clone();
            if !flex[1..].iter().all(|x| self.unify(l, x.clone(), first.clone())) {
                return false;
            }
            flex.truncate(1);
        }
        match (flex.as_slice(), missing.as_slice()) {
            ([tail], _) => {
                let tail = tail.clone();
                let open = self.sibling(&tail);
                let rest = self.row(missing.into_iter().chain(open));
                self.unify(l, tail, rest)
            }
            // What is missing is row variables still to be found, such as
            // what resuming a handler does, used inside a function (and the
            // tails that what they were solved with left open): each may be
            // what is allowed here, and no more.
            ([], _) if missing.iter().all(|m| matches!(**m, Value::Flex(..))) => {
                missing.into_iter().all(|m| self.unify(l, m, allowed.clone()))
            }
            _ => false,
        }
    }

    /// A row as a message shows it: without the row variables made only to
    /// keep it open for more, which nothing has filled.
    pub(super) fn row_has_error(&self, row: &Rc<Value>) -> bool {
        let parts = self.row_parts(row);
        parts.labels.iter().chain(&parts.tails).any(|(_, v)| matches!(**v, Value::Error))
    }

    pub(super) fn shown_row(&self, row: Rc<Value>) -> Rc<Value> {
        let parts = self.row_parts(&row);
        let kept = parts.labels.into_iter().chain(parts.tails).map(|(_, v)| v).filter(|v| match &**v {
            Value::Flex(m, _) => !self.opened.iter().any(|(o, _)| o == m),
            _ => true,
        });
        self.row(kept)
    }

    /// A row, checked to be one, of either kind.
    fn check_row(&mut self, ctx: &Ctx, t: &hir::Term) -> Rc<Core> {
        self.row_part(ctx, t).0
    }

    /// An effect, a row variable or a row, in a row: whether it may hold
    /// operations resumed more than once.  One whose kind is not known yet
    /// (a row variable a signature binds for you) is taken to be `Effect`,
    /// resumed at most once, which is the default.
    fn row_part(&mut self, ctx: &Ctx, t: &hir::Term) -> (Rc<Core>, bool) {
        let effect = Rc::new(Value::LitTy(Builtin::Effect));
        let (c, ty, _) = self.infer(ctx, t);
        if matches!(*self.force(ty.clone()), Value::Flex(..)) {
            self.unify(ctx.lvl, ty, effect);
            return (c, false);
        }
        match self.row_kind(&ty) {
            Some(many) => (c, many),
            None if matches!(*self.force(ty.clone()), Value::Error) => (c, false),
            None => {
                let actual = self.quote(ctx.lvl, ty);
                self.error(t.id, TypeErrorKind::Mismatch { expected: Rc::new(Core::LitTy(Builtin::Effect)), actual });
                (Rc::new(Core::Error), false)
            }
        }
    }

    /// Whether a function type's codomain mentions its argument.
    pub(super) fn depends(&self, l: Lvl, cod: &Closure) -> bool {
        let body = self.apply(cod, vvar(l, "_"));
        super::matching::mentions(&self.quote(l + 1, body), 0)
    }

    /// Elaborate a term that has to be a type, and say which universe it is
    /// in.  A type whose universe is not known yet (a hole, a variable whose
    /// type is a meta) is taken to be in `Type`.  Unlike [`Self::check_type`]
    /// this keeps the usage, for the places where a type is the subject
    /// rather than a description of one: the domain and codomain of a
    /// function type being formed.
    fn infer_type(&mut self, ctx: &Ctx, t: &hir::Term) -> (Rc<Core>, VTy, Level, Usage) {
        // `()` is the unit value, and where a type is wanted, its type
        if let TermKind::Lit(Lit::Unit) = t.kind {
            let unit = Builtin::Unit;
            self.record(t.id, Rc::new(Core::Type(0)));
            return (Rc::new(Core::LitTy(unit)), Rc::new(Value::LitTy(unit)), 0, Usage::none());
        }
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
    /// `\name -> body`, where the binder has type `dom` in `ctx` and is used
    /// `grade` times: both noted for the lowering.
    pub(super) fn lam(&self, ctx: &Ctx, name: String, grade: &Rc<Value>, dom: &Rc<Value>, body: Rc<Core>) -> Rc<Core> {
        let lam = Rc::new(Core::Lam(name, body));
        let ty = Some(self.quote(ctx.lvl, dom.clone()));
        Rc::new(Core::Runtime(Note::Binds { grade: self.constant(grade), ty }, lam))
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

    /// Whether a type is small enough to solve meta `m` with, as far as is
    /// known.
    fn fits_meta(&self, ctx: &Ctx, m: usize, ty: &VTy) -> bool {
        let super::MetaEntry::Unsolved(info) = &self.metas[m] else { return true };
        let Some(mt) = &info.ty else { return true };
        let Value::Type(allowed) = *self.force(mt.ty.clone()) else { return true };
        self.sort_of(&mut ctx.types(), ty.clone()).is_none_or(|actual| actual <= allowed)
    }

    /// Apply a term to fresh metas for as long as its type asks for implicit
    /// arguments.
    pub(super) fn insert(&mut self, ctx: &Ctx, node: NodeId, mut core: Rc<Core>, mut ty: VTy) -> (Rc<Core>, VTy) {
        loop {
            ty = self.force(ty);
            let Value::Pi { name, icit: Icit::Implicit, grade, dom, cod, .. } = &*ty else { return (core, ty) };
            let desc = MetaOrigin::Inferred(format!("the implicit argument {name}"));
            let arg = self.fresh_meta_with(ctx, node, desc, Some(dom.clone()));
            let arg_v = self.eval(&ctx.env, &arg);
            core = Rc::new(Core::App(core, self.argument(grade, arg)));
            ty = self.split_comp(self.apply(cod, arg_v)).0;
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
    pub(super) fn grade_fits(&mut self, ctx: &Ctx, used: &Poly, allowed: Rc<Value>) -> bool {
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
    pub(super) fn leave_binder(
        &mut self,
        ctx: &Ctx,
        node: NodeId,
        x: &hir::Ident,
        allowed: Rc<Value>,
        usage: &mut Usage,
        in_type: bool,
    ) {
        self.leave_key(ctx, node, (x.unique, &x.text), allowed, usage, in_type);
    }

    /// [`Self::leave_binder`], for a slot known by the unique its uses are
    /// charged to.
    pub(super) fn leave_key(
        &mut self,
        ctx: &Ctx,
        node: NodeId,
        (key, name): (Option<usize>, &str),
        allowed: Rc<Value>,
        usage: &mut Usage,
        in_type: bool,
    ) {
        let used = usage.take(key);
        if usage.is_unknown() || self.grade_fits(ctx, &used, allowed.clone()) {
            return;
        }
        // `()` is the only value of its type, so there is nothing to lose by
        // dropping one: a `Unit` may be used less than its grade says
        let unit = ctx.binders.iter().rev().find(|b| key.is_some() && b.key == key).map(|b| self.force(b.ty.clone()));
        if matches!(unit.as_deref(), Some(Value::LitTy(Builtin::Unit))) {
            let at_most = self.to_poly(allowed.clone()).join(Poly::zero()).into_value();
            if self.grade_fits(ctx, &used, at_most) {
                return;
            }
        }
        let used = self.quote(ctx.lvl, used.into_value());
        let allowed = self.quote(ctx.lvl, allowed);
        self.error(node, TypeErrorKind::UsageMismatch { name: name.to_string(), used, allowed, in_type });
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
            let ctx2 = ctx.bind(Some(x), name, dom.clone()).graded(grade);
            let (cod_v, allowed) = self.split_comp(self.apply(cod, vvar(ctx.lvl, name.clone())));
            let (body, mut usage) = self.check_against_sig(&ctx2, rhs, sig_cod, cod_v);
            self.close_effects(&ctx2, rhs.id, &mut usage, allowed);
            self.leave_binder(&ctx2, rhs.id, x, grade.clone(), &mut usage, false);
            return (self.lam(ctx, name.clone(), grade, dom, body), usage);
        }
        self.check(ctx, rhs, vsig)
    }

    /// Check one dependency-ordered group, returning the context extended
    /// with its bindings, what it owes, and how to re-bind it in Core.
    /// What an unannotated binder's grade is where the checker is: `[]`,
    /// or `1` in a linear module.  An implicit binder's is `0` either way.
    pub(super) fn default_grade(&self) -> Grade {
        if self.linear { Grade::ONE } else { Grade::DEFAULT }
    }

    fn check_group(&mut self, ctx: &Ctx, group: &hir::BindGroup) -> (Ctx, Pending, LetWrap) {
        // in the mode of the module the group was written in
        let linear = match group {
            hir::BindGroup::NonRec(b) => b.linear,
            hir::BindGroup::Rec(bs) => bs.iter().any(|b| b.linear),
        };
        let outer = std::mem::replace(&mut self.linear, linear);
        let printing = crate::core::print_linear(linear);
        let result = self.check_group_in(ctx, group);
        crate::core::print_linear(printing);
        self.linear = outer;
        result
    }

    fn check_group_in(&mut self, ctx: &Ctx, group: &hir::BindGroup) -> (Ctx, Pending, LetWrap) {
        match group {
            hir::BindGroup::NonRec(b) => {
                let (rhs, ty, usage) = match &b.ty {
                    // no signature: infer, and let the definition be transparent
                    None => self.infer(ctx, &b.rhs),
                    // `x : b ! e`: computing `x` may have the effects `e`; a
                    // signature with no row allows none
                    Some(sig) => {
                        let (sig, allowed) = match &sig.kind {
                            TermKind::Eff(ty, row) => (&**ty, self.check_row(ctx, row)),
                            _ => (sig, Rc::new(Core::Row(Vec::new(), Vec::new()))),
                        };
                        let (_, vsig) = self.check_type(ctx, sig);
                        let (rhs, usage) = self.check_against_sig(ctx, &b.rhs, sig, vsig.clone());
                        if let Some(effects) = self.effects_of(&usage) {
                            let allowed = self.eval(&ctx.env, &allowed);
                            if !self.effects_fit(ctx.lvl, effects.clone(), allowed.clone()) {
                                let (effects, allowed) = (self.quote(ctx.lvl, effects), self.quote(ctx.lvl, allowed));
                                self.error(b.rhs.id, TypeErrorKind::EffectNotAllowed { effects, allowed });
                            }
                        }
                        (rhs, vsig, usage)
                    }
                };
                // what it left open is closed before its type is recorded,
                // which is then what it was found to do
                if self.lets == 0 {
                    self.retry_postponed(false);
                    self.close_opened();
                }
                let quoted = self.quote(ctx.lvl, ty.clone());
                self.record(b.id, quoted);
                // A definition with effects has a value only at run time: two
                // runs need not agree, so the types must not see it.
                let bound = match self.effects_of(&usage) {
                    Some(_) => ctx.define_opaque(&b.name, ty),
                    None => {
                        let v = self.eval(&ctx.env, &rhs);
                        ctx.define(&b.name, ty, v)
                    }
                };
                (bound, Pending::NonRec { unique: b.name.unique, rhs: usage }, Some((b.name.text.clone(), rhs)))
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
                    bs.iter().zip(&sigs).fold(ctx.clone(), |c, (b, ty)| self.declare_constant(&c, &b.name, ty.clone()));
                // 3. check each right-hand side against its own signature.
                let mut rhs = Usage::none();
                let mut members = Vec::new();
                for (b, ty) in bs.iter().zip(&sigs) {
                    let (core, usage) = match &b.ty {
                        Some(sig) => self.check_against_sig(&ctx_rec, &b.rhs, sig, ty.clone()),
                        None => self.check(&ctx_rec, &b.rhs, ty.clone()),
                    };
                    if let Some(unique) = b.name.unique {
                        let name = b.name.text.clone();
                        members.push((unique, core.clone()));
                        let local = self.lets > 0;
                        self.rec_defs.push(RecDef { unique, name, lvl: ctx_rec.lvl, core, local });
                    }
                    rhs = rhs + usage;
                    let quoted = self.quote(ctx_rec.lvl, ty.clone());
                    self.record(b.id, quoted);
                }
                // 4. if they terminate, they may compute in types from here
                //    on; if they may not, they have to say so, unless
                //    `@terminating` says they end after all
                if !self.check_termination(&ctx_rec, &members) {
                    for (b, ty) in bs.iter().zip(&sigs) {
                        if b.name.unique.is_some_and(|u| self.terminating.contains(&u)) {
                            continue;
                        }
                        let core = members.iter().find(|(u, _)| Some(*u) == b.name.unique).map(|(_, c)| c);
                        if let Some(core) = core {
                            if !self.declares_div(ctx_rec.lvl, ty.clone(), super::termination::arity(core)) {
                                self.error(b.id, TypeErrorKind::MayNotTerminate(b.name.text.clone()));
                            }
                        }
                    }
                }
                let uniques = bs.iter().map(|b| b.name.unique).collect();
                (ctx_rec, Pending::Rec { uniques, rhs }, None)
            }
        }
    }

    /// Whether a type says `! Div` where a call with `arity` arguments gets
    /// to: on the function type those arguments use up last.
    fn declares_div(&self, lvl: Lvl, mut ty: VTy, arity: usize) -> bool {
        if matches!(*self.force(ty.clone()), Value::Error) {
            return true;
        }
        let mut row = super::value::empty_row();
        for i in 0..arity {
            let forced = self.force(ty);
            let Value::Pi { name, cod, .. } = &*forced else { return false };
            (ty, row) = self.split_comp(self.apply(cod, vvar(lvl + i, name.clone())));
        }
        arity > 0 && self.row_parts(&row).labels.iter().any(|(_, l)| matches!(**l, Value::LitTy(Builtin::Div)))
    }

    /// The groups of a `let` block: what is defined there is local.
    fn enter_let(&mut self, ctx: &Ctx, groups: &[hir::BindGroup]) -> (Ctx, Vec<Pending>, Vec<LetWrap>) {
        self.lets += 1;
        let entered = self.enter_groups(ctx, groups);
        self.lets -= 1;
        entered
    }

    pub(super) fn enter_groups(&mut self, ctx: &Ctx, groups: &[hir::BindGroup]) -> (Ctx, Vec<Pending>, Vec<LetWrap>) {
        let mut ctx = ctx.clone();
        let (mut pending, mut wraps) = (Vec::new(), Vec::new());
        for g in groups {
            let (next, p, w) = self.check_group(&ctx, g);
            // what had to wait for the rest of the definition to be known
            self.retry_postponed(false);
            // and at the top level, the rows it left open are closed
            if self.lets == 0 {
                self.close_opened();
            }
            ctx = next;
            pending.push(p);
            wraps.push(w);
        }
        (ctx, pending, wraps)
    }

    /// A term of type `actual` was to have type `expected`, and they do not
    /// unify yet: keep the question for later if either is stuck on a meta
    /// applied to something (`?b Z` against `Vec Z a`), which only what is
    /// checked after it may solve.  An argument is checked before the
    /// application's result meets the type it is to have, and that result is
    /// often what says what such a meta is: in `Sigma Z Nil`, checked
    /// against `Sigma Nat (\n -> Vec n a)`, it is the result that solves
    /// `?b := \n -> Vec n a`, after `Nil` has been checked against `?b Z`.
    /// Whether the question was kept.
    pub(super) fn postpone(&mut self, lvl: Lvl, actual: &Rc<Value>, expected: &Rc<Value>, node: NodeId) -> bool {
        let stuck = |v: &Rc<Value>| matches!(&*self.force(v.clone()), Value::Flex(_, spine) if !spine.is_empty());
        if !(stuck(expected) || stuck(actual)) {
            return false;
        }
        self.postponed.push(super::Postponed { lvl, actual: actual.clone(), expected: expected.clone(), node });
        true
    }

    /// Ask the postponed questions again, until none can be answered; with
    /// `last`, what is still unanswered is a mismatch.
    pub(super) fn retry_postponed(&mut self, last: bool) {
        loop {
            let waiting = std::mem::take(&mut self.postponed);
            let before = waiting.len();
            for p in waiting {
                if !self.subtype(p.lvl, p.actual.clone(), p.expected.clone()) {
                    self.postponed.push(p);
                }
            }
            if self.postponed.len() == before {
                break;
            }
        }
        if last {
            for p in std::mem::take(&mut self.postponed) {
                let expected = self.quote(p.lvl, p.expected);
                let actual = self.quote(p.lvl, p.actual);
                self.error(p.node, TypeErrorKind::Mismatch { expected, actual });
            }
        }
    }

    /// Charge each definition's right-hand side for the uses its scope made
    /// of it: `let y = e in body` uses what `e` uses once per use of `y`.  A
    /// recursive group can call itself any number of times, so if it is used
    /// at all, whatever it captures is used without restriction.
    fn leave_groups(usage: Usage, pending: Vec<Pending>) -> Usage {
        pending.into_iter().rev().fold(usage, |mut usage, p| match p {
            Pending::NonRec { unique, rhs } => {
                let mut n = usage.take(unique);
                // A right-hand side with effects is run, once, whether its
                // value is used or not: `let u = putStr s in ..` uses `s`.
                if !rhs.effects.is_empty() {
                    if let Some(g) = n.as_constant() {
                        n = Poly::constant(g.at_least_once());
                    }
                }
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
                    TermKind::Lit(Lit::Int(n)) => Level::try_from(*n).ok().filter(|n| *n < Level::MAX),
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
                (Some(_), Some((core, ty, key))) => (core, ty, Usage::var(key)),
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
                // `() [r]`: the unit *type*, since that is what can be boxed
                let (f_c, f_ty, f_usage) = match (&f.kind, &a.kind) {
                    (TermKind::Lit(Lit::Unit), TermKind::BoxIntro(_)) => {
                        let (c, _, _, usage) = self.infer_type(ctx, f);
                        (c, universe(0), usage)
                    }
                    _ => self.infer(ctx, f),
                };
                let (f_c, f_ty) = self.insert(ctx, f.id, f_c, f_ty);
                match (&*f_ty, &a.kind) {
                    // Juxtaposing brackets to a *type* is the graded modality,
                    // `a [r]`, not an application to a promoted `r`: types
                    // are not functions, so there is nothing to confuse.
                    (Value::Type(n), TermKind::BoxIntro(grade)) => {
                        let n = *n;
                        let (grade_c, _) = self.check_grade(ctx, grade);
                        // boxing a type leaves it in the universe it was in
                        (Rc::new(Core::BoxTy(grade_c, f_c)), universe(n), f_usage)
                    }
                    // `a [r]` where nothing is known about `a` yet, as with a
                    // variable a signature binds for itself: brackets after
                    // something that could be a type make a modality, since a
                    // function is rarely applied to a promoted value
                    (Value::Flex(..), TermKind::BoxIntro(grade))
                        if self.unify(ctx.lvl, f_ty.clone(), universe(0)) =>
                    {
                        let (grade_c, _) = self.check_grade(ctx, grade);
                        (Rc::new(Core::BoxTy(grade_c, f_c)), universe(0), f_usage)
                    }
                    _ => self.apply_to(ctx, f.id, (f_c, f_ty, f_usage), a),
                }
            }
            // Forming a function type uses what its domain and codomain use,
            // and the codomain must use the binder as its second grade says.
            // It lives in the larger of their two universes: a function type
            // quantifying over `Type` is not itself in `Type`, which is what
            // keeps the hierarchy from collapsing into `Type : Type`.
            TermKind::Pi(mx, icit, ann, dom, cod) => {
                let default = if *icit == Icit::Implicit { Grade::ZERO } else { self.default_grade() };
                let ((grade, _), (in_type, in_type_v)) = self.binder_grades(ctx, ann.as_ref(), default);
                let name = mx.as_ref().map_or("_", |x| x.text.as_str()).to_string();
                let (dom_c, dom_v, dom_level, dom_usage) = match &dom.kind {
                    // a variable a signature mentions without binding it:
                    // its type is for unification to find, in whatever
                    // universe that turns out to be (a signature's own
                    // universe is never asked for)
                    TermKind::Omitted => {
                        let c = self.fresh_meta(ctx, dom.id, format!("the type of {name}"));
                        let v = self.eval(&ctx.env, &c);
                        (c, v, 0, Usage::none())
                    }
                    _ => self.infer_type(ctx, dom),
                };
                let ctx2 = ctx.bind(mx.as_ref(), &name, dom_v);
                let (cod_c, cod_level, mut cod_usage) = match &cod.kind {
                    // `a -> b ! e`
                    TermKind::Eff(ty, row) => {
                        let (ty_c, _, level, usage) = self.infer_type(&ctx2, ty);
                        let row_c = self.check_row(&ctx2, row);
                        (Rc::new(Core::Comp(ty_c, row_c)), level, usage)
                    }
                    _ => {
                        let (c, _, level, usage) = self.infer_type(&ctx2, cod);
                        (c, level, usage)
                    }
                };
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
                let ((_, grade), (_, in_type)) = self.binder_grades(ctx, ann.as_ref(), self.default_grade());
                let ctx2 = ctx.bind(Some(x), &name, dom.clone()).graded(&grade);
                let (body_c, body_ty, mut usage) = self.infer(&ctx2, body);
                // what calling it does is what its body does
                let effects = self.row(usage.take_effects().into_iter());
                let body_ty = self.comp(body_ty, effects);
                self.leave_binder(&ctx2, nid, x, grade.clone(), &mut usage, false);
                // the codomain may mention the binder, so quote it one level
                // in and close it over the *outer* environment
                let cod = Closure { env: ctx.env.clone(), body: self.quote(ctx2.lvl, body_ty) };
                let lam = self.lam(ctx, name.clone(), &grade, &dom, body_c);
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
                let (ctx2, pending, wraps) = self.enter_let(ctx, groups);
                let (body_c, ty, usage) = self.infer(&ctx2, body);
                // the type may not mention what only has a value at run time
                let quoted = self.quote(ctx2.lvl, ty.clone());
                let escaped = (ctx.lvl..ctx2.lvl)
                    .find(|&k| ctx2.binders[k].info == BinderInfo::Opaque && mentions(&quoted, ctx2.lvl - k - 1));
                let ty = match escaped {
                    Some(k) => {
                        self.error(nid, TypeErrorKind::EffectfulLetEscapes(ctx2.binders[k].name.clone()));
                        Rc::new(Value::Error)
                    }
                    None => ty,
                };
                (Self::wrap_lets(body_c, wraps), ty, Self::leave_groups(usage, pending))
            }
            TermKind::Case(scrutinee, clauses) => self.case_of(ctx, nid, scrutinee, clauses, None),
            // a row: effects and row variables
            TermKind::Row(labels, tail) => {
                let mut many = false;
                let mut parts = |this: &mut Self, ts: &mut dyn Iterator<Item = &hir::Term>| {
                    let mut out = Vec::new();
                    for t in ts {
                        let (c, m) = this.row_part(ctx, t);
                        many |= m;
                        out.push(c);
                    }
                    out
                };
                let labels = parts(self, &mut labels.iter());
                let tails = parts(self, &mut tail.iter().map(|t| &**t));
                let kind = if many { super::handlers::many_effect_kind() } else { Rc::new(Core::LitTy(Builtin::Effect)) };
                (Rc::new(Core::Row(labels, tails)), self.eval(&ctx.env, &kind), Usage::none())
            }
            TermKind::Eff(..) => {
                self.error(nid, TypeErrorKind::EffectOutsideArrow);
                err()
            }
            TermKind::Handle(body, clauses) => self.handle(ctx, nid, body, clauses, None),
            TermKind::Match(clauses) => self.infer_match(ctx, nid, clauses),
            TermKind::SeqNil | TermKind::SeqCons(..) => self.infer_seq(ctx, t),
            TermKind::ArrayLit(items) => self.array_literal(ctx, t, items, None),
            TermKind::Omitted => {
                let ty_c = self.fresh_meta(ctx, nid, "the type of this");
                let ty = self.eval(&ctx.env, &ty_c);
                let c = self.fresh_meta_with(ctx, nid, MetaOrigin::Inferred("this".into()), Some(ty.clone()));
                (c, ty, Usage::none())
            }
        };
        self.recorded(ctx, nid, result)
    }

    /// `f a`, for an `f` already elaborated at `node`, with its implicit
    /// arguments inserted.
    pub(super) fn apply_to(
        &mut self,
        ctx: &Ctx,
        node: NodeId,
        (f_c, f_ty, f_usage): (Rc<Core>, VTy, Usage),
        a: &hir::Term,
    ) -> (Rc<Core>, VTy, Usage) {
        let err = || (Rc::new(Core::Error), Rc::new(Value::Error), Usage::unknown());
        let pi = match &*f_ty {
            Value::Pi { grade, dom, cod, .. } => Some((grade.clone(), dom.clone(), cod.clone())),
            Value::Error => return err(),
            // the head's type is not known to be a function yet: if
            // it is still flexible, force it to be one by unifying
            // with a fresh Pi
            Value::Flex(..) => {
                let dom_c = self.fresh_meta(ctx, node, "the argument type of this function");
                let dom = self.eval(&ctx.env, &dom_c);
                let ctx_arg = ctx.bind(None, "x", dom.clone());
                let cod_c = self.fresh_meta(&ctx_arg, node, "the result type of this function");
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
                let mut a_v = self.eval(&ctx.env, &a_c);
                // A computation with effects never reaches a type: not
                // as an argument the result type mentions, and not as
                // an erased one, which is never run, so its effects
                // (not terminating, say) would never happen.
                if let Some(effects) = self.effects_of(&a_usage) {
                    let erased = self.constant(&grade) == Some(Grade::ZERO);
                    if erased || self.depends(ctx.lvl, &cod) {
                        let effects = self.quote(ctx.lvl, effects);
                        self.error(a.id, TypeErrorKind::ImpureArgument { effects, erased });
                        // what the type would have said is unknown
                        a_v = Rc::new(Value::Error);
                    }
                }
                // the function uses its argument `grade` times, so
                // everything the argument uses is used that often;
                // and calling it has its effects
                let (result, effects) = self.split_comp(self.apply(&cod, a_v));
                let usage = f_usage + a_usage.scale(&self.to_poly(grade.clone())) + Usage::effect(effects);
                (Rc::new(Core::App(f_c, self.argument(&grade, a_c))), result, usage)
            }
            None => {
                let q = self.quote(ctx.lvl, f_ty.clone());
                self.error(node, TypeErrorKind::NotAFunction(q));
                err()
            }
        }
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
                    let m = self.new_meta(boxed.id, MetaOrigin::Inferred(desc.into()), None);
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
        let ty = Some(self.quote(ctx.lvl, a.clone()));
        let unboxed = Rc::new(Core::Runtime(Note::Binds { grade: self.constant(&grade), ty }, unboxed));
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
                let ctx2 = ctx.bind(None, name, dom.clone()).graded(grade);
                let (cod_v, allowed) = self.split_comp(self.apply(cod, vvar(ctx.lvl, name.clone())));
                let (body_c, mut usage) = self.check(&ctx2, t, cod_v);
                self.close_effects(&ctx2, nid, &mut usage, allowed);
                (self.lam(ctx, name.clone(), grade, dom, body_c), usage)
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
                let ctx2 = ctx.bind(Some(x), &name, dom.clone()).graded(&allowed);
                let (cod_v, effects) = self.split_comp(self.apply(cod, vvar(ctx.lvl, name.clone())));
                let (body_c, mut usage) = self.check(&ctx2, body, cod_v);
                self.close_effects(&ctx2, nid, &mut usage, effects);
                let lam = self.lam(ctx, name, &allowed, dom, body_c);
                self.leave_binder(&ctx2, nid, x, allowed, &mut usage, false);
                (lam, usage)
            }
            // promotion: to provide `e` `grade` times, everything `e` uses
            // must be available `grade` times over
            (TermKind::BoxIntro(e), Value::BoxTy(grade, a)) => {
                let (e_c, usage) = self.check(ctx, e, a.clone());
                // a box nobody may open is never filled, so what would fill
                // it must not have effects that then never happen
                if self.constant(grade) == Some(Grade::ZERO) {
                    if let Some(effects) = self.effects_of(&usage) {
                        let effects = self.quote(ctx.lvl, effects);
                        self.error(e.id, TypeErrorKind::ImpureArgument { effects, erased: true });
                    }
                }
                // a box nobody may open need not be filled
                (Rc::new(Core::BoxIntro(self.argument(grade, e_c))), usage.scale(&self.to_poly(grade.clone())))
            }
            // Where a grade is wanted, numerals and arithmetic are grades.
            // Nothing else is overloaded: a grade *variable* is an ordinary
            // variable, and gets here by the fallback.
            (TermKind::Lit(Lit::Unit), Value::Type(_)) => (Rc::new(Core::LitTy(Builtin::Unit)), Usage::none()),
            (TermKind::Lit(Lit::Int(n)), Value::LitTy(Builtin::Grade)) => match u64::try_from(*n) {
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
                    Prim::IntSub | Prim::IntDiv | Prim::IntMod | Prim::IntLt | Prim::IntEq | Prim::StrAppend => {
                        unreachable!("not grade arithmetic")
                    }
                };
                (Rc::new(core), Usage::none())
            }
            (TermKind::Case(scrutinee, clauses), _) => {
                let (core, _, usage) = self.case_of(ctx, nid, scrutinee, clauses, Some(expected.clone()));
                (core, usage)
            }
            (TermKind::Match(clauses), _) => self.elab_match(ctx, nid, clauses, expected.clone()),
            (TermKind::Handle(body, clauses), _) => {
                let (core, _, usage) = self.handle(ctx, nid, body, clauses, Some(expected.clone()));
                (core, usage)
            }
            // a sequence is built as the type it is expected to be, if that
            // is known yet
            (TermKind::SeqNil | TermKind::SeqCons(..), _) if !matches!(*expected, Value::Flex(..)) => {
                self.check_seq(ctx, t, expected.clone())
            }
            // an array of what is expected is made of elements checked
            // against that
            (TermKind::ArrayLit(items), _) if self.array_element(&expected).is_some() => {
                let elem = self.array_element(&expected);
                let (core, _, usage) = self.array_literal(ctx, t, items, elem);
                (core, usage)
            }
            (TermKind::Omitted, _) => {
                let origin = MetaOrigin::Inferred("this".into());
                (self.fresh_meta_with(ctx, nid, origin, Some(expected.clone())), Usage::none())
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
                let (ctx2, pending, wraps) = self.enter_let(ctx, groups);
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
            // But only if it fits: a meta that must be a type in `Type`
            // cannot be that one, which is in `Type 1`, and then the term is
            // instantiated after all.
            _ => {
                let (core, actual, usage) = self.infer(ctx, t);
                // (a meta applied to something is a type to be worked out
                // from what it is applied to, not a bare unknown)
                let (core, actual) = match &*self.force(expected.clone()) {
                    Value::Flex(m, spine) if spine.is_empty() && self.fits_meta(ctx, *m, &actual) => (core, actual),
                    _ => self.insert(ctx, nid, core, actual),
                };
                if self.subtype(ctx.lvl, actual.clone(), expected.clone()) {
                    (core, usage)
                } else if self.postpone(ctx.lvl, &actual, &expected, nid) {
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
    /// `data T (a : k) .. : kind where C : ty; ..` gives the constructors'
    /// types in full instead, under the parameters: `T : (a : k) -> .. ->
    /// kind`, and `C : {a : k} -> .. -> ty`.  The kind's arguments are the
    /// type's *indices*, which each constructor can choose, and its type must
    /// end in `T`, applied to the parameters and then to indices.  Its
    /// implicit arguments come first and are erased; the rest are fields.
    ///
    /// A data type lives in `Type`, so each field must be a type in `Type`:
    /// a type with a field of type `Type` would be a set of all sets, which
    /// is a paradox.  An implicit argument may be larger only if it is
    /// *forced* — one of the indices, as the `a` of `Nil : Vec Z a` is — for
    /// then the type says what it is, and nothing is hidden in the value.
    /// And each type must be strictly positive in its own fields (see
    /// [`super::positivity`]), which is checked once every constructor has
    /// been elaborated.
    pub(super) fn declare_datas(&mut self, ctx: &Ctx, datas: &[hir::DataDecl]) -> Ctx {
        let pi = |name: &str, grade: Grade, icit: Icit, dom: Rc<Core>, cod: Rc<Core>| {
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
        // the kind of each parameter, under the parameters before it, and
        // how many indices follow them
        let mut kinds: Vec<(Vec<Rc<Core>>, usize)> = Vec::new();
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
            let (kind, indices) = match &d.kind {
                None => (Rc::new(Core::Type(0)), 0),
                Some(k) => {
                    let (k_c, k_v) = self.check_type(&under, k);
                    let (indices, end) = self.telescope_end(under.lvl, k_v);
                    match &*end {
                        Value::Type(0) | Value::Error => (k_c, indices),
                        _ => {
                            let end = self.quote(under.lvl + indices, end);
                            self.error(k.id, TypeErrorKind::DataKindNotType(end));
                            (Rc::new(Core::Type(0)), 0)
                        }
                    }
                }
            };
            let ty = d
                .params
                .iter()
                .zip(&ks)
                .rev()
                .fold(kind, |acc, ((x, _), k)| pi(&x.text, Grade::DEFAULT, Icit::Explicit, k.clone(), acc));
            self.record(d.id, ty.clone());
            let ty = self.eval(&ctx.env, &ty);
            ctx = self.declare_constant(&ctx, &d.name, ty);
            if let Some(u) = d.name.unique {
                self.datas.insert(u, d.ctors.iter().filter_map(|c| c.name.unique).collect());
            }
            kinds.push((ks, indices));
        }
        let mut shapes = Vec::new();
        for (d, (ks, indices)) in datas.iter().zip(&kinds) {
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
            let n = d.params.len();
            for (tag, c) in d.ctors.iter().enumerate() {
                let declared = match &c.shape {
                    hir::CtorShape::Fields(fields) if *indices == 0 => {
                        Some(self.ctor_fields(d, &params, c, fields, &mut shape))
                    }
                    hir::CtorShape::Fields(_) => {
                        self.error(c.id, TypeErrorKind::IndexedNeedsSignatures(d.name.text.clone()));
                        None
                    }
                    hir::CtorShape::Sig(sig) => Some(self.ctor_sig(d, &params, c, sig, *indices, &mut shape)),
                };
                let Some((ty, implicit, fields)) = declared else {
                    ctx = self.declare_constant(&ctx, &c.name, Rc::new(Value::Error));
                    continue;
                };
                // the parameters, implicit and erased
                let ty = d
                    .params
                    .iter()
                    .zip(ks)
                    .rev()
                    .fold(ty, |acc, ((x, _), k)| pi(&x.text, Grade::ZERO, Icit::Implicit, k.clone(), acc));
                let ty = self.eval(&ctx.env, &ty);
                // as the metas in its signature were solved
                let shown = self.quote(ctx.lvl, ty.clone());
                self.record(c.id, shown);
                ctx = self.declare_constant(&ctx, &c.name, ty);
                if let (Some(u), Some(data)) = (c.name.unique, d.name.unique) {
                    let info = CtorInfo { name: c.name.text.clone(), data, tag, params: n + implicit, fields };
                    self.ctors.insert(u, info);
                }
            }
            shapes.push(shape);
            self.declare_seq(d);
        }
        self.check_positivity(&shapes);
        ctx
    }

    /// How many function types a type starts with, and what it ends in.
    fn telescope_end(&self, mut lvl: Lvl, mut ty: VTy) -> (usize, VTy) {
        let mut n = 0;
        loop {
            let forced = self.force(ty);
            let Value::Pi { name, cod, .. } = &*forced else { return (n, forced) };
            ty = self.apply(cod, vvar(lvl, name.clone()));
            lvl += 1;
            n += 1;
        }
    }

    /// `C t ..`: a constructor's type under the parameters, `t -> .. -> T a
    /// ..`, and how many implicit arguments and fields it has.
    fn ctor_fields(
        &mut self,
        d: &hir::DataDecl,
        params: &Ctx,
        c: &hir::CtorDecl,
        fields: &[hir::Term],
        shape: &mut DataShape,
    ) -> (Rc<Core>, usize, usize) {
        // each field under the parameters and the (anonymous) fields before
        // it, which is where it will sit in the type
        let mut under = params.clone();
        let mut elaborated = Vec::new();
        for f in fields {
            let (f_c, f_v, level, _not_charged) = self.infer_type(&under, f);
            if level > 0 {
                self.error(f.id, TypeErrorKind::LargeField { data: d.name.text.clone(), level });
            }
            shape.fields.push(Field { node: f.id, ctor: c.name.text.clone(), lvl: under.lvl, ty: f_v.clone() });
            elaborated.push(f_c);
            under = under.bind(None, "_", f_v);
        }
        let (n, m) = (d.params.len(), elaborated.len());
        let head = Rc::new(Core::Opaque(d.name.unique.unwrap_or(usize::MAX), d.name.text.clone()));
        let result = (0..n).fold(head, |acc, j| {
            Rc::new(Core::App(acc, Rc::new(Core::Var(n + m - j - 1, d.params[j].0.text.clone()))))
        });
        let ty = elaborated.into_iter().rev().fold(result, |acc, f| {
            Rc::new(Core::Pi {
                name: "_".into(),
                icit: Icit::Explicit,
                // linear in a linear module, and `[]` elsewhere, as a field
                // of a constructor's signature is
                grade: Core::grade(if d.linear { Grade::ONE } else { Grade::DEFAULT }),
                in_type: Core::grade(Grade::DEFAULT_IN_TYPE),
                dom: f,
                cod: acc,
            })
        });
        (ty, 0, m)
    }

    /// `C : ty`: the constructor's type under the parameters, checked to be
    /// one, and how many implicit arguments and fields it has.
    fn ctor_sig(
        &mut self,
        d: &hir::DataDecl,
        params: &Ctx,
        c: &hir::CtorDecl,
        sig: &hir::Term,
        indices: usize,
        shape: &mut DataShape,
    ) -> (Rc<Core>, usize, usize) {
        let (sig_c, sig_v) = self.check_type(params, sig);
        // the written domains, for errors about one argument
        let mut nodes = Vec::new();
        let mut t = sig;
        while let TermKind::Pi(_, _, _, dom, cod) = &t.kind {
            nodes.push(dom.id);
            t = cod;
        }
        let mut types = params.types();
        let mut binders = Vec::new();
        let mut ty = sig_v;
        loop {
            let forced = self.force(ty);
            let Value::Pi { name, icit, grade, dom, cod, .. } = &*forced else {
                ty = forced;
                break;
            };
            let lvl = types.len();
            binders.push((lvl, *icit, grade.clone(), dom.clone(), types.clone()));
            types.push((name.clone(), dom.clone()));
            ty = self.apply(cod, vvar(lvl, name.clone()));
        }
        // the result: this type, at its parameters, then at any indices
        let n = d.params.len();
        let at_params = |sp: &[Elim]| {
            sp.iter().take(n).enumerate().all(|(j, e)| match e {
                Elim::App(a) => matches!(&**a, Value::Rigid(l, _, s) if *l == shape.params_at + j && s.is_empty()),
                _ => false,
            })
        };
        let forced: Vec<Lvl> = match &*ty {
            Value::Opaque(u, _, sp) if Some(*u) == d.name.unique && sp.len() == n + indices && at_params(sp) => sp[n..]
                .iter()
                .filter_map(|e| match e {
                    Elim::App(a) => match &**a {
                        Value::Rigid(l, _, s) if s.is_empty() => Some(*l),
                        _ => None,
                    },
                    _ => None,
                })
                .collect(),
            Value::Error => Vec::new(),
            _ => {
                let kind = TypeErrorKind::CtorResult { ctor: c.name.text.clone(), data: d.name.text.clone() };
                self.error(sig.id, kind);
                Vec::new()
            }
        };
        let implicit = binders.iter().take_while(|b| b.1 == Icit::Implicit).count();
        let erased = binders[..implicit].iter().all(|b| self.constant(&b.2) == Some(Grade::ZERO));
        if !erased || binders[implicit..].iter().any(|b| b.1 == Icit::Implicit) {
            self.error(sig.id, TypeErrorKind::CtorImplicit(c.name.text.clone()));
        }
        for (i, (lvl, icit, _, dom, mut types)) in binders.iter().cloned().enumerate() {
            let node = nodes.get(i).copied().unwrap_or(sig.id);
            let level = self.sort_of(&mut types, dom.clone());
            if let Some(level) = level.filter(|l| *l > 0) {
                if icit == Icit::Explicit || !forced.contains(&lvl) {
                    self.error(node, TypeErrorKind::LargeField { data: d.name.text.clone(), level });
                }
            }
            shape.fields.push(Field { node, ctor: c.name.text.clone(), lvl, ty: dom });
        }
        (sig_c, implicit, binders.len() - implicit)
    }
}
