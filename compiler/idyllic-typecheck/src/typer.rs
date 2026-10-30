//! The types of an elaborated program after checking, for whoever lowers it.
//!
//! A back end typed by representations (the shared back end's Cut) needs to
//! know, for every value the program computes, how it is represented at run
//! time: a 64-bit integer, a pointer, a value whose representation a type
//! variable stands for.  That is a question about the value's type, which
//! the checker knew and the elaborated program does not say.  So the
//! [`Typer`] keeps the checker, and works the type of any term out again,
//! by normalisation by evaluation as checking did: from the types the
//! elaborator noted on binders ([`Note::Binds`]), the constants' types, and
//! the constructors' types, whose fields it instantiates as matching does.
//! [`Typer::rep`] then reads a representation off a type.
//!
//! A term it cannot type (a solved hole, which carries no type of its own)
//! is `None`, and a value of a type it cannot read a representation from is
//! [`Rep::Any`]: boxed, and self-describing at run time.

use super::core::{lit_builtin, CaseAlt, Core, Head, Lvl, Note};
use super::value::{vvar, Closure, Elim, Poly, Value};
use super::{BinderInfo, Checker, Ctx, MetaEntry};
use crate::grade::Grade;
use idyllic_builtins::{Builtin, Native};
use idyllic_hir::NodeId;
use std::rc::Rc;

/// How a value is represented at run time, as the shared back end's IRs
/// have it (`docs/CUT.md` in the Meadow repository).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rep {
    /// a 64-bit integer, untagged: `Int`
    I64,
    /// a sized integer, at its width
    Word(idyllic_builtins::Width),
    /// a `Float`, and a `Float32`
    F64,
    F32,
    Bool,
    Unit,
    /// an immutable string
    Str,
    /// a character, a Unicode scalar value: on Idyll's own back ends its code
    /// point, as an `I64` is
    Char,
    /// anything else on the heap: data, a function, a box
    Ptr,
    /// whatever the type variable bound at this level of the context is,
    /// which a descriptor says at run time
    Var(Lvl),
    /// whatever the type is that a field of the record bound at this level
    /// holds (the field by its place among a `case` alternative's
    /// variables): an associated type, `Elem c` at a dictionary `d`, which
    /// a descriptor passed with `d` says
    Field(Lvl, usize),
    /// a type held at run time, as a record's field that is a type holds
    /// one: its descriptor
    Desc,
    /// not known when compiled: boxed, and self-describing
    Any,
    /// a type, a grade or an effect: nothing at run time
    Erased,
}

/// What a value of a type looks like written out, as the REPL shows one:
/// what a back end that prints a program's answer must write.
#[derive(Clone, Debug)]
pub enum Shown {
    Int,
    Bool,
    Unit,
    Str,
    /// `'a'`, from its code point
    Char,
    /// a sized integer or a float, as Meadow's `display` writes it
    Number,
    /// a box, `[v]`, of what is inside
    Box(Rc<Value>),
    /// an array, `#[a; b]`, of these
    Array(Rc<Value>),
    /// data: the type, written out (which names what shows it), and each
    /// constructor that can make a value of it, with its fields' types
    Data { key: String, ctors: Vec<ShownCtor> },
    /// a function, a type, or data whose fields' types depend on what is
    /// in them: nothing the REPL would show
    Nothing,
}

/// A constructor that can make a value of a type, with its fields' types
/// there.
#[derive(Clone, Debug)]
pub struct ShownCtor {
    pub unique: usize,
    pub name: String,
    pub fields: Vec<Rc<Value>>,
}

/// A program's types after checking: see the module docs.
pub struct Typer {
    chk: Checker,
}

/// The variables in scope at a point of the program, with their types (and
/// the values of definitions, which types may compute with).
#[derive(Clone)]
pub struct Scope {
    ctx: Ctx,
}

impl Scope {
    /// How many variables are in scope.
    pub fn len(&self) -> usize {
        self.ctx.lvl
    }

    pub fn is_empty(&self) -> bool {
        self.ctx.lvl == 0
    }

