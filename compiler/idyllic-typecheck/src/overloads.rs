//! **Names with more than one meaning**, chosen by type, as Meadow chooses
//! them (after Idris).
//!
//! A module's own top-level names and what its `use`s bring are one layer,
//! where a clash is an overload: the renamer keeps every meaning a name may
//! have where it is written ([`hir::Module::overloads`]), and the checker
//! takes the one whose type fits.
//!
//! A term headed by such a name -- the name alone, or applied, `Just 3` --
//! is elaborated once for each meaning, as a trial on a copy of the
//! checker, against the type wanted if one is: the meanings whose trial
//! made no error are the ones that fit. One that fits is elaborated for
//! real. Several that fit put the term off: a placeholder stands for it,
//! of a type the rest of the definition goes on to constrain, and the term
//! is tried again against that type once the definition has been checked
//! ([`Checker::settle_deferred`]). Then the placeholder is written over
//! with what the term was elaborated to ([`Checker::resolve`]). None
//! fitting, or several still fitting at the end, is an error that lists
//! them.
//!
//! Names whose choices hang on each other are chosen together for free: a
//! trial of `size (Just 5)` with one `size` elaborates `Just 5`, which
//! tries each `Just` in turn against what that `size` takes. A constructor
//! in a pattern is chosen by the type of what it is matched against.

use super::*;
use hir::TermKind;

/// A term put off because more than one meaning of its head still fitted.
#[derive(Clone)]
pub(super) struct Deferred {
    /// the placeholder standing for it
    meta: MetaVar,
    /// the node its overloaded name was written at
    head: NodeId,
    term: hir::Term,
    ctx: Ctx,
    /// what it is to be: the type wanted, or a meta the rest constrains
    ty: VTy,
}

impl Checker {
    /// The node of the overloaded name heading `t` (`f` in `f a b`), if
    /// it heads one whose meaning has not been chosen.
    fn overloaded_head(&self, t: &hir::Term) -> Option<NodeId> {
        let mut t = t;
        while let TermKind::App(f, _) | TermKind::ImplicitApp(f, _) | TermKind::AutoApp(f, _) = &t.kind {
            t = f;
        }
        let TermKind::Var(_) = &t.kind else { return None };
        (self.overloads.contains_key(&t.id) && !self.forced.contains_key(&t.id)).then_some(t.id)
    }

    /// The meaning a name has where it is written: the one chosen for it,
    /// if it is overloaded.
    pub(super) fn meaning(&self, node: NodeId, x: &hir::Ident) -> hir::Ident {
        match self.forced.get(&node) {
            Some(&u) => hir::Ident { unique: Some(u), text: x.text.clone() },
            None => x.clone(),
        }
    }

    /// Elaborate `t`, against `wanted` if it is to have a type, as headed
    /// by the meaning `u` of its overloaded name; with whether that made no
    /// error. On a copy: the checker is as it was afterwards.
    fn fits(&mut self, ctx: &Ctx, t: &hir::Term, head: NodeId, u: usize, wanted: Option<&VTy>) -> bool {
        let saved = self.clone();
        let before = self.errors.len();
        self.forced.insert(head, u);
        match wanted {
            Some(ty) => {
                self.check(ctx, t, ty.clone());
            }
            None => {
                self.infer(ctx, t);
            }
        }
        let fits = self.errors.len() == before;
        *self = saved;
        fits
    }

    /// The meanings of the name heading `t` that fit.
    fn fitting(&mut self, ctx: &Ctx, t: &hir::Term, head: NodeId, wanted: Option<&VTy>) -> Vec<usize> {
        let candidates = self.overloads.get(&head).cloned().unwrap_or_default();
        candidates.into_iter().filter(|&u| self.fits(ctx, t, head, u, wanted)).collect()
    }

