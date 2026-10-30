//! Strongly-connected-components pass over renamed bindings.
//!
//! Runs after name resolution and before type checking.  The renamer emits
//! every `let` / top-level group as a single conservative [`BindGroup::Rec`];
//! this pass rebuilds those groups from the actual dependency graph:
//!
//! * bindings that do not (transitively, within the group) refer to
//!   themselves become [`BindGroup::NonRec`];
//!
//! * mutually-recursive bindings are collected into one [`BindGroup::Rec`];
//!
//! * the resulting groups are ordered so that each group only refers to
//!   earlier groups (topological order) — this is the "retopologize" step.
//!
//! Node ids are preserved, so the span / type side tables stay valid.
//!
//! Data types are put in order too: the checker declares their kinds one
//! after another, so a kind may only mention the types before it (`Fin :
//! Nat -> Type` wants `Nat`), and the modules of a package come in no
//! particular order.

use idyllic_hir::{Bind, BindGroup, Clause, CtorShape, DataDecl, GradeAnn, Module, PatKind, Pattern, Rhs, Term, TermKind};
use std::collections::{HashMap, HashSet};

/// Retopologize a whole module: its top-level groups, and every nested `let`
/// — in the trailing body and inside the declarations themselves.
pub fn scc_module(m: Module) -> Module {
    // (each top-level group apart: the prelude's definitions all come before
    // the program's)
    // (and the hints before what may want them, which is everything that
    // they do not need themselves: a search for an auto argument finds only
    // what has been checked)
    // (so are a trait's superclass projections, which a search inside a
    // definition may go through without its naming them)
    let hints = m.hints.clone();
    let early: HashSet<usize> = hints.iter().copied().chain(m.supers.values().flatten().copied()).collect();
    let groups = m.groups.into_iter().flat_map(|g| scc_groups_with(vec![g], &hints, &early)).collect();
    let datas = order_datas(m.datas);
    let (datas, data_after, groups) = interleave(datas, groups);
    Module { groups, datas, data_after, body: m.body.map(scc_term), ..m }
}

/// What a data type's declaration mentions: its parameters' kinds, its
/// kind, and its constructors' types.
fn data_uniques(d: &DataDecl) -> HashSet<usize> {
    let mut out = HashSet::new();
    for kind in d.params.iter().filter_map(|(_, k)| k.as_ref()).chain(d.kind.as_ref()) {
        out.extend(free_uniques(kind));
    }
    for c in &d.ctors {
        match &c.shape {
            CtorShape::Fields(fields) => fields.iter().for_each(|f| out.extend(free_uniques(f))),
            CtorShape::Sig(ty) => out.extend(free_uniques(ty)),
        }
    }
    out
}