    /// The scope as it was when it had `lvl` variables: where something
    /// defined with that many in scope was defined.
    pub fn prefix(&self, lvl: usize) -> Scope {
        Scope { ctx: self.ctx.prefix(lvl) }
    }
}

impl Typer {
    pub(super) fn new(chk: Checker) -> Typer {
        Typer { chk }
    }

    /// Know what other packages give too: a program linked from them refers
    /// to their constants.
    pub fn import(&mut self, deps: &[&super::packages::CompiledPackage]) {
        let ctx = Ctx::initial(&self.chk);
        let _ = self.chk.import(&ctx, deps);
    }

    /// Whether the constant `u` has a type here.
    pub fn knows(&self, u: usize) -> bool {
        self.chk.constants.contains_key(&u)
    }

    /// Where an elaborated program starts: the builtins, then the
    /// primitives.
    pub fn scope(&self) -> Scope {
        Scope { ctx: Ctx::initial(&self.chk) }
    }

    /// `s` with a variable of type `ty` (a term in `s`) bound.
    pub fn bind(&mut self, s: &Scope, name: &str, ty: &Rc<Core>) -> Scope {
        let ty = self.chk.eval(&s.ctx.env, ty);
        Scope { ctx: s.ctx.bind(None, name, ty) }
    }

    /// `s` with a variable bound whose type is not known: what a binder the
    /// elaborator noted nothing on gets.
    pub fn bind_unknown(&mut self, s: &Scope, name: &str) -> Scope {
        Scope { ctx: s.ctx.bind(None, name, Rc::new(Value::Error)) }
    }

    /// `s` with `name` defined as `rhs`, a `let`.
    pub fn define(&mut self, s: &Scope, name: &str, rhs: &Rc<Core>) -> Scope {
        let ty = self.ty(&s.ctx, rhs).unwrap_or_else(|| Rc::new(Value::Error));
        let value = self.chk.eval(&s.ctx.env, rhs);
        Scope { ctx: s.ctx.extend(None, name, ty, BinderInfo::Defined, value) }
    }

    /// The type of the variable at `lvl`.
    pub fn var_type(&self, s: &Scope, lvl: Lvl) -> Rc<Value> {
        s.ctx.binders[lvl].ty.clone()
    }

    /// The type of a term in `s`.
    pub fn type_of(&mut self, s: &Scope, c: &Rc<Core>) -> Option<Rc<Value>> {
        self.ty(&s.ctx, c)
    }

    /// How a value of a term in `s` is represented: [`Rep::Any`] if its
    /// type cannot be worked out.
    pub fn rep_of(&mut self, s: &Scope, c: &Rc<Core>) -> Rep {
        match self.ty(&s.ctx, c) {
            Some(ty) => self.rep(s, &ty),
            None => Rep::Any,
        }
    }

    /// How a value of type `ty` (in `s`) is represented.
    pub fn rep(&self, s: &Scope, ty: &Rc<Value>) -> Rep {
        match &*self.chk.force(ty.clone()) {
            Value::LitTy(Builtin::Int) => Rep::I64,
            Value::LitTy(Builtin::Bool) => Rep::Bool,
            Value::LitTy(Builtin::Unit) => Rep::Unit,
            Value::LitTy(Builtin::Str) => Rep::Str,
            Value::LitTy(Builtin::Char) => Rep::Char,
            Value::LitTy(Builtin::Float) => Rep::F64,
            Value::LitTy(Builtin::Float32) => Rep::F32,
            Value::LitTy(b) if b.width().is_some() => Rep::Word(b.width().expect("guarded")),
            Value::LitTy(_) | Value::Type(_) | Value::Row(_) => Rep::Erased,
            Value::Pi { .. } | Value::BoxTy(..) => Rep::Ptr,
            Value::Comp(a, _) => self.rep(s, a),
            // a data type is on the heap; an effect is no value at all
            Value::Opaque(u, _, _) if self.chk.datas.contains_key(u) => Rep::Ptr,
            // an array: a heap object
            Value::Opaque(u, _, _) if self.chk.natives.get(u).is_some_and(|n| n.native.is_type()) => Rep::Ptr,
            Value::Opaque(u, _, _) if self.chk.effects.contains_key(u) => Rep::Erased,
            // a type variable, which a descriptor stands for
            Value::Rigid(l, _, sp) if sp.is_empty() => match s.ctx.binders.get(*l).map(|b| self.chk.force(b.ty.clone())) {
                Some(t) if matches!(*t, Value::Type(_)) => Rep::Var(*l),
                _ => Rep::Any,
            },
            v => self.field_of(v, 4).map_or(Rep::Any, |(l, i)| Rep::Field(l, i)),
        }
    }

