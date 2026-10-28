//! **Number literals take the type wanted**, as Meadow's do: `5` is an
//! `Int8` where an `Int8` is wanted, and `1.5` a `Float32` where a `Float32`
//! is; with nothing to say, an integer is an `Int` and a number with a point
//! a `Float`.
//!
//! What is wanted may not be known yet where the literal is: in `1 + x`, the
//! `1` is checked before `x` says what `+` is at. So a literal checked
//! against a type still to be worked out waits behind a placeholder, as an
//! auto argument does (see [`auto`](super::auto)), and is read once its type
//! is known, after the application it is in or at the end of its
//! definition, where an unknown type is taken to be the default.

use super::*;
use idyllic_ast::Lit;
use idyllic_builtins::NumTy;

/// A number literal whose type was not known yet.
#[derive(Clone)]
pub(super) struct DeferredLiteral {
    meta: MetaVar,
    node: NodeId,
    ctx: Ctx,
    ty: VTy,
    lit: Lit,
}

/// Whether `lit`, as the parser read it, can be read at the builtin `b`:
/// an integer at any integer type, a number with a point at a float type.
pub(super) fn reads_as(lit: &Lit, b: Builtin) -> bool {
    match (lit, b.number()) {
        (Lit::Int(_), Some(n)) => !n.is_float(),
        (Lit::Float(_), Some(n)) => n.is_float(),
        _ => false,
    }
}

/// Whether a literal as written, `5`, is the number `read` is, `5` at
/// `Int8`.
pub(super) fn same_number(written: &Lit, read: &Lit) -> bool {
    match (written, read) {
        (Lit::Int(n), Lit::Word(w, b)) => w.value(*b) == i128::from(*n),
        (Lit::Float(x), Lit::Float32(y)) => (f64::from_bits(*x) as f32).to_bits() == *y,
        _ => written == read,
    }
}

/// What `lit` is with nothing to say otherwise.
fn default_of(lit: &Lit) -> Builtin {
    match lit {
        Lit::Float(_) => Builtin::Float,
        _ => Builtin::Int,
    }
}

impl Checker {
    /// `lit` read at `b`, which [`reads_as`] says it can be; an integer too
    /// big for its type is reported, and wrapped.
    pub(super) fn read_number(&mut self, node: NodeId, lit: &Lit, b: Builtin) -> Lit {
        match (lit, b.number()) {
            (Lit::Int(n), Some(NumTy::Word(w))) => {
                let bits = w.wrap(i128::from(*n));
                if w.value(bits) != i128::from(*n) {
                    self.error(node, TypeErrorKind::NumberTooLarge { number: n.to_string(), ty: b.name().to_string() });
                }
                Lit::Word(w, bits)
            }
            (Lit::Float(x), Some(NumTy::Float32)) => Lit::Float32((f64::from_bits(*x) as f32).to_bits()),
            _ => lit.clone(),
        }
    }

    /// A number literal checked against `ty`, a type still to be worked
    /// out: a placeholder, read once it is known.
    pub(super) fn defer_literal(&mut self, ctx: &Ctx, node: NodeId, lit: &Lit, ty: VTy) -> Rc<Core> {
        let meta = self.new_meta(node, MetaOrigin::Inferred("a number".into()), None);
        self.placeholders.insert(meta);
        self.deferred_literals.push(DeferredLiteral { meta, node, ctx: ctx.clone(), ty, lit: lit.clone() });
        Rc::new(Core::Meta(meta))
    }

    /// The literals that waited for their types, read where their types are
    /// known now; with `last`, every one, an unknown type taken to be the
    /// default. Whether any was read.
    pub(super) fn settle_literals(&mut self, last: bool) -> bool {
        let mut progress = false;
        for d in std::mem::take(&mut self.deferred_literals) {
            let ty = self.force(d.ty.clone());
            let core = match &*ty {
                Value::LitTy(b) if reads_as(&d.lit, *b) => Rc::new(Core::Lit(self.read_number(d.node, &d.lit, *b))),
                _ if !last && self.blocked(&ty) => {
                    self.deferred_literals.push(d);
                    continue;
                }
                _ => {
                    let default = default_of(&d.lit);
                    if self.unify(d.ctx.lvl, ty.clone(), Rc::new(Value::LitTy(default))) {
                        Rc::new(Core::Lit(d.lit.clone()))
                    } else {
                        let expected = self.quote(d.ctx.lvl, ty);
                        self.error(d.node, TypeErrorKind::Mismatch { expected, actual: Rc::new(Core::LitTy(default)) });
                        Rc::new(Core::Error)
                    }
                }
            };
            let value = self.eval(&d.ctx.env, &core);
            if let MetaEntry::Unsolved(info) = &self.metas[d.meta] {
                let info = info.clone();
                self.metas[d.meta] = MetaEntry::Solved(value, info);
            }
            self.chosen.insert(d.meta, (core, d.ctx.lvl));
            progress = true;
        }
        progress
    }
}
