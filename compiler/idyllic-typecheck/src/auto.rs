//! **Auto arguments**: an implicit argument found by search, `{auto d : t}`
//! or `t => ..`, as Idris finds them.
//!
//! Where one is to be inserted, what its type asks for is searched for:
//! first among the auto binders in scope, innermost first, and then among
//! the hints, the definitions `@hint` offers (a trait's instances are
//! hints). A candidate fits when its type, its implicit arguments made
//! metas, comes to what is wanted, and its own auto arguments can be found
//! in turn; each is tried on a copy of the checker, as an overloaded name's
//! meanings are (see [`overloads`](super::overloads)). The innermost auto
//! binder that fits is the one taken; among the hints, exactly one must.
//!
//! What is wanted has to be known first: a search for `Show ?a` would
//! choose `?a` rather than find what it is. So a search whose type still
//! has a meta in it waits, behind a placeholder, until the definition it is
//! in has been checked ([`Checker::settle_deferred`]), and is reported if it
//! still cannot tell. What is found is written over the placeholder as a
//! chosen overload is, and an auto argument found at once goes straight
//! into the term.

use super::*;
use crate::value::Elim;

/// How deep the search goes into the auto arguments of what it finds: a
/// search that loops, a hint for `Show a` wanting `Show a`, ends here.
const DEPTH: usize = 16;

/// An auto argument whose type was not known yet.
#[derive(Clone)]
pub(super) struct DeferredAuto {
    pub(super) meta: MetaVar,
    node: NodeId,
    pub(super) ctx: Ctx,
    wanted: VTy,
    /// put off till the end of the definition (see `auto_argument_in`)
    later: bool,
}

impl Checker {
    /// The auto argument of type `wanted`, for the term at `node`.
    pub(super) fn auto_argument(&mut self, ctx: &Ctx, node: NodeId, wanted: VTy) -> Rc<Core> {
        self.auto_argument_in(ctx, node, wanted, false)
    }

    /// The same; put off till the end of the definition if `later`, when
    /// the rest of the function's type mentions the argument (`MapBy k o v`
    /// after `{auto o : Ord k}`): then what it is applied to says which it
    /// is, as unification finds, whatever order it comes in, and a search
    /// is made only if nothing does.
    pub(super) fn auto_argument_in(&mut self, ctx: &Ctx, node: NodeId, wanted: VTy, later: bool) -> Rc<Core> {
        if later || !self.known(ctx, &wanted) {
            let meta = self.new_meta(node, MetaOrigin::Inferred("an auto argument".into()), None);
            self.placeholders.insert(meta);
            self.deferred_autos.push(DeferredAuto { meta, node, ctx: ctx.clone(), wanted, later });
            return Rc::new(Core::Meta(meta));
        }
        match self.search(ctx, node, &wanted, 0) {
            Ok(found) => found,
            Err(why) => {
                self.error(node, why);
                Rc::new(Core::Error)
            }
        }
    }

    /// Whether a type is known enough to search for: no meta left in it.
    fn known(&self, ctx: &Ctx, ty: &VTy) -> bool {
        let quoted = self.quote(ctx.lvl, ty.clone());
        let open = std::cell::Cell::new(false);
        quoted.rewrite(0, &|c, _| {
            if let Core::Meta(m) = c {
                if matches!(self.metas[*m], MetaEntry::Unsolved(_)) {
                    open.set(true);
                }
            }
            None
        });
        !open.get()
    }

    /// The searches that waited for their types, tried again; with `last`,
    /// what still cannot be told is reported. Whether any was settled.
    pub(super) fn settle_autos(&mut self, last: bool) -> bool {
        self.settle_autos_in(last, false)
    }

    /// The same, in the middle of a definition (`mid`): what was put off
    /// till its end waits.
    pub(super) fn settle_autos_in(&mut self, last: bool, mid: bool) -> bool {
        // (a literal's type may be what a search waits for, `hash 5`, and
        // the default is taken before the search is given up on; but first
        // what can be found already is, as a literal's type may wait on
        // that, `showFirst xs 0` with `Elem c` the type)
        let mut progress = false;
        if last {
            loop {
                let mut round = self.settle_literals(false);
                round |= self.search_autos(false, mid);
                round |= self.settle_free_literals();
                if !round {
                    break;
                }
                progress = true;
            }
        }
        progress |= self.settle_literals(last);
        progress |= self.search_autos(last, mid);
        progress
    }