    /// The record variable and the field of it that a type is, if it is
    /// one: a `case` on the variable choosing a field that is a type, or a
    /// definition that unfolds to one (`Elem c @{d}`, through
    /// `Container.Elem d`), at most `depth` deep.
    fn field_of(&self, v: &Value, depth: usize) -> Option<(Lvl, usize)> {
        match v {
            Value::Rigid(l, _, sp) => {
                let [Elim::Case(_, alts)] = sp.as_slice() else { return None };
                let [alt] = alts.as_slice() else { return None };
                let Head::Con(ctor) = alt.head else { return None };
                let mut body = &alt.body;
                while let Core::Runtime(_, inner) = &**body {
                    body = inner;
                }
                let Core::Var(k, _) = &**body else { return None };
                let field = alt.vars.len().checked_sub(k + 1)?;
                self.type_fields(ctor).iter().any(|(i, _)| *i == field).then_some((*l, field))
            }
            Value::Opaque(u, _, sp) if depth > 0 => {
                let v = self.chk.unfold_stuck(*u, sp)?;
                self.field_of(&v, depth - 1)
            }
            _ => None,
        }
    }

    /// A constructor's fields that are types, each by its place among a
    /// `case` alternative's variables (after the constructor's parameters),
    /// with its name: what a record holds as descriptors at run time.
    pub fn type_fields(&self, ctor: usize) -> Vec<(usize, String)> {
        let Some(mut ty) = self.chk.constants.get(&ctor).cloned() else { return Vec::new() };
        let params = self.ctor_params(ctor);
        let mut lvl = self.scope().ctx.lvl;
        let mut out = Vec::new();
        let mut i = 0;
        while let Value::Pi { name, dom, cod, .. } = &*self.chk.force(ty.clone()) {
            if i >= params && matches!(*self.chk.force(dom.clone()), Value::Type(_)) {
                out.push((i, name.clone()));
            }
            ty = self.chk.apply(cod, vvar(lvl, name.clone()));
            lvl += 1;
            i += 1;
        }
        out
    }

    /// The fields that are types of the record a value of type `ty` is, if
    /// it is a record (a data type with one constructor) with any: its
    /// constructor, and those fields (see [`Self::type_fields`]).  A
    /// parameter of such a type comes with a descriptor for each.
    pub fn record_type_fields(&self, ty: &Rc<Value>) -> Option<(usize, Vec<(usize, String)>)> {
        let Value::Opaque(u, _, _) = &*self.chk.force(ty.clone()) else { return None };
        let [ctor] = self.chk.datas.get(u)?.as_slice() else { return None };
        let fields = self.type_fields(*ctor);
        (!fields.is_empty()).then_some((*ctor, fields))
    }

    /// How a value of the type that field `field` of `record` (a term in
    /// `s`, of a record whose constructor is `ctor`) holds is represented:
    /// what a descriptor passed with the record says.
    pub fn field_rep(&mut self, s: &Scope, record: &Rc<Core>, ctor: usize, field: usize) -> Rep {
        let Some(info) = self.chk.ctors.get(&ctor).cloned() else { return Rep::Any };
        let vars: Vec<String> = (0..info.params + info.fields).map(|i| format!("#f{i}")).collect();
        let k = vars.len() - 1 - field;
        let alt = CaseAlt { head: Head::Con(ctor), name: info.name.clone(), vars, body: Rc::new(Core::Var(k, format!("#f{field}"))) };
        let c = Rc::new(Core::Case(record.clone(), Rc::new(vec![alt])));
        let v = self.chk.eval(&s.ctx.env, &c);
        self.rep(s, &v)
    }