    /// Infer or check a term headed by an overloaded name, if `t` is one.
    pub(super) fn overloaded(
        &mut self,
        ctx: &Ctx,
        t: &hir::Term,
        wanted: Option<&VTy>,
    ) -> Option<(Rc<Core>, VTy, Usage)> {
        let head = self.overloaded_head(t)?;
        let name = self.overloaded_name(t);
        self.overload_names.insert(head, name);
        let fits = self.fitting(ctx, t, head, wanted);
        Some(match fits[..] {
            [u] => self.elaborate_as(ctx, t, head, u, wanted),
            [] => {
                let wanted = wanted.map(|ty| self.quote(ctx.lvl, ty.clone()));
                self.no_meaning(ctx, head, wanted);
                (Rc::new(Core::Error), Rc::new(Value::Error), Usage::unknown())
            }
            // more than one: wait for the rest of the definition to say
            _ => {
                let ty = match wanted {
                    Some(ty) => ty.clone(),
                    None => {
                        let c = self.fresh_meta(ctx, t.id, "the type of an overloaded name's use");
                        self.eval(&ctx.env, &c)
                    }
                };
                let meta = self.new_meta(t.id, MetaOrigin::Inferred("an overloaded name's meaning".into()), None);
                self.placeholders.insert(meta);
                self.deferred.push(Deferred { meta, head, term: t.clone(), ctx: ctx.clone(), ty: ty.clone() });
                (Rc::new(Core::Meta(meta)), ty, Usage::none())
            }
        })
    }

    /// Elaborate `t` for real, its overloaded name meaning `u`.
    fn elaborate_as(&mut self, ctx: &Ctx, t: &hir::Term, head: NodeId, u: usize, wanted: Option<&VTy>) -> (Rc<Core>, VTy, Usage) {
        self.forced.insert(head, u);
        match wanted {
            Some(ty) => {
                let (core, usage) = self.check(ctx, t, ty.clone());
                (core, ty.clone(), usage)
            }
            None => self.infer(ctx, t),
        }
    }

    /// Choose what the terms put off mean, now that the definition they are
    /// in has been checked: each against what its type has come to be.
    /// Choosing one can settle another, so round until nothing changes;
    /// what still has several meanings then is ambiguous.
    pub(super) fn settle_deferred(&mut self) {
        loop {
            // (a search for an auto argument may wait on an overloaded name,
            // and the other way round)
            let mut progress = self.settle_autos(false);
            for d in std::mem::take(&mut self.deferred) {
                let fits = self.fitting(&d.ctx, &d.term, d.head, Some(&d.ty));
                match fits[..] {
                    [u] => {
                        let (core, _, _) = self.elaborate_as(&d.ctx, &d.term, d.head, u, Some(&d.ty));
                        self.choose_deferred(&d, core);
                        progress = true;
                    }
                    [] => {
                        let wanted = self.shown_wanted(&d.ctx, &d.ty);
                        self.no_meaning(&d.ctx, d.head, wanted);
                        self.give_up(&d);
                        progress = true;
                    }
                    _ => self.deferred.push(d),
                }
            }
            if !progress || (self.deferred.is_empty() && self.deferred_autos.is_empty()) {
                break;
            }
        }
        self.settle_autos(true);
        for d in std::mem::take(&mut self.deferred) {
            let fits = self.fitting(&d.ctx, &d.term, d.head, Some(&d.ty));
            let wanted = self.shown_wanted(&d.ctx, &d.ty);
            let name = self.overloaded_name(&d.term);
            let candidates = self.candidate_lines(&d.ctx, d.head, &fits);
            let example = self.example(&name, &fits);
            self.error(d.head, TypeErrorKind::AmbiguousName { name, wanted, fitting: fits.len(), candidates, example });
            self.give_up(&d);
        }
    }

    /// What was reported about the term `d` put off is all there is to say:
    /// it stands for an error, of a type that is one, so that nothing about
    /// it is reported again.
    fn give_up(&mut self, d: &Deferred) {
        self.abandon(&d.ty);
        self.choose_deferred(d, Rc::new(Core::Error));
    }

    /// The type wanted where a name is used, to show: nothing, if nothing is
    /// known of it yet.
    fn shown_wanted(&self, ctx: &Ctx, ty: &VTy) -> Option<Rc<Core>> {
        match &*self.force(ty.clone()) {
            Value::Flex(..) => None,
            _ => Some(self.quote(ctx.lvl, ty.clone())),
        }
    }

