//! Linear types: types whose values are never shared, so that changing one
//! in place is never seen. `MArray` is one (a native, [`Native::is_linear_type`]),
//! and so is a data type with a field of a linear type, or applied to one
//! (`Pair (Int []) (MArray a)`).
//!
//! A grade says how often a binder may be used, but a binder's grade is the
//! module's to choose (`[]` where it says nothing, outside `@linear`), and
//! a value is not its binder. So what is linear is kept so by its type:
//!
//! - a variable of a linear type is used at most once, whatever its grade
//!   says ([`Checker::linear_usage`], from `leave_key`);
//! - a function type's binder of a linear type that says no grade is `[1]`,
//!   whatever the module's default ([`Checker::linear_default`]), so that a
//!   constructor's field or a parameter does not charge what is given for
//!   it without limit;
//! - a value of a linear type is never boxed, `[e]`, to be used more often;
//! - a linear type is given for a type parameter only of something that
//!   holds values of that type at most once: `Pair`, whose fields are
//!   linear, but not a function taking `(x : [] a)`, which could share it
//!   ([`Checker::note_linear_want`], checked when the module is);
//! - and no top-level definition is of a linear type, every use of it
//!   being the same value.

use super::value::{Elim, VTy, Value};
use super::elab::Pending;
use idyllic_hir as hir;
use super::{Checker, Ctx, Lvl, TypeErrorKind, Usage};
use crate::grade::Grade;
use crate::Poly;
use idyllic_ast::Icit;
use idyllic_hir::NodeId;
use std::rc::Rc;

/// A type parameter given a type that may be linear, to something that may
/// hold a value of it more than once: checked once the type is known.
#[derive(Clone, Debug)]
pub(super) struct LinearWant {
    pub node: NodeId,
    pub lvl: Lvl,
    pub given: Rc<Value>,
    pub param: String,
}

/// `[0..1]`: at most once.
fn at_most_once() -> Grade {
    Grade::range(Grade::ZERO, Grade::ONE).expect("not empty")
}

impl Checker {
    /// Whether values of `ty` are linear.
    pub(super) fn is_linear(&mut self, ty: &VTy) -> bool {
        match &*self.force(ty.clone()) {
            Value::Opaque(u, _, spine) => {
                if self.natives.get(u).is_some_and(|n| n.native.is_linear_type()) {
                    return true;
                }
                if !self.datas.contains_key(u) {
                    return false;
                }
                let spine = spine.clone();
                self.data_is_linear(*u) || spine.iter().any(|e| matches!(e, Elim::App(a) if self.is_linear(a)))
            }
            Value::BoxTy(_, inner) | Value::Comp(inner, _) => {
                let inner = inner.clone();
                self.is_linear(&inner)
            }
            _ => false,
        }
    }