    /// The deferred auto arguments searched for: those whose type is known,
    /// and with `last` the rest too (reported as unknown).
    fn search_autos(&mut self, last: bool, mid: bool) -> bool {
        let mut progress = false;
        for d in std::mem::take(&mut self.deferred_autos) {
            if (!self.known(&d.ctx, &d.wanted) && !last) || (mid && d.later) {
                self.deferred_autos.push(d);
                continue;
            }
            let found = if self.known(&d.ctx, &d.wanted) {
                match self.search(&d.ctx, d.node, &d.wanted, 0) {
                    Ok(found) => found,
                    Err(why) => {
                        self.error(d.node, why);
                        Rc::new(Core::Error)
                    }
                }
            } else {
                let wanted = self.quote(d.ctx.lvl, d.wanted.clone());
                self.error(d.node, TypeErrorKind::UnknownInstance(wanted));
                Rc::new(Core::Error)
            };
            let value = self.eval(&d.ctx.env, &found);
            if let MetaEntry::Unsolved(info) = &self.metas[d.meta] {
                let info = info.clone();
                self.metas[d.meta] = MetaEntry::Solved(value, info);
            }
            self.chosen.insert(d.meta, (found, d.ctx.lvl));
            progress = true;
        }
        progress
    }

    /// What there is of type `wanted`: an auto binder in scope, or a hint.
    fn search(&mut self, ctx: &Ctx, node: NodeId, wanted: &VTy, depth: usize) -> Result<Rc<Core>, TypeErrorKind> {
        let quoted = self.quote(ctx.lvl, wanted.clone());
        let shown = || quoted.clone();
        if depth > DEPTH {
            return Err(TypeErrorKind::NoInstance { wanted: shown(), deep: true });
        }
        // what is wanted of a type an error is in was reported where the
        // error is; every instance of the trait could fit it, and each be
        // tried, its own wants too, which could take as long as it likes
        if quoted.mentions_error() {
            return Ok(Rc::new(Core::Error));
        }
        // the auto binders in scope, innermost first, and what each has
        // inside it of its trait's superclasses
        let locals: Vec<Lvl> = (0..ctx.lvl).rev().filter(|&l| ctx.binders[l].auto).collect();
        for lvl in locals {
            let core = Rc::new(Core::Var(ctx.lvl - lvl - 1, ctx.binders[lvl].name.clone()));
            let ty = ctx.binders[lvl].ty.clone();
            let mut paths = vec![Vec::new()];
            if let Value::Opaque(t, _, _) = &*self.force(ty.clone()) {
                paths.extend(self.super_paths(ctx, *t, 0));
            }
            for path in paths {
                let found = {
                    let saved = self.clone();
                    let before = self.errors.len();
                    let fits = match self.through(ctx, node, core.clone(), ty.clone(), &path) {
                        Some((c, t)) => self.apply_candidate(ctx, node, c, t, wanted, depth).is_some(),
                        None => false,
                    } && self.errors.len() == before;
                    *self = saved;
                    fits
                };
                if found {
                    let (c, t) = self.through(ctx, node, core.clone(), ty.clone(), &path).expect("it was found");
                    return self.apply_candidate(ctx, node, c, t, wanted, depth).ok_or_else(|| TypeErrorKind::NoInstance { wanted: shown(), deep: false });
                }
            }
        }
        // then the hints that are in scope here, and could be for what is
        // wanted, by what their types and it come to (a cheap look first,
        // since a search is made for every method a program calls)
        let goal = self.head_of(ctx.lvl, wanted);
        let mut hints: Vec<usize> = self.hints.iter().copied().collect();
        hints.sort_unstable();
        let mut candidates = Vec::new();
        for u in hints {
            let x = hir::Ident { unique: Some(u), text: String::new() };
            let Some((core, ty, _)) = ctx.lookup(&x) else { continue };
            let head = self.head_of(ctx.lvl, &ty);
            // a hint whose own type is in error was reported there: it has
            // no head, so it would fit every goal, and make each ambiguous
            if head.0.is_none() && self.quote(ctx.lvl, ty.clone()).mentions_error() {
                continue;
            }
            if compatible(&head, &goal) {
                candidates.push((u, core, ty));
            }
        }
        // one that could is tried for real, with no copy to try it on: what
        // is wanted is known, and a hint's type is closed, so all trying it
        // can change is metas it makes itself, which are forgotten if it
        // does not fit
        if candidates.len() == 1 {
            let (u, core, ty) = candidates.pop().expect("one");
            let core = self.called(ctx, u, core);
            let mark = self.mark();
            return match self.apply_candidate(ctx, node, core, ty, wanted, depth) {
                Some(found) if self.errors.len() == mark.errors => Ok(found),
                _ => {
                    self.roll_back(mark);
                    Err(TypeErrorKind::NoInstance { wanted: shown(), deep: false })
                }
            };
        }
        let mut fitting = Vec::new();
        for (u, core, ty) in candidates {
            if self.candidate_fits(ctx, node, &core, &ty, wanted, depth) {
                fitting.push((u, core, ty));
            }
        }
        match fitting.len() {
            1 => {
                let (u, core, ty) = fitting.pop().expect("one");
                let core = self.called(ctx, u, core);
                self.apply_candidate(ctx, node, core, ty, wanted, depth).ok_or_else(|| TypeErrorKind::NoInstance { wanted: shown(), deep: false })
            }
            0 => Err(TypeErrorKind::NoInstance { wanted: shown(), deep: false }),
            _ => {
                let names = fitting.iter().map(|(u, _, _)| self.hint_name(ctx, *u)).collect();
                Err(TypeErrorKind::AmbiguousInstance { wanted: shown(), candidates: names })
            }
        }
    }