    /// How a value of the type `ty` (a term in `s`) is represented: what a
    /// descriptor passed for a type argument says.
    pub fn rep_of_type(&mut self, s: &Scope, ty: &Rc<Core>) -> Rep {
        let v = self.chk.eval(&s.ctx.env, ty);
        self.rep(s, &v)
    }

    /// Whether a binder of type `ty` (a term in `s`) is a type parameter:
    /// one whose type is a universe, which a descriptor stands for at run
    /// time.
    pub fn is_type_binder(&mut self, s: &Scope, ty: &Rc<Core>) -> bool {
        let v = self.chk.eval(&s.ctx.env, ty);
        matches!(*self.chk.force(v), Value::Type(_))
    }

    /// The kind of the effect variable a binder of type `ty` binds:
    /// `Some(false)` for `Effect`, `Some(true)` for `Effect []` (one that
    /// may stand for an effect resumed more than once), `None` for a binder
    /// of anything else.
    pub fn effect_binder_kind(&mut self, s: &Scope, ty: &Rc<Core>) -> Option<bool> {
        let v = self.chk.eval(&s.ctx.env, ty);
        self.chk.row_kind(&v)
    }

    /// Whether the term `c` (in `s`) is a type: what an erased argument for a
    /// type parameter is.
    pub fn is_type(&mut self, s: &Scope, c: &Rc<Core>) -> bool {
        match self.ty(&s.ctx, c) {
            Some(t) => matches!(*self.chk.force(t), Value::Type(_)),
            None => false,
        }
    }

    /// The parameters a term (in `s`) of function type still takes, as its
    /// type says: each one's representation (in `s`, so a type variable of
    /// the function's own is [`Rep::Any`]), whether it is a type parameter,
    /// whether it is erased, and whether it is a record with fields that are
    /// types (which a descriptor for each comes with).  What a partial
    /// application waits for.
    pub fn function_params(&mut self, s: &Scope, c: &Rc<Core>) -> Vec<(Rep, bool, bool, bool)> {
        let Some(mut t) = self.ty(&s.ctx, c) else { return Vec::new() };
        let mut inner = s.clone();
        let mut out = Vec::new();
        while let Value::Pi { name, grade, dom, cod, .. } = &*self.chk.force(t.clone()) {
            let is_type = matches!(*self.chk.force(dom.clone()), Value::Type(_));
            let erased = self.chk.to_poly(self.chk.force(grade.clone())).as_constant().is_some_and(|g| g.is_zero());
            let rep = match self.rep(&inner, dom) {
                Rep::Var(l) | Rep::Field(l, _) if l >= s.ctx.lvl => Rep::Any,
                r => r,
            };
            out.push((rep, is_type, erased, !erased && self.record_type_fields(dom).is_some()));
            let lvl = inner.ctx.lvl;
            inner = Scope { ctx: inner.ctx.bind(None, name, dom.clone()) };
            t = self.chk.apply(cod, vvar(lvl, name.clone()));
        }
        out
    }

    /// `head`'s arguments `args` (in `s`), each lambda among them whose
    /// binders the program does not say the types of (one read back from a
    /// value, as a solved metavariable's is) given the types of the
    /// parameter it is passed for: what lowering it as a function of those
    /// representations needs.
    pub fn annotate_args(&mut self, s: &Scope, head: &Rc<Core>, args: &[Rc<Core>]) -> Vec<Rc<Core>> {
        if !args.iter().any(|a| matches!(**a, Core::Lam(..))) {
            return args.to_vec();
        }
        let Some(mut t) = self.ty(&s.ctx, head) else { return args.to_vec() };
        let mut out = Vec::new();
        for a in args {
            let (f, _) = self.chk.split_comp(t.clone());
            let Value::Pi { dom, cod, .. } = &*self.chk.force(f) else { break };
            out.push(if matches!(**a, Core::Lam(..)) { self.annotated(s, a, dom) } else { a.clone() });
            let v = self.chk.eval(&s.ctx.env, a);
            t = self.chk.apply(cod, v);
        }
        out.extend(args[out.len()..].iter().cloned());
        out
    }