    /// One of the meanings in `fits`, written in full, to show how to say
    /// which is meant.
    fn example(&self, name: &str, fits: &[usize]) -> String {
        fits.iter()
            .find_map(|u| {
                let written = self.written_as.get(u)?;
                let from = self.origin.get(u)?.trim_matches('`');
                Some(if written.contains('.') || from.contains(' ') { written.clone() } else { format!("{from}.{written}") })
            })
            .unwrap_or_else(|| name.to_string())
    }

    /// The term `d` put off is `core`: its placeholder is solved with what
    /// that is, for what is still to be checked, and written over with it
    /// once the definition is done ([`Checker::resolve`]).
    fn choose_deferred(&mut self, d: &Deferred, core: Rc<Core>) {
        let value = self.eval(&d.ctx.env, &core);
        if let MetaEntry::Unsolved(info) = &self.metas[d.meta] {
            let info = info.clone();
            self.metas[d.meta] = MetaEntry::Solved(value, info);
        }
        self.chosen.insert(d.meta, (core, d.ctx.lvl));
    }

    /// `core`, of a definition checked at level `base`, with each
    /// placeholder written over with what it stands for.
    pub(super) fn resolve(&self, core: &Rc<Core>, base: Lvl) -> Rc<Core> {
        if self.chosen.is_empty() {
            return core.clone();
        }
        core.rewrite(0, &|c, d| match c {
            Core::Meta(m) => self.chosen.get(m).map(|(chosen, at)| {
                // what it stands for may hold placeholders in turn: a hint's
                // own auto arguments
                let chosen = &self.resolve(chosen, *at);
                // made at level `at`, and used at `base + d`
                let by = (base + d) as isize - *at as isize;
                if by == 0 {
                    return chosen.clone();
                }
                chosen.rewrite(0, &|c, inner| match c {
                    Core::Var(i, x) if *i >= inner => Some(Rc::new(Core::Var((*i as isize + by).max(0) as usize, x.clone()))),
                    _ => None,
                })
            }),
            _ => None,
        })
    }

    /// The recursive definitions recorded since `mark`, their placeholders
    /// written over.
    pub(super) fn resolve_rec_defs(&mut self, mark: usize) {
        if self.chosen.is_empty() {
            return;
        }
        for i in mark..self.rec_defs.len() {
            let d = &self.rec_defs[i];
            let core = self.resolve(&d.core, d.lvl);
            self.rec_defs[i].core = core;
        }
    }

    /// The constructor a pattern means, for what it is matched against: of
    /// an overloaded name, the one of the type `ty` has.
    pub(super) fn pattern_ctor(&mut self, ctx: &Ctx, pat: &hir::Pattern, c: &hir::Ident, ty: &VTy) -> Option<usize> {
        if let Some(&u) = self.pattern_choice.get(&pat.id) {
            return Some(u);
        }
        let Some(candidates) = self.overloads.get(&pat.id).cloned() else { return c.unique };
        self.overload_names.insert(pat.id, c.text.clone());
        let ty = self.force(ty.clone());
        let of = |s: &Self, u: usize| s.ctors.get(&u).map(|info| info.data);
        let fits: Vec<usize> = match &*ty {
            Value::Opaque(d, _, _) => candidates.iter().copied().filter(|&u| of(self, u) == Some(*d)).collect(),
            Value::Error => return None,
            _ => candidates.clone(),
        };
        match fits[..] {
            [u] => {
                self.pattern_choice.insert(pat.id, u);
                Some(u)
            }
            _ => {
                let wanted = self.quote(ctx.lvl, ty);
                let lines = self.candidate_lines(ctx, pat.id, &fits);
                let kind = if fits.is_empty() {
                    let candidates = self.candidate_lines(ctx, pat.id, &[]);
                    TypeErrorKind::NoMeaning { name: c.text.clone(), wanted: Some(wanted), candidates }
                } else {
                    let example = self.example(&c.text, &fits);
                    TypeErrorKind::AmbiguousName { name: c.text.clone(), wanted: Some(wanted), fitting: fits.len(), candidates: lines, example }
                };
                self.error(pat.id, kind);
                None
            }
        }
    }