    /// Note what of an instance's record, `core`, checked in `ctx`, a method
    /// call can be made into: those of its fields that are a constant or a
    /// primitive, if it is the record itself (an instance with no
    /// parameters). Each is kept as the name it is, `Opaque`, so that it
    /// can be referred to from anywhere (see [`Checker::named_at`]).
    pub(super) fn note_instance(&mut self, ctx: &Ctx, u: usize, core: &Rc<Core>) {
        let mut args = Vec::new();
        let mut head = core;
        while let Core::App(f, a) = &**head {
            args.push(a.clone());
            head = f;
        }
        args.reverse();
        let Core::Opaque(ctor, _) = &**head else { return };
        let Some(info) = self.ctors.get(ctor) else { return };
        if args.len() != info.params + info.fields {
            return;
        }
        let fields = args[info.params..]
            .iter()
            .map(|a| {
                let mut a = a;
                while let Core::Runtime(_, inner) = &**a {
                    a = inner;
                }
                match &**a {
                    Core::Prim(_) => Some(a.clone()),
                    _ => unique_at(ctx, a, ctx.lvl, 0).map(|(u, x)| Rc::new(Core::Opaque(u, x))),
                }
            })
            .collect();
        self.instance_fields.insert(u, fields);
    }

    /// `core`, checked at level `base`, with each call of a method of a
    /// known instance, `(+) {Int} addInt x y`, made a call of what the
    /// instance gives for it, `intAdd x y`: so that a method costs what its
    /// instance's field does, a primitive's what a primitive does.
    pub(super) fn specialize(&self, ctx: &Ctx, core: &Rc<Core>, base: Lvl) -> Rc<Core> {
        if self.methods.is_empty() || self.instance_fields.is_empty() {
            return core.clone();
        }
        self.specialize_at(ctx, core, base, 0)
    }

    fn specialize_at(&self, ctx: &Ctx, core: &Rc<Core>, base: Lvl, depth: usize) -> Rc<Core> {
        core.transform(depth, &|c, d| {
            let Core::App(..) = &**c else { return None };
            let mut args = Vec::new();
            let mut head = c;
            while let Core::App(f, a) = &**head {
                args.push(a.clone());
                head = f;
            }
            args.reverse();
            let (method, written) = unique_at(ctx, head, base, d)?;
            let &(_, field, params) = self.methods.get(&method)?;
            let mut dict = args.get(params)?;
            while let Core::Runtime(_, inner) = &**dict {
                dict = inner;
            }
            let (instance, _) = unique_at(ctx, dict, base, d)?;
            let given = self.instance_fields.get(&instance)?.get(field)?.as_ref()?;
            // (shown as the method it was called as: `* n 2`)
            let given = match &*named_at(ctx, given, base, d)? {
                Core::Var(i, _) => Rc::new(Core::Var(*i, written)),
                Core::Opaque(u, _) => Rc::new(Core::Opaque(*u, written)),
                _ => named_at(ctx, given, base, d)?,
            };
            Some(args[params + 1..].iter().fold(given, |f, a| Rc::new(Core::App(f, self.specialize_at(ctx, a, base, d)))))
        })
    }