    /// A lambda, its binders noted with the types `ty` (a function type, in
    /// `s`) gives them.
    fn annotated(&mut self, s: &Scope, c: &Rc<Core>, ty: &Rc<Value>) -> Rc<Core> {
        let Core::Lam(x, body) = &**c else { return c.clone() };
        let (f, _) = self.chk.split_comp(ty.clone());
        let Value::Pi { grade, dom, cod, .. } = &*self.chk.force(f) else { return c.clone() };
        let noted = self.chk.quote(s.ctx.lvl, dom.clone());
        let grade = self.chk.to_poly(self.chk.force(grade.clone())).as_constant();
        let inner = Scope { ctx: s.ctx.bind(None, x, dom.clone()) };
        let under = self.chk.apply(cod, vvar(s.ctx.lvl, x.clone()));
        let body = self.annotated(&inner, body, &under);
        Rc::new(Core::Runtime(Note::Binds { grade, ty: Some(noted) }, Rc::new(Core::Lam(x.clone(), body))))
    }

    /// Whether a term (in `s`) is nothing at run time: a type, an effect
    /// row, a grade, or a function that makes one (a type family).
    pub fn is_type_former(&mut self, s: &Scope, c: &Rc<Core>) -> bool {
        let Some(mut t) = self.ty(&s.ctx, c) else { return false };
        let mut lvl = s.ctx.lvl;
        loop {
            match &*self.chk.force(t.clone()) {
                Value::Type(_) | Value::LitTy(Builtin::Effect | Builtin::Grade) => return true,
                Value::Pi { name, cod, .. } => {
                    t = self.chk.apply(cod, vvar(lvl, name.clone()));
                    lvl += 1;
                }
                _ => return false,
            }
        }
    }

    /// A constructor's shape, from its type: its erased arguments, each with
    /// whether it is a type parameter, and its fields' representations,
    /// where `Rep::Var(i)` is the type parameter that is its `i`th erased
    /// argument.
    pub fn ctor_shape(&mut self, ctor: usize) -> Option<(Vec<bool>, Vec<Rep>)> {
        let ty = self.chk.constants.get(&ctor)?.clone();
        let params = self.ctor_params(ctor);
        let mut s = self.scope();
        let base = s.ctx.lvl;
        let mut ty = ty;
        let (mut kinds, mut fields) = (Vec::new(), Vec::new());
        let mut i = 0;
        while let Value::Pi { name, dom, cod, .. } = &*self.chk.force(ty.clone()) {
            let lvl = s.ctx.lvl;
            if i < params {
                kinds.push(matches!(*self.chk.force(dom.clone()), Value::Type(_)));
            } else if matches!(*self.chk.force(dom.clone()), Value::Type(_)) {
                // a type, held as its descriptor
                fields.push(Rep::Desc);
            } else {
                let rep = match self.rep(&s, dom) {
                    Rep::Var(l) if l >= base => Rep::Var(l - base),
                    Rep::Var(_) | Rep::Field(..) => Rep::Any,
                    other => other,
                };
                fields.push(rep);
            }
            s = Scope { ctx: s.ctx.bind(None, name, dom.clone()) };
            ty = self.chk.apply(cod, vvar(lvl, name.clone()));
            i += 1;
        }
        Some((kinds, fields))
    }