/// The data types and the groups in one order, each after what it
/// mentions: a data type before every group, as the checker declared them
/// all before, unless it mentions a definition (a type alias, a type
/// family, a trait's associated type), when it comes after that
/// definition's group, and every group that mentions it (its type or a
/// constructor) after it. Otherwise everything keeps its place. What
/// mentions itself through both (a data type and a definition that need
/// each other) stays as it was, for the checker to report. The data types
/// in their new order, how many groups come before each, and the groups.
fn interleave(datas: Vec<DataDecl>, groups: Vec<BindGroup>) -> (Vec<DataDecl>, Vec<usize>, Vec<BindGroup>) {
    let n_groups = groups.len();
    let group_of: HashMap<usize, usize> = groups
        .iter()
        .enumerate()
        .flat_map(|(g, grp)| grp.binds().iter().filter_map(|b| b.name.unique).map(move |u| (u, g)).collect::<Vec<_>>())
        .collect();
    let mentions: Vec<HashSet<usize>> = datas.iter().map(data_uniques).collect();
    if !mentions.iter().any(|m| m.iter().any(|u| group_of.contains_key(u))) {
        let after = vec![0; datas.len()];
        return (datas, after, groups);
    }
    // each data type's uniques: its own and its constructors'
    let data_of: HashMap<usize, usize> = datas
        .iter()
        .enumerate()
        .flat_map(|(i, d)| std::iter::once(&d.name).chain(d.ctors.iter().map(|c| &c.name)).filter_map(|x| x.unique).map(move |u| (u, i)))
        .collect();
    // nodes: the data types, then the groups; an edge to what must come first
    let n = datas.len() + n_groups;
    let mut needs: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    for (i, m) in mentions.iter().enumerate() {
        for u in m {
            if let Some(&g) = group_of.get(u) {
                needs[i].insert(datas.len() + g);
            }
            if let Some(&j) = data_of.get(u).filter(|&&j| j != i) {
                needs[i].insert(j);
            }
        }
    }
    for (g, grp) in groups.iter().enumerate() {
        for b in grp.binds() {
            for u in free_uniques_bind(b) {
                if let Some(&h) = group_of.get(&u).filter(|&&h| h != g) {
                    needs[datas.len() + g].insert(datas.len() + h);
                }
                if let Some(&j) = data_of.get(&u) {
                    needs[datas.len() + g].insert(j);
                }
            }
        }
    }
    // where each would go by itself: a data type before the group after the
    // last it mentions, a group where it is
    let mut place: Vec<usize> = (0..n).map(|v| if v < datas.len() { 0 } else { 2 * (v - datas.len()) + 1 }).collect();
    for (i, m) in mentions.iter().enumerate() {
        place[i] = m.iter().filter_map(|u| group_of.get(u)).map(|&g| 2 * (g + 1)).max().unwrap_or(0);
    }
    // whatever is ready, the one whose place is first
    let mut done = vec![false; n];
    let mut order = Vec::with_capacity(n);
    while order.len() < n {
        let ready = (0..n).filter(|&v| !done[v] && needs[v].iter().all(|&w| done[w])).min_by_key(|&v| (place[v], v));
        // (a cycle: what is left, in place order)
        let next = ready.unwrap_or_else(|| (0..n).filter(|&v| !done[v]).min_by_key(|&v| (place[v], v)).expect("something left"));
        done[next] = true;
        order.push(next);
    }
    let (mut data_slots, mut group_slots): (Vec<Option<DataDecl>>, Vec<Option<BindGroup>>) =
        (datas.into_iter().map(Some).collect(), groups.into_iter().map(Some).collect());
    let (mut out_datas, mut after, mut out_groups) = (Vec::new(), Vec::new(), Vec::new());
    for v in order {
        if v < data_slots.len() {
            out_datas.push(data_slots[v].take().expect("each data type once"));
            after.push(out_groups.len());
        } else {
            out_groups.push(group_slots[v - data_slots.len()].take().expect("each group once"));
        }
    }
    (out_datas, after, out_groups)
}

/// Data types, each after the ones its parameters' kinds and its own kind
/// mention, and otherwise in the order they came.  (Its constructors may
/// mention any of them, itself included: every kind is declared before any
/// constructor is checked.)
fn order_datas(datas: Vec<DataDecl>) -> Vec<DataDecl> {
    let index: HashMap<usize, usize> = datas.iter().enumerate().filter_map(|(i, d)| Some((d.name.unique?, i))).collect();
    let needs: Vec<Vec<usize>> = datas
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let mut mentioned = HashSet::new();
            for kind in d.params.iter().filter_map(|(_, k)| k.as_ref()).chain(d.kind.as_ref()) {
                mentioned.extend(free_uniques(kind));
            }
            let mut before: Vec<usize> = mentioned.iter().filter_map(|u| index.get(u).copied()).filter(|&j| j != i).collect();
            before.sort_unstable();
            before
        })
        .collect();
    fn visit(i: usize, needs: &[Vec<usize>], seen: &mut [bool], order: &mut Vec<usize>) {
        if seen[i] {
            return;
        }
        seen[i] = true;
        for &j in &needs[i] {
            visit(j, needs, seen, order);
        }
        order.push(i);
    }
    let mut seen = vec![false; datas.len()];
    let mut order = Vec::new();
    for i in 0..datas.len() {
        visit(i, &needs, &mut seen, &mut order);
    }
    let mut slots: Vec<Option<DataDecl>> = datas.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots[i].take()).collect()
}