    /// The recursive definitions recorded since `mark`, specialized.
    pub(super) fn specialize_rec_defs(&mut self, ctx: &Ctx, mark: usize) {
        if self.methods.is_empty() || self.instance_fields.is_empty() {
            return;
        }
        for i in mark..self.rec_defs.len() {
            // what is around it past `ctx` is local, not a method
            let d = &self.rec_defs[i];
            let core = self.specialize(ctx, &d.core, d.lvl);
            self.rec_defs[i].core = core;
        }
    }

    /// Where the checker's growing state is, to go back to.
    fn mark(&self) -> Mark {
        Mark {
            metas: self.metas.len(),
            errors: self.errors.len(),
            postponed: self.postponed.len(),
            opened: self.opened.len(),
            joins: self.joins.len(),
            deferred: self.deferred.len(),
            deferred_autos: self.deferred_autos.len(),
            deferred_literals: self.deferred_literals.len(),
        }
    }

    /// Back to `mark`: what was made since forgotten.
    fn roll_back(&mut self, mark: Mark) {
        self.metas.truncate(mark.metas);
        self.errors.truncate(mark.errors);
        self.postponed.truncate(mark.postponed);
        self.opened.truncate(mark.opened);
        self.joins.truncate(mark.joins);
        self.deferred.truncate(mark.deferred);
        self.deferred_autos.truncate(mark.deferred_autos);
        self.deferred_literals.truncate(mark.deferred_literals);
        self.chosen.retain(|m, _| *m < mark.metas);
        self.placeholders.retain(|m| *m < mark.metas);
        self.inferred.retain(|m, _| *m < mark.metas);
    }

    /// What a type comes to past its binders, roughly: the constant at its
    /// head, and that of its first argument, where they are constants.
    fn head_of(&self, lvl: Lvl, ty: &VTy) -> (Option<usize>, Option<usize>) {
        let mut ty = self.force(ty.clone());
        let mut l = lvl;
        while let Value::Pi { name, icit: Icit::Implicit | Icit::Auto, cod, .. } = &*ty {
            ty = self.force(self.split_comp(self.apply(cod, vvar(l, name.clone()))).0);
            l += 1;
        }
        let Value::Opaque(t, _, spine) = &*ty else { return (None, None) };
        let first = spine.iter().find_map(|e| match e {
            Elim::App(a) => Some(a.clone()),
            _ => None,
        });
        let first = first.and_then(|a| match &*self.force(a) {
            Value::Opaque(u, _, _) => Some(*u),
            Value::LitTy(b) => Some(usize::MAX - *b as usize),
            _ => None,
        });
        (Some(*t), first)
    }

    /// The ways to the superclasses inside a record of trait `t`: each a
    /// path of superclass projections, its own and on through theirs.
    fn super_paths(&self, ctx: &Ctx, t: usize, depth: usize) -> Vec<Vec<usize>> {
        if depth > 4 {
            return Vec::new();
        }
        let mut paths = Vec::new();
        for &p in self.supers.get(&t).into_iter().flatten() {
            paths.push(vec![p]);
            // what the projection gives: the trait its type comes to
            let x = hir::Ident { unique: Some(p), text: String::new() };
            if let Some(next) = ctx.lookup(&x).and_then(|(_, ty, _)| self.result_trait(&ty)) {
                for rest in self.super_paths(ctx, next, depth + 1) {
                    paths.push(std::iter::once(p).chain(rest).collect());
                }
            }
        }
        paths
    }