    /// An operation's shape, from its type: its arguments' representations
    /// (after the erased ones) and its result's.  A type variable of the
    /// operation's own is `Rep::Any`: the operation is not generic at run
    /// time.
    pub fn op_shape(&mut self, op: usize) -> Option<(Vec<Rep>, Rep)> {
        let ty = self.chk.constants.get(&op)?.clone();
        let info = self.chk.ops.get(&op)?.clone();
        let mut s = self.scope();
        let mut ty = ty;
        let mut params = Vec::new();
        let mut i = 0;
        while let Value::Pi { name, dom, cod, .. } = &*self.chk.force(ty.clone()) {
            if i >= info.erased + info.arity {
                break;
            }
            let lvl = s.ctx.lvl;
            if i >= info.erased {
                params.push(match self.rep(&s, dom) {
                    Rep::Var(_) | Rep::Field(..) => Rep::Any,
                    other => other,
                });
            }
            s = Scope { ctx: s.ctx.bind(None, name, dom.clone()) };
            ty = self.chk.apply(cod, vvar(lvl, name.clone()));
            i += 1;
        }
        let (result, _) = self.chk.split_comp(ty);
        let result = match self.rep(&s, &result) {
            Rep::Var(_) | Rep::Field(..) => Rep::Any,
            other => other,
        };
        Some((params, result))
    }

    /// The scope inside an alternative of a `case` on `scrutinee` (in
    /// `s`): its variables, the constructor's erased arguments and then its
    /// fields, with their types instantiated as the scrutinee's type says.
    pub fn alt_scope(&mut self, s: &Scope, scrutinee: &Rc<Core>, alt: &CaseAlt) -> Scope {
        let ctx = self.alt_ctx(&s.ctx, scrutinee, alt);
        Scope { ctx }
    }

    /// The types of an alternative's variables, in the scope it makes.
    pub fn alt_types(&mut self, s: &Scope, scrutinee: &Rc<Core>, alt: &CaseAlt) -> Vec<Rc<Value>> {
        let inner = self.alt_scope(s, scrutinee, alt);
        (s.ctx.lvl..inner.ctx.lvl).map(|l| inner.ctx.binders[l].ty.clone()).collect()
    }

    /// A type, as a value, in `s`.
    pub fn eval_type(&self, s: &Scope, ty: &Rc<Core>) -> Rc<Value> {
        self.chk.eval(&s.ctx.env, ty)
    }

    /// What a value of `ty` looks like written out: see [`Shown`].
    pub fn shown(&mut self, s: &Scope, ty: &Rc<Value>) -> Shown {
        let ty = self.chk.force(ty.clone());
        match &*ty {
            Value::LitTy(Builtin::Int) => Shown::Int,
            Value::LitTy(Builtin::Bool) => Shown::Bool,
            Value::LitTy(Builtin::Unit) => Shown::Unit,
            Value::LitTy(Builtin::Str) => Shown::Str,
            Value::LitTy(Builtin::Char) => Shown::Char,
            Value::LitTy(b) if b.number().is_some() => Shown::Number,
            Value::BoxTy(_, a) => Shown::Box(a.clone()),
            Value::Opaque(u, _, sp) if self.chk.natives.get(u).is_some_and(|n| n.native == Native::Array) => {
                match sp.as_slice() {
                    [super::value::Elim::App(a)] => Shown::Array(a.clone()),
                    _ => Shown::Nothing,
                }
            }
            Value::Opaque(u, _, _) if self.chk.datas.contains_key(u) => {
                let key = self.chk.quote(s.ctx.lvl, ty.clone()).to_string();
                // (a metavariable left in it: something it was not told)
                if key.contains('?') {
                    return Shown::Nothing;
                }
                let mut ctors: Vec<usize> = self.chk.datas[u].clone();
                ctors.sort_by_key(|c| self.chk.ctors.get(c).map_or(0, |i| i.tag));
                let mut out = Vec::new();
                for c in ctors {
                    let name = self.chk.ctors.get(&c).map_or_else(String::new, |i| i.name.clone());
                    // one that cannot make a value of this type, as `Nil`
                    // cannot a `Vec (S n) a`, has no value to show
                    if let Some(fields) = self.ctor_fields(s, c, &ty) {
                        out.push(ShownCtor { unique: c, name, fields });
                    }
                }
                Shown::Data { key, ctors: out }
            }
            _ => Shown::Nothing,
        }
    }

    /// The written form of a type, as `Shown::Data`'s key is: `None` if
    /// it has an unsolved metavariable in it.
    pub fn type_key(&mut self, s: &Scope, ty: &Rc<Value>) -> Option<String> {
        let key = self.chk.quote(s.ctx.lvl, ty.clone()).to_string();
        (!key.contains('?')).then_some(key)
    }

