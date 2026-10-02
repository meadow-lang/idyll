//! Natives: `@native f : a`, a type or an operation that every back end
//! implements itself (see `idyllic_builtins::Native`), declared in Idyll with
//! its type.  The prelude declares `Array` and its operations this way, and
//! `Std.Collections.MArray` the mutable arrays.
//!
//! To the type theory a native is a constant, `Core::Opaque`, that never
//! computes: `arrayLen #[1; 2]` is `arrayLen #[1; 2]` in a type, not `2`.
//! What it is at run time is the back ends' business, which they look up in
//! [`NativeInfo`] by the constant's unique.
//!
//! A native type (`Array : Type -> Type`) is declared before the data types
//! and effects, which may mention it; a native operation after them, since
//! its type may mention theirs (`arrayToList : .. -> List a`).
//!
//! Also here: array literals, `#[a; b]`, which are `arrayFromList` applied to
//! the list of their elements.

use super::core::Core;
use super::value::{VTy, Value};
use super::{Checker, Ctx, TypeErrorKind, Usage};
use idyllic_ast::Icit;
use idyllic_builtins::Native;
use idyllic_hir as hir;
use std::rc::Rc;

/// What is known about a native, for whoever runs it.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct NativeInfo {
    pub native: Native,
    /// how many erased (implicit) arguments come before the ones it takes
    /// at run time, of which there are `native.arity()`
    pub params: usize,
}

impl Checker {
    /// Declare the natives of one kind: the types, or the operations.  A
    /// name that is no native is reported with the operations.
    pub(super) fn declare_natives(&mut self, ctx: &Ctx, natives: &[hir::NativeDecl], types: bool) -> Ctx {
        let mut ctx = ctx.clone();
        for n in natives {
            let Some(native) = Native::from_name(&n.name.text) else {
                if !types {
                    self.error(n.id, TypeErrorKind::UnknownNative(n.name.text.clone()));
                    ctx = self.declare_constant(&ctx, &n.name, Rc::new(Value::Error));
                }
                continue;
            };
            if native.is_type() != types {
                continue;
            }
            let (sig, _) = self.check_type(&ctx, &n.ty);
            let (params, arity) = shape(&sig);
            let fits = match native.is_type() {
                // a type constructor: its kind ends in a universe
                true => ends_in_universe(&sig),
                false => arity == native.arity() && !ends_in_effects(&sig),
            };
            if !fits && !matches!(*sig, Core::Error) {
                let why = if native.is_type() {
                    "a native type's kind ends in Type, as in Type -> Type".to_string()
                } else {
                    let n = native.arity();
                    let s = if n == 1 { "" } else { "s" };
                    format!("it takes {n} argument{s} at run time, after any implicit ones, and has no effects")
                };
                self.error(n.id, TypeErrorKind::NativeSignature(n.name.text.clone(), why));
            }
            let ty = self.eval(&ctx.env, &sig);
            let shown = self.quote(ctx.lvl, ty.clone());
            self.record(n.id, shown);
            ctx = self.declare_constant(&ctx, &n.name, ty);
            if let Some(u) = n.name.unique {
                self.natives.insert(u, NativeInfo { native, params });
                // an array holds its elements, and so mentions them only
                // positively: a data type may have itself in one
                if native.is_type() {
                    self.positive_params.insert(u, vec![true; arity]);
                }
            }
        }
        ctx
    }

    /// The unique of the native `n`, if something declares it.
    pub(super) fn native_unique(&self, n: Native) -> Option<usize> {
        self.natives.iter().find(|(_, info)| info.native == n).map(|(u, _)| *u)
    }

    /// `#[a; b; c]`: its elements checked against `elem` (or a fresh
    /// metavariable for their type), and the array made from them.
    pub(super) fn array_literal(
        &mut self,
        ctx: &Ctx,
        t: &hir::Term,
        items: &[hir::Term],
        elem: Option<VTy>,
    ) -> (Rc<Core>, VTy, Usage) {
        let failed = (Rc::new(Core::Error), Rc::new(Value::Error), Usage::unknown());
        let (Some(array), Some(from_list)) = (self.native_unique(Native::Array), self.native_unique(Native::ArrayFromList))
        else {
            self.error(t.id, TypeErrorKind::UnknownNative("Array".into()));
            return failed;
        };
        let Some((nil, cons)) = self.prelude.get("List").and_then(|d| self.seqs.get(d)).copied() else { return failed };
        let elem = match elem {
            Some(elem) => elem,
            None => {
                let m = self.fresh_meta(ctx, t.id, "the type of the array's elements");
                self.eval(&ctx.env, &m)
            }
        };
        let elem_c = self.quote(ctx.lvl, elem.clone());
        let mut usage = Usage::none();
        let mut cores = Vec::new();
        for item in items {
            let (c, u) = self.check(ctx, item, elem.clone());
            cores.push(c);
            usage = usage + u;
        }
        let app = |f: Rc<Core>, a: Rc<Core>| Rc::new(Core::App(f, a));
        let opaque = |u: usize, name: &str| Rc::new(Core::Opaque(u, name.into()));
        let empty = app(opaque(nil, "Nil"), elem_c.clone());
        let list = cores.into_iter().rev().fold(empty, |rest, x| {
            app(app(app(opaque(cons, "::"), elem_c.clone()), x), rest)
        });
        let core = app(app(opaque(from_list, Native::ArrayFromList.name()), elem_c.clone()), list);
        let ty = self.eval(&ctx.env, &app(opaque(array, "Array"), elem_c));
        (core, ty, usage)
    }

    /// The element type of an `Array a`, if `ty` is one.
    pub(super) fn array_element(&self, ty: &VTy) -> Option<VTy> {
        let array = self.native_unique(Native::Array)?;
        match &*self.force(ty.clone()) {
            Value::Opaque(u, _, sp) if *u == array && sp.len() == 1 => match &sp[0] {
                super::value::Elim::App(a) => Some(a.clone()),
                _ => None,
            },
            _ => None,
        }
    }
}

/// A native's type: how many implicit binders it starts with, and how many
/// explicit ones follow them.
fn shape(c: &Rc<Core>) -> (usize, usize) {
    let mut params = 0;
    let mut arity = 0;
    let mut at = c;
    while let Core::Pi { icit, cod, .. } = &**at {
        match icit {
            Icit::Implicit if arity == 0 => params += 1,
            _ => arity += 1,
        }
        at = cod;
    }
    (params, arity)
}

fn result_of(c: &Rc<Core>) -> &Rc<Core> {
    let mut at = c;
    while let Core::Pi { cod, .. } = &**at {
        at = cod;
    }
    at
}

fn ends_in_universe(c: &Rc<Core>) -> bool {
    matches!(**result_of(c), Core::Type(_))
}

fn ends_in_effects(c: &Rc<Core>) -> bool {
    let mut at = c;
    while let Core::Pi { cod, .. } = &**at {
        if matches!(**cod, Core::Comp(..)) {
            return true;
        }
        at = cod;
    }
    matches!(**at, Core::Comp(..))
}