    /// The constructor a pattern was found to mean, or the one it names.
    pub(super) fn pattern_unique(&self, pat: &hir::Pattern, c: &hir::Ident) -> Option<usize> {
        self.pattern_choice.get(&pat.id).copied().or(c.unique)
    }

    /// Say that no meaning of the name at `head` fits.
    fn no_meaning(&mut self, ctx: &Ctx, head: NodeId, wanted: Option<Rc<Core>>) {
        let candidates = self.candidate_lines(ctx, head, &[]);
        let name = self.names_of(head);
        self.error(head, TypeErrorKind::NoMeaning { name, wanted, candidates });
    }

    fn overloaded_name(&self, t: &hir::Term) -> String {
        let mut t = t;
        while let TermKind::App(f, _) | TermKind::ImplicitApp(f, _) | TermKind::AutoApp(f, _) = &t.kind {
            t = f;
        }
        match &t.kind {
            TermKind::Var(x) => x.text.clone(),
            _ => "this name".into(),
        }
    }

    /// The spelling of the overloaded name at `node`.
    fn names_of(&self, node: NodeId) -> String {
        self.overload_names.get(&node).cloned().unwrap_or_else(|| "this name".into())
    }

    /// How each thing an overloaded name may mean is written in full, and
    /// where it is from: a constructor under its type (`Maybe.Just`), and
    /// anything of another module or package with that module's name.
    pub(super) fn name_meanings(&mut self, m: &hir::Module, deps: &[&super::packages::CompiledPackage]) {
        let prelude: HashSet<usize> = m.prelude.values().copied().collect();
        let from = |u: usize, q: Option<&String>| match q {
            Some(q) => format!("`{q}`"),
            None if prelude.contains(&u) => "the prelude".to_string(),
            None => "this module".to_string(),
        };
        for d in &m.datas {
            for c in &d.ctors {
                let Some(u) = c.name.unique else { continue };
                self.written_as.insert(u, format!("{}.{}", d.name.text, c.name.text));
                let q = d.name.unique.and_then(|t| m.qualifiers.get(&t));
                self.origin.insert(u, from(u, q));
            }
        }
        for b in m.groups.iter().flat_map(hir::BindGroup::binds) {
            let Some(u) = b.name.unique else { continue };
            self.origin.insert(u, from(u, m.qualifiers.get(&u)));
        }
        // another package's, by the paths its symbols give: `Maybe.Maybe.Just`
        for p in deps {
            let ctors: HashSet<usize> = p.ctors.iter().map(|(u, _)| *u).collect();
            for (u, package, path) in &p.symbols {
                let parts: Vec<&str> = path.split('.').collect();
                let (module, written) = match parts.len() {
                    n if ctors.contains(u) && n >= 2 => (&parts[..n - 2], parts[n - 2..].join(".")),
                    n if n >= 1 => (&parts[..n - 1], parts[n - 1].to_string()),
                    _ => continue,
                };
                self.written_as.insert(*u, written);
                let module = if module.is_empty() { package.clone() } else { format!("{package}.{}", module.join(".")) };
                self.origin.insert(*u, format!("`{module}`"));
            }
        }
    }

    /// Each meaning of the name at `node`, as a line: how it is written,
    /// its type, and where it is from; those in `fitting` marked.
    fn candidate_lines(&mut self, ctx: &Ctx, node: NodeId, fitting: &[usize]) -> Vec<String> {
        let candidates = self.overloads.get(&node).cloned().unwrap_or_default();
        let name = self.names_of(node);
        candidates
            .iter()
            .map(|&u| {
                let x = hir::Ident { unique: Some(u), text: name.clone() };
                let ty = ctx.lookup(&x).map(|(_, ty, _)| self.quote(ctx.lvl, ty).to_string()).unwrap_or_else(|| "?".into());
                let written = self.written_as.get(&u).cloned().unwrap_or_else(|| name.clone());
                let from = self.origin.get(&u).map_or(String::new(), |o| format!(", from {o}"));
                let mark = if fitting.contains(&u) { "  (fits)" } else { "" };
                format!("{written} : {ty}{from}{mark}")
            })
            .collect()
    }
}