    /// The types of the fields of constructor `c` making a value of `ty`,
    /// if it can make one.
    fn ctor_fields(&mut self, s: &Scope, c: usize, ty: &Rc<Value>) -> Option<Vec<Rc<Value>>> {
        let mut cty = self.chk.constants.get(&c)?.clone();
        let params = self.ctor_params(c);
        let all = params + self.chk.ctors.get(&c)?.fields;
        let mut fields = Vec::new();
        for i in 0..all {
            let forced = self.chk.force(cty.clone());
            let Value::Pi { dom, cod, .. } = &*forced else { return None };
            // every argument a metavariable: the parameters are what the
            // type says, and a field another's type depends on is unknown
            let m = self.chk.fresh_meta(&s.ctx, NodeId(0), "a constructor's argument");
            let v = self.chk.eval(&s.ctx.env, &m);
            if i >= params {
                fields.push(dom.clone());
            }
            cty = self.chk.apply(cod, v);
        }
        let (result, _) = self.chk.split_comp(cty);
        self.chk.unify(s.ctx.lvl, result, ty.clone()).then_some(fields)
    }

    /// How many of a constructor's arguments are erased type parameters.
    pub fn ctor_params(&self, ctor: usize) -> usize {
        self.chk.ctors.get(&ctor).map_or(0, |c| c.params)
    }

    fn alt_ctx(&mut self, ctx: &Ctx, scrutinee: &Rc<Core>, alt: &CaseAlt) -> Ctx {
        let Head::Con(u) = alt.head else { return ctx.clone() };
        let unknown = |ctx: &Ctx| alt.vars.iter().fold(ctx.clone(), |c, x| c.bind(None, x, Rc::new(Value::Error)));
        let (Some(mut ty), Some(scrut_ty)) = (self.chk.constants.get(&u).cloned(), self.ty(ctx, scrutinee)) else {
            return unknown(ctx);
        };
        let params = self.ctor_params(u);
        let mut inner = ctx.clone();
        for (i, x) in alt.vars.iter().enumerate() {
            let Value::Pi { dom, cod, .. } = &*self.chk.force(ty.clone()) else { return unknown(ctx) };
            if i < params {
                // an erased argument: whatever makes the constructor's type
                // the scrutinee's, which unification works out below
                let m = self.chk.fresh_meta(&inner, NodeId(0), "a constructor's argument");
                let v = self.chk.eval(&inner.env, &m);
                inner = inner.extend(None, x, dom.clone(), BinderInfo::Defined, v.clone());
                ty = self.chk.apply(cod, v);
            } else {
                let lvl = inner.lvl;
                inner = inner.bind(None, x, dom.clone());
                ty = self.chk.apply(cod, vvar(lvl, x.clone()));
            }
        }
        let (result, _) = self.chk.split_comp(ty);
        // argument by argument, so that an index unification cannot solve
        // (`S m` against the rigid `n` of a refined family) leaves the rest
        // to be solved all the same
        let (result, scrut_ty) = (self.chk.force(result), self.chk.force(scrut_ty));
        match (&*result, &*scrut_ty) {
            (Value::Opaque(a, _, xs), Value::Opaque(b, _, ys)) if a == b && xs.len() == ys.len() => {
                for (x, y) in xs.iter().zip(ys) {
                    if let (super::value::Elim::App(x), super::value::Elim::App(y)) = (x, y) {
                        self.chk.unify(inner.lvl, x.clone(), y.clone());
                    }
                }
            }
            _ => {
                self.chk.unify(inner.lvl, result, scrut_ty);
            }
        }
        inner
    }