/// Regroup a `let`'s bindings, first doing the same for any `let` nested in
/// their signatures and right-hand sides.  (Skipping that descent leaves the
/// renamer's conservative recursive group in place, and a plain nested
/// `let x = 1` is then wrongly told it needs a signature.)
fn scc_groups(groups: Vec<BindGroup>) -> Vec<BindGroup> {
    scc_groups_with(groups, &HashSet::new(), &HashSet::new())
}

fn scc_groups_with(groups: Vec<BindGroup>, hints: &HashSet<usize>, early: &HashSet<usize>) -> Vec<BindGroup> {
    let binds = groups
        .into_iter()
        .flat_map(BindGroup::into_binds)
        .map(|b| Bind { ty: b.ty.map(scc_term), rhs: scc_term(b.rhs), ..b })
        .collect();
    scc_binds_with(binds, hints, early)
}

/// Rewrite every `let` in a term so its groups are dependency-ordered SCCs,
/// recursing into all subterms.
pub fn scc_term(t: Term) -> Term {
    use TermKind::*;
    let kind = match t.kind {
        k @ (Type | Hole | Goal(_) | Omitted | Inf | Lit(_) | Var(_) | SeqNil) => k,
        App(f, a) => App(Box::new(scc_term(*f)), Box::new(scc_term(*a))),
        ImplicitApp(f, a) => ImplicitApp(Box::new(scc_term(*f)), Box::new(scc_term(*a))),
        AutoApp(f, a) => AutoApp(Box::new(scc_term(*f)), Box::new(scc_term(*a))),
        Search => Search,
        SeqCons(h, t) => SeqCons(Box::new(scc_term(*h)), Box::new(scc_term(*t))),
        ArrayLit(items) => ArrayLit(items.into_iter().map(scc_term).collect()),
        Ann(e, a) => Ann(Box::new(scc_term(*e)), Box::new(scc_term(*a))),
        Range(lo, hi) => Range(Box::new(scc_term(*lo)), Box::new(scc_term(*hi))),
        Pi(x, icit, g, dom, cod) => {
            Pi(x, icit, g.map(scc_grades), Box::new(scc_term(*dom)), Box::new(scc_term(*cod)))
        }
        Lam(x, g, ty, body) => {
            Lam(x, g.map(scc_grades), ty.map(|t| Box::new(scc_term(*t))), Box::new(scc_term(*body)))
        }
        BoxIntro(e) => BoxIntro(Box::new(scc_term(*e))),
        LetBox(x, e, body) => LetBox(x, Box::new(scc_term(*e)), Box::new(scc_term(*body))),
        Let(groups, body) => Let(scc_groups(groups), Box::new(scc_term(*body))),
        Case(scrutinee, clauses) => Case(Box::new(scc_term(*scrutinee)), clauses.into_iter().map(scc_clause).collect()),
        Match(clauses) => Match(clauses.into_iter().map(scc_clause).collect()),
        Eff(t, r) => Eff(Box::new(scc_term(*t)), Box::new(scc_term(*r))),
        Row(labels, tail) => Row(labels.into_iter().map(scc_term).collect(), tail.map(|t| Box::new(scc_term(*t)))),
        Handle(body, clauses) => Handle(
            Box::new(scc_term(*body)),
            clauses
                .into_iter()
                .map(|c| idyllic_hir::HandleClause { body: scc_term(c.body), ..c })
                .collect(),
        ),
    };
    Term { id: t.id, kind }
}

fn scc_clause(c: Clause) -> Clause {
    let rhs = match c.rhs {
        Rhs::Plain(e) => Rhs::Plain(scc_term(e)),
        Rhs::Guarded(gs) => Rhs::Guarded(gs.into_iter().map(|(g, e)| (scc_term(g), scc_term(e))).collect()),
        Rhs::Impossible => Rhs::Impossible,
    };
    Clause { pats: c.pats.into_iter().map(scc_pattern).collect(), rhs, ..c }
}