    /// The trait a projection's type comes to, past its binders.
    fn result_trait(&self, ty: &VTy) -> Option<usize> {
        let mut ty = self.force(ty.clone());
        let mut lvl = 0;
        while let Value::Pi { name, cod, .. } = &*ty {
            ty = self.force(self.split_comp(self.apply(cod, vvar(lvl, name.clone()))).0);
            lvl += 1;
        }
        match &*ty {
            Value::Opaque(t, _, _) if self.traits.contains(t) => Some(*t),
            _ => None,
        }
    }

    /// `core : ty` taken through each projection of `path` in turn: the
    /// superclass record inside it, and its type.
    fn through(&mut self, ctx: &Ctx, node: NodeId, mut core: Rc<Core>, mut ty: VTy, path: &[usize]) -> Option<(Rc<Core>, VTy)> {
        for &p in path {
            let x = hir::Ident { unique: Some(p), text: String::new() };
            let (p_core, p_ty, _) = ctx.lookup(&x)?;
            let (p_core, p_ty) = self.insert_implicit(ctx, node, p_core, p_ty);
            let Value::Pi { icit: Icit::Explicit, dom, cod, .. } = &*self.force(p_ty) else { return None };
            if !self.unify(ctx.lvl, ty.clone(), dom.clone()) {
                return None;
            }
            let value = self.eval(&ctx.env, &core);
            ty = self.split_comp(self.apply(cod, value)).0;
            core = Rc::new(Core::App(p_core, core));
        }
        Some((core, ty))
    }

    /// Whether `core : ty` gives what is wanted, as [`Checker::apply_candidate`]
    /// would: tried on a copy, the checker as it was afterwards.
    fn candidate_fits(&mut self, ctx: &Ctx, node: NodeId, core: &Rc<Core>, ty: &VTy, wanted: &VTy, depth: usize) -> bool {
        let saved = self.clone();
        let before = self.errors.len();
        let fits = self.apply_candidate(ctx, node, core.clone(), ty.clone(), wanted, depth).is_some() && self.errors.len() == before;
        *self = saved;
        fits
    }

    /// `core : ty`, applied to metas for its implicit arguments and to what
    /// search finds for its auto ones, as what is wanted: if it comes to that.
    fn apply_candidate(&mut self, ctx: &Ctx, node: NodeId, mut core: Rc<Core>, mut ty: VTy, wanted: &VTy, depth: usize) -> Option<Rc<Core>> {
        // its own auto arguments are looked for once what it comes to is
        // known, which is what says what they are
        let mut owed: Vec<(MetaVar, VTy)> = Vec::new();
        loop {
            ty = self.force(ty);
            let (name, icit, grade, dom, cod) = match &*ty {
                Value::Pi { name, icit, grade, dom, cod, .. } if *icit != Icit::Explicit => {
                    (name.clone(), *icit, grade.clone(), dom.clone(), cod.clone())
                }
                _ => break,
            };
            let arg = if icit == Icit::Auto {
                let meta = self.new_meta(node, MetaOrigin::Inferred("an auto argument".into()), None);
                self.placeholders.insert(meta);
                owed.push((meta, dom.clone()));
                Rc::new(Core::Meta(meta))
            } else {
                let desc = MetaOrigin::Inferred(format!("the implicit argument {name}"));
                self.fresh_meta_with(ctx, node, desc, Some(dom.clone()))
            };
            let arg_v = self.eval(&ctx.env, &arg);
            core = Rc::new(Core::App(core, self.argument(&grade, arg)));
            ty = self.split_comp(self.apply(&cod, arg_v)).0;
        }
        if !self.unify(ctx.lvl, ty, wanted.clone()) {
            return None;
        }
        for (meta, dom) in owed {
            let found = self.search(ctx, node, &dom, depth + 1).ok()?;
            let value = self.eval(&ctx.env, &found);
            if let MetaEntry::Unsolved(info) = &self.metas[meta] {
                let info = info.clone();
                self.metas[meta] = MetaEntry::Solved(value, info);
            }
            self.chosen.insert(meta, (found, ctx.lvl));
        }
        Some(core)
    }

