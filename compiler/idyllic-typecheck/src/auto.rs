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

/// How deep the search goes into the auto arguments of what it finds: a
/// search that loops, a hint for `Show a` wanting `Show a`, ends here.
const DEPTH: usize = 16;

/// An auto argument whose type was not known yet.
#[derive(Clone)]
pub(super) struct DeferredAuto {
    meta: MetaVar,
    node: NodeId,
    ctx: Ctx,
    wanted: VTy,
}

impl Checker {
    /// The auto argument of type `wanted`, for the term at `node`.
    pub(super) fn auto_argument(&mut self, ctx: &Ctx, node: NodeId, wanted: VTy) -> Rc<Core> {
        if !self.known(ctx, &wanted) {
            let meta = self.new_meta(node, MetaOrigin::Inferred("an auto argument".into()), None);
            self.placeholders.insert(meta);
            self.deferred_autos.push(DeferredAuto { meta, node, ctx: ctx.clone(), wanted });
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
        let mut progress = false;
        for d in std::mem::take(&mut self.deferred_autos) {
            if !self.known(&d.ctx, &d.wanted) && !last {
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
        // the auto binders in scope, innermost first
        let locals: Vec<Lvl> = (0..ctx.lvl).rev().filter(|&l| ctx.binders[l].auto).collect();
        for lvl in locals {
            let core = Rc::new(Core::Var(ctx.lvl - lvl - 1, ctx.binders[lvl].name.clone()));
            let ty = ctx.binders[lvl].ty.clone();
            if self.candidate_fits(ctx, node, &core, &ty, wanted, depth) {
                return self.apply_candidate(ctx, node, core, ty, wanted, depth).ok_or_else(|| TypeErrorKind::NoInstance { wanted: shown(), deep: false });
            }
        }
        // then the hints that are in scope here
        let mut hints: Vec<usize> = self.hints.iter().copied().collect();
        hints.sort_unstable();
        let mut fitting = Vec::new();
        for u in hints {
            let x = hir::Ident { unique: Some(u), text: String::new() };
            let Some((core, ty, _)) = ctx.lookup(&x) else { continue };
            if self.candidate_fits(ctx, node, &core, &ty, wanted, depth) {
                fitting.push((u, core, ty));
            }
        }
        match fitting.len() {
            1 => {
                let (_, core, ty) = fitting.pop().expect("one");
                self.apply_candidate(ctx, node, core, ty, wanted, depth).ok_or_else(|| TypeErrorKind::NoInstance { wanted: shown(), deep: false })
            }
            0 => Err(TypeErrorKind::NoInstance { wanted: shown(), deep: false }),
            _ => {
                let names = fitting.iter().map(|(u, _, _)| self.hint_name(*u)).collect();
                Err(TypeErrorKind::AmbiguousInstance { wanted: shown(), candidates: names })
            }
        }
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

    /// A hint, as a message names it.
    fn hint_name(&self, u: usize) -> String {
        match (self.written_as.get(&u), self.origin.get(&u)) {
            (Some(w), Some(o)) => format!("{w}, from {o}"),
            (Some(w), None) => w.clone(),
            _ => self.hint_names.get(&u).cloned().unwrap_or_else(|| format!("hint #{u}")),
        }
    }
}