fn scc_pattern(p: Pattern) -> Pattern {
    let kind = match p.kind {
        PatKind::Con(c, ps) => PatKind::Con(c, ps.into_iter().map(scc_pattern).collect()),
        PatKind::SeqCons(ps) => PatKind::SeqCons(ps.into_iter().map(scc_pattern).collect()),
        PatKind::As(x, q) => PatKind::As(x, Box::new(scc_pattern(*q))),
        PatKind::Dot(e) => PatKind::Dot(scc_term(e)),
        PatKind::Implicit(icit, q) => PatKind::Implicit(icit, Box::new(scc_pattern(*q))),
        k @ (PatKind::Var(_) | PatKind::Wild | PatKind::Lit(_) | PatKind::SeqNil) => k,
    };
    Pattern { id: p.id, kind }
}

fn scc_grades(g: GradeAnn) -> GradeAnn {
    GradeAnn {
        subject: Box::new(scc_term(*g.subject)),
        in_type: g.in_type.map(|r| Box::new(scc_term(*r))),
    }
}

/// Split a mutually-recursive bind list into dependency-ordered groups.
///
/// Tarjan's algorithm emits a component only once everything reachable from
/// it has been emitted, so with edges pointing from a binding to what it
/// uses, components come out dependencies-first — the order type checking
/// wants.  Members of a component keep their source order.
pub fn scc_binds(binds: Vec<Bind>) -> Vec<BindGroup> {
    scc_binds_with(binds, &HashSet::new(), &HashSet::new())
}

/// The same, with each of `hints` before every binding it does not itself
/// lead to.
fn scc_binds_with(binds: Vec<Bind>, hints: &HashSet<usize>, early: &HashSet<usize>) -> Vec<BindGroup> {
    let index_of: HashMap<usize, usize> = binds
        .iter()
        .enumerate()
        .filter_map(|(i, b)| b.name.unique.map(|u| (u, i)))
        .collect();
    let edges: Vec<Vec<usize>> = binds
        .iter()
        .map(|b| {
            let mut deps: Vec<usize> = free_uniques_bind(b)
                .into_iter()
                .filter_map(|u| index_of.get(&u).copied())
                .collect();
            deps.sort_unstable();
            deps
        })
        .collect();
    let mut tarjan = Tarjan::new(binds.len());
    for v in 0..binds.len() {
        if tarjan.index[v].is_none() {
            tarjan.visit(v, &edges);
        }
    }
    let components = early_first(&binds, &edges, tarjan.components, early, hints);

    // move the binds out by index, in the order the components were found
    let mut slots: Vec<Option<Bind>> = binds.into_iter().map(Some).collect();
    components
        .into_iter()
        .map(|mut members| {
            members.sort_unstable();
            let self_edge = members.len() == 1 && edges[members[0]].contains(&members[0]);
            let mut bs: Vec<Bind> =
                members.iter().map(|&i| slots[i].take().expect("each bind is in one component")).collect();
            // A definition by matching, with a signature, is a recursive one
            // even if it never
            // calls itself: a constant that unfolds only when its case tree
            // can choose (see the checker's termination module), as Agda's
            // and Idris's do, so that a type mentioning `first p` says so
            // rather than the `case` it would be.
            // (One with no signature stays as it is: a recursive definition's
            // type is not inferred.)
            let by_matching = bs.len() == 1 && bs[0].ty.is_some() && matches!(bs[0].rhs.kind, TermKind::Match(_));
            // A hint, with its signature, is a recursive one too: an
            // instance's methods find the instance itself, as what a
            // recursive call at its type wants.
            let hint = bs.len() == 1 && bs[0].ty.is_some() && bs[0].name.unique.is_some_and(|u| hints.contains(&u));
            if bs.len() == 1 && !self_edge && !by_matching && !hint {
                BindGroup::NonRec(bs.pop().unwrap())
            } else {
                BindGroup::Rec(bs)
            }
        })
        .collect()
}

