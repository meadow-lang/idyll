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

use idyllic_hir::{Bind, BindGroup, Clause, DataDecl, GradeAnn, Module, PatKind, Pattern, Rhs, Term, TermKind};
use std::collections::{HashMap, HashSet};

/// Retopologize a whole module: its top-level groups, and every nested `let`
/// — in the trailing body and inside the declarations themselves.
pub fn scc_module(m: Module) -> Module {
    // (each top-level group apart: the prelude's definitions all come before
    // the program's)
    let groups = m.groups.into_iter().flat_map(|g| scc_groups(vec![g])).collect();
    let datas = order_datas(m.datas);
    Module { groups, datas, body: m.body.map(scc_term), ..m }
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
    let binds = groups
        .into_iter()
        .flat_map(BindGroup::into_binds)
        .map(|b| Bind { ty: b.ty.map(scc_term), rhs: scc_term(b.rhs), ..b })
        .collect();
    scc_binds(binds)
}

/// Rewrite every `let` in a term so its groups are dependency-ordered SCCs,
/// recursing into all subterms.
pub fn scc_term(t: Term) -> Term {
    use TermKind::*;
    let kind = match t.kind {
        k @ (Type | Hole | Omitted | Inf | Lit(_) | Var(_) | SeqNil) => k,
        App(f, a) => App(Box::new(scc_term(*f)), Box::new(scc_term(*a))),
        SeqCons(h, t) => SeqCons(Box::new(scc_term(*h)), Box::new(scc_term(*t))),
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

    // move the binds out by index, in the order the components were found
    let mut slots: Vec<Option<Bind>> = binds.into_iter().map(Some).collect();
    tarjan
        .components
        .into_iter()
        .map(|mut members| {
            members.sort_unstable();
            let self_edge = members.len() == 1 && edges[members[0]].contains(&members[0]);
            let mut bs: Vec<Bind> =
                members.iter().map(|&i| slots[i].take().expect("each bind is in one component")).collect();
            if bs.len() == 1 && !self_edge {
                BindGroup::NonRec(bs.pop().unwrap())
            } else {
                BindGroup::Rec(bs)
            }
        })
        .collect()
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
            Type | Hole | Omitted | Inf | Lit(_) | SeqNil => {}
            Var(i) => {
                if let Some(u) = i.unique {
                    out.insert(u);
                }
            }
            App(f, a) | Ann(f, a) | Range(f, a) | Eff(f, a) | SeqCons(f, a) => {
                go(f, out);
                go(a, out);
            }
            BoxIntro(a) => go(a, out),
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
            PatKind::As(_, q) => pattern(q, out),
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