    fn ty(&mut self, ctx: &Ctx, c: &Rc<Core>) -> Option<Rc<Value>> {
        let erased = |t: Rc<Value>| Some(t);
        Some(match &**c {
            Core::Var(i, _) => ctx.binders.get(ctx.lvl.checked_sub(i + 1)?)?.ty.clone(),
            Core::Opaque(u, _) => self.chk.constants.get(u)?.clone(),
            Core::Lit(l) => Rc::new(Value::LitTy(lit_builtin(l))),
            Core::Prim(p) => match ctx.names.get(&p.unique())? {
                super::Entry::Local(l) => ctx.binders[*l].ty.clone(),
                _ => return None,
            },
            // (a hole's solution carries no type of its own)
            Core::Meta(m) => match &self.chk.metas[*m] {
                MetaEntry::Solved(_, info) | MetaEntry::Unsolved(info) => {
                    let mt = info.ty.as_ref()?;
                    if !mt.spine.is_empty() {
                        return None;
                    }
                    mt.ty.clone()
                }
            },
            Core::Type(n) => return erased(Rc::new(Value::Type(n + 1))),
            Core::LitTy(Builtin::Div) => return erased(Rc::new(Value::LitTy(Builtin::Effect))),
            Core::LitTy(_) | Core::Pi { .. } | Core::BoxTy(..) | Core::Comp(..) => Rc::new(Value::Type(0)),
            Core::GradeLit(_) | Core::GradeAdd(..) | Core::GradeMul(..) | Core::GradeRange(..) => {
                Rc::new(Value::LitTy(Builtin::Grade))
            }
            Core::Row(..) => Rc::new(Value::LitTy(Builtin::Effect)),
            Core::Runtime(Note::Binds { grade, ty: Some(dom) }, inner) => match &**inner {
                Core::Lam(x, body) => {
                    let dom_v = self.chk.eval(&ctx.env, dom);
                    let under = ctx.bind(None, x, dom_v.clone());
                    let body_ty = self.ty(&under, body).unwrap_or_else(|| Rc::new(Value::Error));
                    let cod = Closure { env: ctx.env.clone(), body: self.chk.quote(under.lvl, body_ty) };
                    let grade = Poly::constant(grade.unwrap_or(Grade::DEFAULT)).into_value();
                    let in_type = Poly::constant(Grade::DEFAULT_IN_TYPE).into_value();
                    Rc::new(Value::Pi { name: x.clone(), icit: idyllic_ast::Icit::Explicit, grade, in_type, dom: dom_v, cod })
                }
                // what comes out of the box
                Core::Unbox(_) => self.chk.eval(&ctx.env, dom),
                _ => return self.ty(ctx, inner),
            },
            Core::Runtime(Note::Erased, _) => return None,
            Core::Runtime(_, inner) => return self.ty(ctx, inner),
            Core::Lam(..) => return None,
            Core::App(f, a) => {
                let f_ty = self.ty(ctx, f)?;
                let Value::Pi { cod, .. } = &*self.chk.force(f_ty) else { return None };
                let a_v = self.chk.eval(&ctx.env, a);
                self.chk.split_comp(self.chk.apply(cod, a_v)).0
            }
            Core::BoxIntro(e) => {
                let a = self.ty(ctx, e)?;
                Rc::new(Value::BoxTy(Poly::constant(Grade::ANY).into_value(), a))
            }
            Core::Unbox(e) => match &*{
                let t = self.ty(ctx, e)?;
                self.chk.force(t)
            } {
                Value::BoxTy(_, a) => a.clone(),
                _ => return None,
            },
            Core::Let(x, rhs, body) => {
                let ty = self.ty(ctx, rhs).unwrap_or_else(|| Rc::new(Value::Error));
                let value = self.chk.eval(&ctx.env, rhs);
                let under = ctx.extend(None, x, ty, BinderInfo::Defined, value);
                return self.ty(&under, body);
            }
            Core::Case(scrutinee, alts) => {
                for alt in alts.iter() {
                    let inner = self.alt_ctx(ctx, scrutinee, alt);
                    if let Some(t) = self.ty(&inner, &alt.body) {
                        return Some(t);
                    }
                }
                return None;
            }
            Core::Handle(_, ret, _) => {
                let ret_ty = self.ty(ctx, ret)?;
                let Value::Pi { cod, .. } = &*self.chk.force(ret_ty) else { return None };
                self.chk.split_comp(self.chk.apply(cod, vvar(ctx.lvl, "x"))).0
            }
            Core::Error => return None,
        })
    }
}