/// The components, dependencies first as Tarjan's algorithm finds them, with
/// each holding one of `early` (a hint, a superclass projection) and what
/// it needs moved as far forward as they can go: the projections first, in
/// the order they come, then the hints (a search for a hint's own auto
/// arguments may go through a projection it does not name), since a
/// search finds only what has been checked, and need not be named.
fn early_first(
    binds: &[Bind],
    edges: &[Vec<usize>],
    components: Vec<Vec<usize>>,
    early: &HashSet<usize>,
    hints: &HashSet<usize>,
) -> Vec<Vec<usize>> {
    let is_early = |i: usize| binds[i].name.unique.is_some_and(|u| early.contains(&u));
    if !(0..binds.len()).any(is_early) {
        return components;
    }
    let mut component_of = vec![0; binds.len()];
    for (c, members) in components.iter().enumerate() {
        for &v in members {
            component_of[v] = c;
        }
    }
    fn emit(c: usize, components: &[Vec<usize>], edges: &[Vec<usize>], component_of: &[usize], done: &mut [bool], order: &mut Vec<usize>) {
        if std::mem::replace(&mut done[c], true) {
            return;
        }
        for &v in &components[c] {
            for &w in &edges[v] {
                emit(component_of[w], components, edges, component_of, done, order);
            }
        }
        order.push(c);
    }
    let mut done = vec![false; components.len()];
    let mut order = Vec::new();
    let mut firsts: Vec<usize> = (0..binds.len()).filter(|&i| is_early(i)).collect();
    let is_hint = |i: usize| binds[i].name.unique.is_some_and(|u| hints.contains(&u));
    firsts.sort_unstable_by_key(|&i| (is_hint(i), i));
    for v in firsts {
        emit(component_of[v], &components, edges, &component_of, &mut done, &mut order);
    }
    for c in 0..components.len() {
        emit(c, &components, edges, &component_of, &mut done, &mut order);
    }
    let mut slots: Vec<Option<Vec<usize>>> = components.into_iter().map(Some).collect();
    order.into_iter().filter_map(|c| slots[c].take()).collect()
}

struct Tarjan {
    index: Vec<Option<usize>>,
    lowlink: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    next_index: usize,
    components: Vec<Vec<usize>>,
}

impl Tarjan {
    fn new(n: usize) -> Self {
        Tarjan {
            index: vec![None; n],
            lowlink: vec![0; n],
            on_stack: vec![false; n],
            stack: Vec::new(),
            next_index: 0,
            components: Vec::new(),
        }
    }

    fn visit(&mut self, v: usize, edges: &[Vec<usize>]) {
        self.index[v] = Some(self.next_index);
        self.lowlink[v] = self.next_index;
        self.next_index += 1;
        self.stack.push(v);
        self.on_stack[v] = true;
        for &w in &edges[v] {
            match self.index[w] {
                None => {
                    self.visit(w, edges);
                    self.lowlink[v] = self.lowlink[v].min(self.lowlink[w]);
                }
                Some(wi) if self.on_stack[w] => {
                    self.lowlink[v] = self.lowlink[v].min(wi);
                }
                Some(_) => {}
            }
        }
        if Some(self.lowlink[v]) == self.index[v] {
            let mut component = Vec::new();
            loop {
                let w = self.stack.pop().expect("root is on the stack");
                self.on_stack[w] = false;
                component.push(w);
                if w == v {
                    break;
                }
            }
            self.components.push(component);
        }
    }
}

/// Free name uniques of a binding: those of its optional type annotation and
/// its right-hand side.  The binding's own name counts as free if the rhs
/// mentions it, which is exactly what makes direct recursion a self-edge.
fn free_uniques_bind(b: &Bind) -> HashSet<usize> {
    let mut s = free_uniques(&b.rhs);
    if let Some(ty) = &b.ty {
        s.extend(free_uniques(ty));
    }
    s
}