    /// Whether a data type has a field of a linear type, its parameters
    /// aside: worked out once.
    fn data_is_linear(&mut self, u: usize) -> bool {
        if let Some(known) = self.linear_datas.get(&u) {
            return *known;
        }
        // (a data type that holds itself is not linear for that)
        self.linear_datas.insert(u, false);
        let ctors = self.datas.get(&u).cloned().unwrap_or_default();
        let mut linear = false;
        'ctors: for c in ctors {
            let Some(mut ty) = self.constants.get(&c).cloned() else { continue };
            let mut l = 0;
            loop {
                let Value::Pi { name, icit, dom, cod, .. } = &*self.force(ty.clone()) else { break };
                if *icit == Icit::Explicit && self.is_linear(dom) {
                    linear = true;
                    break 'ctors;
                }
                ty = self.apply(cod, super::value::vvar(l, name.clone()));
                l += 1;
            }
        }
        self.linear_datas.insert(u, linear);
        linear
    }

    /// Whether `ty` holds a value of the type variable at `a`: is it, or
    /// has it among a data type's arguments or in a box, but not under a
    /// function type (a function of one is no value of one).
    fn holds(&self, ty: &VTy, a: Lvl) -> bool {
        match &*self.force(ty.clone()) {
            Value::Rigid(l, _, spine) => *l == a && spine.is_empty(),
            Value::Opaque(_, _, spine) => spine.iter().any(|e| matches!(e, Elim::App(x) if self.holds(x, a))),
            Value::BoxTy(_, inner) | Value::Comp(inner, _) => self.holds(inner, a),
            _ => false,
        }
    }

    /// Whether a function type, after its implicit type parameter `name`
    /// bound at `lvl` (its body `cod`), takes a value holding one of that
    /// type at a grade that allows more than one use.
    fn shares(&self, lvl: Lvl, name: &str, cod: &super::value::Closure) -> bool {
        let a = lvl;
        let mut ty = self.apply(cod, super::value::vvar(lvl, name.to_string()));
        let mut l = lvl + 1;
        loop {
            let forced = self.force(ty.clone());
            let Value::Pi { name, grade, dom, cod, .. } = &*forced else { return false };
            let once = self.constant(grade).is_some_and(|g| g.fits(at_most_once()));
            if !once && self.holds(dom, a) {
                return true;
            }
            ty = self.split_comp(self.apply(cod, super::value::vvar(l, name.clone()))).0;
            l += 1;
        }
    }

    /// An implicit argument `given` inserted for the parameter `name` of a
    /// function type whose body is `cod`, a type: if the function may share
    /// a value of that type, the argument is to be no linear type, which is
    /// checked once what it is is known.
    pub(super) fn note_linear_want(&mut self, ctx: &Ctx, node: NodeId, name: &str, dom: &VTy, cod: &super::value::Closure, given: Rc<Value>) {
        if !matches!(*self.force(dom.clone()), Value::Type(_)) || !self.shares(ctx.lvl, name, cod) {
            return;
        }
        self.linear_wants.push(LinearWant { node, lvl: ctx.lvl, given, param: name.to_string() });
    }

    /// The type parameters given what may be linear types: each that is one
    /// is an error.
    pub(super) fn check_linear_wants(&mut self) {
        for w in std::mem::take(&mut self.linear_wants) {
            if self.is_linear(&w.given) {
                let ty = self.quote(w.lvl, w.given.clone());
                self.error(w.node, TypeErrorKind::LinearInstance { ty, param: w.param });
            }
        }
    }

    /// The variables a `let` bound, from level `from` of `ctx` on, used in
    /// its body as `usage` says: those of a linear type at most once. (A
    /// `let`'s uses are charged to its right-hand side, its binder having
    /// no grade of its own.)
    pub(super) fn linear_lets(&mut self, ctx: &Ctx, from: Lvl, node: NodeId, usage: &Usage, pending: &[Pending]) {
        if usage.is_unknown() {
            return;
        }
        for l in from..ctx.lvl {
            let b = &ctx.binders[l];
            let Some(key) = b.key else { continue };
            if !pending.iter().any(|p| matches!(p, Pending::NonRec { unique: Some(u), .. } if *u == key)) {
                continue;
            }
            let (name, ty) = (b.name.clone(), b.ty.clone());
            let used = usage.clone().take(Some(key));
            self.linear_usage(ctx, node, &name, &ty, &used);
        }
    }

    /// A module's top-level definitions of a linear type. In a program (a
    /// module with an expression, or a `main`, which `uses` is the uses of),
    /// each is used at most once: what a definition uses is used once per
    /// use of it, as a `let`'s right-hand side is. Elsewhere, any module may
    /// use one, as often as it likes, so there is to be none.
    pub(super) fn linear_top_level(&mut self, ctx: &Ctx, groups: &[hir::BindGroup], owed: &[Pending], uses: Option<Usage>) {
        let Some(mut usage) = uses else {
            for b in groups.iter().filter_map(|g| if let hir::BindGroup::NonRec(b) = g { Some(b) } else { None }) {
                let Some((_, ty, _)) = ctx.lookup(&b.name) else { continue };
                if self.is_linear(&ty) {
                    let ty = self.quote(ctx.lvl, ty);
                    self.error(b.id, TypeErrorKind::LinearTopLevel { name: b.name.text.clone(), ty });
                }
            }
            return;
        };
        for (g, p) in groups.iter().zip(owed).rev() {
            if usage.is_unknown() {
                return;
            }
            match p {
                Pending::NonRec { unique, rhs } => {
                    let n = usage.take(*unique);
                    if let hir::BindGroup::NonRec(b) = g {
                        if let Some((_, ty, _)) = ctx.lookup(&b.name) {
                            self.linear_usage(ctx, b.id, &b.name.text, &ty, &n);
                        }
                    }
                    usage = usage + rhs.clone().scale(&n);
                }
                Pending::Rec { uniques, rhs } => {
                    let mut rhs = rhs.clone();
                    let mut used = false;
                    for u in uniques {
                        used |= !usage.take(*u).is_zero();
                        rhs.take(*u);
                    }
                    usage = usage + rhs.scale(&Poly::constant(if used { Grade::ANY } else { Grade::ZERO }));
                }
            }
        }
    }

    /// A value of type `ty` boxed, `[e]`: no linear type may be.
    pub(super) fn linear_box(&mut self, ctx: &Ctx, node: NodeId, ty: &VTy) {
        if self.is_linear(ty) {
            let ty = self.quote(ctx.lvl, ty.clone());
            self.error(node, TypeErrorKind::LinearBox(ty));
        }
    }

    /// A variable of a linear type `ty`, used as `used` says: at most once,
    /// or the error is reported (`true`), or waits on a grade being worked
    /// out.
    pub(super) fn linear_usage(&mut self, ctx: &Ctx, node: NodeId, name: &str, ty: &VTy, used: &Poly) -> bool {
        if !self.is_linear(ty) {
            return false;
        }
        let allowed = Poly::constant(at_most_once()).into_value();
        if self.fits_or_waits(ctx, node, name, used, allowed, None) {
            return false;
        }
        let used = self.quote(ctx.lvl, used.clone().into_value());
        let ty = self.quote(ctx.lvl, ty.clone());
        self.error(node, TypeErrorKind::LinearUsage { name: name.to_string(), ty, used });
        true
    }
}
