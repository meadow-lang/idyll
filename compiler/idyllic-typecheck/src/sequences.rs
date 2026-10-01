//! Sequence literals, `#[a, b, c]`.
//!
//! A literal is built as whatever sequence type is expected: a data type
//! declared with `@seq(nil = Nil, cons = (::))` gets its own constructors,
//! `a :: b :: c :: Nil`.  So an indexed family gets its length in its type
//! for nothing — `#[1, 2] : Vec (S (S Z)) Int` — and the same form is a
//! pattern, matched with the same constructors.  With nothing to say which
//! type, a sequence is a `List`, the prelude's.
//!
//! The renamer has already taken a literal apart into a `SeqCons` for each
//! element and a `SeqNil` at the end, so building one is applying `cons` to
//! an element and the rest, which is checked against `cons`'s second field:
//! the same type again, and so the same constructors.

use super::core::Core;
use super::value::{VTy, Value};
use super::{Checker, Ctx, TypeErrorKind, Usage};
use idyllic_hir::{self as hir, NodeId, TermKind};
use std::rc::Rc;

impl Checker {
    /// Take note of a data type's `@seq`, once its constructors are declared:
    /// `nil` has to be one of them with no fields, and `cons` one with two,
    /// an element and the rest.
    pub(super) fn declare_seq(&mut self, d: &hir::DataDecl) {
        let (Some((nil, cons)), Some(data)) = (&d.seq, d.name.unique) else { return };
        // an unknown name, which the renamer reported
        let (Some(n), Some(c)) = (nil.unique, cons.unique) else { return };
        let fits = |x: &hir::Ident, u: usize, fields: usize, what: &str| match self.ctors.get(&u) {
            Some(info) if info.data == data && info.fields == fields => Ok(()),
            Some(info) if info.data == data => {
                Err(format!("{} is {what}, so it has {fields} fields; {} has {}", x.text, x.text, info.fields))
            }
            _ => Err(format!("{} is not a constructor of {}", x.text, d.name.text)),
        };
        let checked = fits(nil, n, 0, "the empty sequence")
            .and_then(|()| fits(cons, c, 2, "an element in front of a sequence"));
        match checked {
            Ok(()) => {
                self.seqs.insert(data, (n, c));
            }
            Err(why) => self.error(d.id, TypeErrorKind::BadSeq { data: d.name.text.clone(), why }),
        }
    }

    /// The `nil` and `cons` of a type a literal can build.
    pub(super) fn seq_ctors(&self, ty: &VTy) -> Option<(usize, usize)> {
        match &*self.force(ty.clone()) {
            Value::Opaque(d, _, _) => self.seqs.get(d).copied(),
            _ => None,
        }
    }

    /// A literal checked against `expected`, which is not a metavariable.
    pub(super) fn check_seq(&mut self, ctx: &Ctx, t: &hir::Term, expected: VTy) -> (Rc<Core>, Usage) {
        let Some(ctors) = self.seq_ctors(&expected) else {
            let ty = self.quote(ctx.lvl, expected);
            self.error(t.id, TypeErrorKind::NotASequence(ty));
            return (Rc::new(Core::Error), Usage::unknown());
        };
        let (core, actual, usage) = self.build_seq(ctx, t, ctors, Some(&expected));
        let (core, actual) = self.insert(ctx, t.id, core, actual);
        if self.subtype(ctx.lvl, actual.clone(), expected.clone()) {
            return (core, usage);
        }
        let (expected, actual) = (self.quote(ctx.lvl, expected), self.quote(ctx.lvl, actual));
        self.error(t.id, TypeErrorKind::Mismatch { expected, actual });
        (Rc::new(Core::Error), usage)
    }

    /// A literal with nothing to say what it is: a `List`.
    pub(super) fn infer_seq(&mut self, ctx: &Ctx, t: &hir::Term) -> (Rc<Core>, VTy, Usage) {
        match self.default_seq() {
            Some(ctors) => self.build_seq(ctx, t, ctors, None),
            // (only if the prelude has no `List`)
            None => (Rc::new(Core::Error), Rc::new(Value::Error), Usage::unknown()),
        }
    }

    fn default_seq(&self) -> Option<(usize, usize)> {
        self.prelude.get("List").and_then(|d| self.seqs.get(d)).copied()
    }

    /// `#[]` is `nil`, and `#[a, ..]` is `cons a #[..]`.  With an expected
    /// type, `cons`'s result is made that type before its arguments are
    /// checked, so that they are checked against what they must be: an
    /// element that is a polymorphic function, say, is instantiated.
    fn build_seq(
        &mut self,
        ctx: &Ctx,
        t: &hir::Term,
        (nil, cons): (usize, usize),
        expected: Option<&VTy>,
    ) -> (Rc<Core>, VTy, Usage) {
        let TermKind::SeqCons(head, rest) = &t.kind else { return self.seq_ctor(ctx, nil) };
        let (f_c, f_ty, f_usage) = self.seq_ctor(ctx, cons);
        let (f_c, f_ty) = self.insert(ctx, t.id, f_c, f_ty);
        if let Some(expected) = expected {
            self.expect_result(ctx, f_ty.clone(), 2, expected.clone());
        }
        let partial = self.apply_to(ctx, t.id, (f_c, f_ty, f_usage), head);
        self.apply_to(ctx, t.id, partial, rest)
    }

    /// A constructor, as a term.
    fn seq_ctor(&mut self, ctx: &Ctx, c: usize) -> (Rc<Core>, VTy, Usage) {
        let x = hir::Ident { unique: Some(c), text: self.ctors[&c].name.clone() };
        match ctx.lookup(&x) {
            Some((core, ty, key)) => (core, ty, Usage::var(key)),
            None => (Rc::new(Core::Error), Rc::new(Value::Error), Usage::unknown()),
        }
    }

    /// Unify the result of a function of `arity` arguments with `expected`,
    /// if the result does not depend on the arguments.  A failure is left
    /// for checking the whole to report.
    fn expect_result(&mut self, ctx: &Ctx, mut ty: VTy, arity: usize, expected: VTy) {
        for _ in 0..arity {
            let forced = self.force(ty);
            let Value::Pi { cod, .. } = &*forced else { return };
            if self.depends(ctx.lvl, cod) {
                return;
            }
            ty = self.split_comp(self.apply(cod, Rc::new(Value::Error))).0;
        }
        self.unify(ctx.lvl, ty, expected);
    }

    /// The constructors a sequence pattern is matched with, for what it is
    /// matched against, of type `ty`.  With nothing known about that yet, it
    /// is a `List`.
    pub(super) fn seq_pattern_ctors(&mut self, ctx: &Ctx, node: NodeId, ty: VTy) -> Option<(usize, usize)> {
        let ty = self.force(ty);
        if !matches!(*ty, Value::Flex(..)) {
            return self.seq_ctors(&ty);
        }
        let ctors = self.default_seq()?;
        let list = *self.prelude.get("List")?;
        // `List ?a`, a metavariable for each parameter
        let mut kind = self.constants.get(&list)?.clone();
        let mut core = Rc::new(Core::Opaque(list, "List".into()));
        while let Value::Pi { cod, .. } = &*self.force(kind.clone()) {
            let param = self.fresh_meta(ctx, node, "the type of the elements");
            let v = self.eval(&ctx.env, &param);
            core = Rc::new(Core::App(core, param));
            kind = self.apply(cod, v);
        }
        let list_ty = self.eval(&ctx.env, &core);
        self.unify(ctx.lvl, ty, list_ty).then_some(ctors)
    }
}