    /// Each instance of a trait this package declares (a hint whose type
    /// comes to a trait's record): that it lives with its trait or with the
    /// type it is for, and that no other instance of the trait is for types
    /// that unify with its.
    pub(super) fn check_instances(&mut self, ctx: &Ctx, m: &hir::Module) {
        let own = |u: usize| (m.uniques.0..m.uniques.1).contains(&u);
        // the package's, for the orphan rule: its earlier parts' too
        let parts = self.kin.clone();
        let kin = |u: usize| own(u) || parts.iter().any(|r| r.contains(&u));
        let mut instances: Vec<(usize, NodeId, usize)> = Vec::new();
        let mut hints: Vec<usize> = self.hints.iter().copied().collect();
        hints.sort_unstable();
        let node_of: HashMap<usize, NodeId> =
            m.groups.iter().flat_map(hir::BindGroup::binds).filter_map(|b| Some((b.name.unique?, b.id))).collect();
        for u in hints {
            let x = hir::Ident { unique: Some(u), text: String::new() };
            let Some((_, ty, _)) = ctx.lookup(&x) else { continue };
            let Some((trait_u, head)) = self.instance_of(ctx, &ty) else { continue };
            instances.push((u, node_of.get(&u).copied().unwrap_or(NodeId(0)), trait_u));
            if !own(u) {
                continue;
            }
            let node = node_of.get(&u).copied().unwrap_or(NodeId(0));
            if !kin(trait_u) && !head.is_some_and(kin) {
                let instance = self.quote(ctx.lvl, ty.clone());
                let trait_name = self.homes.get(&trait_u).map_or_else(|| "its trait".to_string(), |h| format!("the trait, from {h},"));
                self.error(node, TypeErrorKind::OrphanInstance { instance, trait_name });
            }
        }
        // each own instance against those before it of the same trait
        for i in 0..instances.len() {
            let (u, node, t) = instances[i];
            if !own(u) {
                continue;
            }
            for j in 0..i {
                let (v, _, s) = instances[j];
                if s != t {
                    continue;
                }
                if self.heads_unify(ctx, u, v) {
                    let x = hir::Ident { unique: Some(u), text: String::new() };
                    let instance = ctx.lookup(&x).map(|(_, ty, _)| self.quote(ctx.lvl, ty)).unwrap_or_else(|| Rc::new(Core::Error));
                    let other = self.hint_name(ctx, v);
                    self.error(node, TypeErrorKind::OverlappingInstances { instance, other });
                    break;
                }
            }
        }
    }

    /// The trait an instance's type `ty` is of, past its implicit and auto
    /// binders, and the type constructor of what it is for (its first
    /// argument's head), if that is a declared one.
    fn instance_of(&self, ctx: &Ctx, ty: &VTy) -> Option<(usize, Option<usize>)> {
        let mut ty = self.force(ty.clone());
        let mut lvl = ctx.lvl;
        while let Value::Pi { name, icit: Icit::Implicit | Icit::Auto, cod, .. } = &*ty {
            ty = self.force(self.split_comp(self.apply(cod, vvar(lvl, name.clone()))).0);
            lvl += 1;
        }
        let Value::Opaque(t, _, spine) = &*ty else { return None };
        if !self.traits.contains(t) {
            return None;
        }
        let head = spine.iter().find_map(|e| match e {
            Elim::App(a) => Some(a.clone()),
            _ => None,
        });
        let head = head.and_then(|a| match &*self.force(a) {
            Value::Opaque(u, _, _) => Some(*u),
            _ => None,
        });
        Some((*t, head))
    }

    /// Whether the instances `u` and `v` are for types that unify, their
    /// implicit and auto binders made metas: on a copy.
    fn heads_unify(&mut self, ctx: &Ctx, u: usize, v: usize) -> bool {
        let saved = self.clone();
        let result = |s: &mut Self, w: usize| -> Option<VTy> {
            let x = hir::Ident { unique: Some(w), text: String::new() };
            let (_, mut ty, _) = ctx.lookup(&x)?;
            loop {
                ty = s.force(ty);
                let (dom, cod) = match &*ty {
                    Value::Pi { icit: Icit::Implicit | Icit::Auto, dom, cod, .. } => (dom.clone(), cod.clone()),
                    _ => break,
                };
                let arg = s.fresh_meta_with(ctx, NodeId(0), MetaOrigin::Inferred("an instance's argument".into()), Some(dom));
                let arg_v = s.eval(&ctx.env, &arg);
                ty = s.split_comp(s.apply(&cod, arg_v)).0;
            }
            Some(ty)
        };
        let unify = match (result(self, u), result(self, v)) {
            (Some(a), Some(b)) => self.unify(ctx.lvl, a, b),
            _ => false,
        };
        *self = saved;
        unify
    }