/// Free name uniques of a term (uses of names not bound within it).
pub fn free_uniques(t: &Term) -> HashSet<usize> {
    fn go(t: &Term, out: &mut HashSet<usize>) {
        use TermKind::*;
        match &t.kind {
            Type | Hole | Goal(_) | Omitted | Inf | Lit(_) | SeqNil | Search => {}
            Var(i) => {
                if let Some(u) = i.unique {
                    out.insert(u);
                }
            }
            App(f, a) | ImplicitApp(f, a) | AutoApp(f, a) | Ann(f, a) | Range(f, a) | Eff(f, a) | SeqCons(f, a) => {
                go(f, out);
                go(a, out);
            }
            BoxIntro(a) => go(a, out),
            ArrayLit(items) => items.iter().for_each(|e| go(e, out)),
            LetBox(x, e, body) => {
                go(e, out);
                let mut inner = HashSet::new();
                go(body, &mut inner);
                if let Some(u) = x.unique {
                    inner.remove(&u);
                }
                out.extend(inner);
            }
            Pi(mb, _, g, dom, cod) => {
                grades(g, out);
                go(dom, out);
                let mut inner = HashSet::new();
                go(cod, &mut inner);
                if let Some(u) = mb.as_ref().and_then(|x| x.unique) {
                    inner.remove(&u);
                }
                out.extend(inner);
            }
            Lam(x, g, ty, body) => {
                grades(g, out);
                if let Some(ty) = ty {
                    go(ty, out);
                }
                let mut inner = HashSet::new();
                go(body, &mut inner);
                if let Some(u) = x.unique {
                    inner.remove(&u);
                }
                out.extend(inner);
            }
            // names are unique, so what a clause binds cannot be mistaken
            // for anything outside it
            Case(scrutinee, clauses) => {
                go(scrutinee, out);
                clauses.iter().for_each(|c| clause(c, out));
            }
            Match(clauses) => clauses.iter().for_each(|c| clause(c, out)),
            Handle(body, clauses) => {
                go(body, out);
                for c in clauses {
                    out.extend(c.op.as_ref().and_then(|op| op.unique));
                    go(&c.body, out);
                }
            }
            Row(labels, tail) => {
                labels.iter().for_each(|l| go(l, out));
                if let Some(t) = tail {
                    go(t, out);
                }
            }
            Let(groups, body) => {
                let mut inner = HashSet::new();
                for b in groups.iter().flat_map(BindGroup::binds) {
                    inner.extend(free_uniques_bind(b));
                }
                go(body, &mut inner);
                for b in groups.iter().flat_map(BindGroup::binds) {
                    if let Some(u) = b.name.unique {
                        inner.remove(&u);
                    }
                }
                out.extend(inner);
            }
        }
    }
    fn clause(c: &Clause, out: &mut HashSet<usize>) {
        c.pats.iter().for_each(|p| pattern(p, out));
        match &c.rhs {
            Rhs::Plain(e) => go(e, out),
            Rhs::Guarded(gs) => gs.iter().for_each(|(g, e)| {
                go(g, out);
                go(e, out);
            }),
            Rhs::Impossible => {}
        }
    }
    fn pattern(p: &Pattern, out: &mut HashSet<usize>) {
        match &p.kind {
            PatKind::Con(c, ps) => {
                out.extend(c.unique);
                ps.iter().for_each(|p| pattern(p, out));
            }
            PatKind::SeqCons(ps) => ps.iter().for_each(|p| pattern(p, out)),
            PatKind::As(_, q) | PatKind::Implicit(_, q) => pattern(q, out),
            PatKind::Dot(e) => go(e, out),
            PatKind::Var(_) | PatKind::Wild | PatKind::Lit(_) | PatKind::SeqNil => {}
        }
    }
    fn grades(g: &Option<GradeAnn>, out: &mut HashSet<usize>) {
        if let Some(g) = g {
            go(&g.subject, out);
            if let Some(r) = &g.in_type {
                go(r, out);
            }
        }
    }
    let mut out = HashSet::new();
    go(t, &mut out);
    out
}