    /// The hint `u`, referred to as `core` (with no name, as the search
    /// looks it up), with the name a type shows it by: its own, or for an
    /// instance with none, `instance Ord Int`.
    fn called(&self, ctx: &Ctx, u: usize, core: Rc<Core>) -> Rc<Core> {
        let name = self.hint_names.get(&u).cloned().unwrap_or_default();
        let name = name.split(", from").next().unwrap_or_default().to_string();
        let name = if name.is_empty() || name.contains("#instance") {
            let x = hir::Ident { unique: Some(u), text: String::new() };
            let Some((_, ty, _)) = ctx.lookup(&x) else { return core };
            // (what it is an instance of, past what it is for any of)
            let mut head = self.quote(ctx.lvl, ty);
            while let Core::Pi { icit: Icit::Implicit | Icit::Auto, cod, .. } = &*head {
                head = cod.clone();
            }
            format!("instance {head}")
        } else {
            name
        };
        match &*core {
            Core::Opaque(u, _) => Rc::new(Core::Opaque(*u, name)),
            Core::Var(i, _) => Rc::new(Core::Var(*i, name)),
            _ => core,
        }
    }

    /// A hint, as a message names it.
    fn hint_name(&self, ctx: &Ctx, u: usize) -> String {
        let name = match (self.written_as.get(&u), self.origin.get(&u)) {
            (Some(w), Some(o)) => format!("{w}, from {o}"),
            (Some(w), None) => w.clone(),
            _ => self.hint_names.get(&u).cloned().unwrap_or_else(|| format!("hint #{u}")),
        };
        // an instance with no name of its own is known by its type
        if !name.starts_with("#instance") && !name.contains(".#instance") {
            return name;
        }
        let x = hir::Ident { unique: Some(u), text: String::new() };
        match ctx.lookup(&x) {
            Some((_, ty, _)) => format!("the instance {}", self.quote(ctx.lvl, ty)),
            None => "an instance".to_string(),
        }
    }
}

/// How far the checker's growing state had grown (see [`Checker::mark`]).
struct Mark {
    metas: usize,
    errors: usize,
    postponed: usize,
    opened: usize,
    joins: usize,
    deferred: usize,
    deferred_autos: usize,
    deferred_literals: usize,
}

/// Whether a hint whose type comes to `hint` could give what comes to
/// `goal`: what is not known of either could be anything.
fn compatible(hint: &(Option<usize>, Option<usize>), goal: &(Option<usize>, Option<usize>)) -> bool {
    let fits = |a: Option<usize>, b: Option<usize>| a.is_none() || b.is_none() || a == b;
    fits(hint.0, goal.0) && fits(hint.1, goal.1)
}

/// The name `core` is, at `depth` binders inside a term checked at level
/// `base` of `ctx`: a constant, or a slot of `ctx` below `base` (one past
/// it is local to the term, and is no instance or method).
fn unique_at(ctx: &Ctx, core: &Rc<Core>, base: Lvl, depth: usize) -> Option<(usize, String)> {
    match &**core {
        Core::Opaque(u, x) => Some((*u, x.clone())),
        Core::Var(i, x) => {
            let l = (base + depth).checked_sub(i + 1).filter(|l| *l < base)?;
            Some((ctx.binders.get(l)?.key?, x.clone()))
        }
        _ => None,
    }
}

/// What [`Checker::note_instance`] kept, referred to from `depth` binders
/// inside a term checked at level `base` of `ctx`.
fn named_at(ctx: &Ctx, kept: &Rc<Core>, base: Lvl, depth: usize) -> Option<Rc<Core>> {
    let Core::Opaque(u, x) = &**kept else { return Some(kept.clone()) };
    match ctx.names.get(u)? {
        Entry::Local(l) if *l < base => Some(Rc::new(Core::Var(base + depth - l - 1, x.clone()))),
        Entry::Local(_) => None,
        Entry::Opaque(_) => Some(kept.clone()),
    }
}
