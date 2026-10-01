//! Strict positivity: a data type may appear in its own constructors' fields
//! only to the right of every arrow.
//!
//! Without this, a data type can loop with no recursion in sight.  From
//! `data Bad = MkBad (Bad -> Int)` one writes `\b -> case b of MkBad f -> f b`
//! and applies it to itself wrapped in `MkBad`: the same trick as `Type : Type`,
//! and just as fatal to the promise that a term with an empty effect row
//! terminates.  So this sits beside the universe hierarchy in phase 0 of the
//! roadmap.
//!
//! The rule, for the data types being defined (one strongly connected group
//! of declarations at a time, so that mutually recursive types are checked
//! together and each group can rely on the verdicts for the groups it
//! mentions):
//!
//! * a field that does not mention them is fine;
//! * in a function type, they may not appear in the domain at all, and the
//!   codomain must be positive in turn;
//! * applied to arguments, they may not appear in those arguments (`T (T a)`);
//! * inside another data type `D`, they may appear only in arguments for
//!   parameters that `D` itself uses strictly positively, so a rose tree
//!   `Node a (List (Rose a))` is fine, and positivity is compositional;
//! * anywhere else — under a type variable, `f (T a)`, where `f` could be
//!   anything — they are rejected.
//!
//! A parameter's polarity is decided by the same test, treating the
//! parameter as what is being looked for.  Within a group the answer is the
//! greatest fixed point: every parameter starts positive and is demoted until
//! nothing changes.

use super::core::Lvl;
use super::value::{vvar, Elim, Spine, Value};
use super::{Checker, Positivity, TypeErrorKind};
use idyllic_hir::NodeId;
use std::collections::HashSet;
use std::rc::Rc;

/// A constructor field, as positivity sees it: its type, and the size of the
/// context it was elaborated in (the parameters, then the fields before it).
pub(super) struct Field {
    pub node: NodeId,
    pub ctor: String,
    pub lvl: Lvl,
    pub ty: Rc<Value>,
}

/// A data type declared in this module.
pub(super) struct DataShape {
    pub unique: usize,
    /// the level of its first parameter, in its fields' contexts
    pub params_at: Lvl,
    pub params: usize,
    pub fields: Vec<Field>,
}

/// What an occurrence is being looked for.
enum Target<'a> {
    /// These data types: one group being checked.
    Datas(&'a HashSet<usize>),
    /// A parameter, by its level, when deciding the parameter's polarity.
    Param(Lvl),
}

impl Target<'_> {
    fn is_data(&self, u: usize) -> bool {
        matches!(self, Target::Datas(set) if set.contains(&u))
    }

    fn is_param(&self, k: Lvl) -> bool {
        matches!(self, Target::Param(p) if *p == k)
    }
}

/// The offending name, and what is wrong with where it appears.
type Violation = (String, Positivity);

impl Checker {
    /// Check every data type in the module for strict positivity, reporting
    /// each field that fails, and record each type's parameter polarities
    /// for the types that mention it.
    pub(super) fn check_positivity(&mut self, shapes: &[DataShape]) {
        let index_of: std::collections::HashMap<usize, usize> =
            shapes.iter().enumerate().map(|(i, d)| (d.unique, i)).collect();
        let all: HashSet<usize> = index_of.keys().copied().collect();
        let edges: Vec<Vec<usize>> = shapes
            .iter()
            .map(|d| {
                let mut out = Vec::new();
                for f in &d.fields {
                    self.data_heads(f.lvl, &f.ty, &all, &mut out);
                }
                out.iter().filter_map(|u| index_of.get(u).copied()).collect()
            })
            .collect();
        for group in strongly_connected(&edges) {
            let members: HashSet<usize> = group.iter().map(|&i| shapes[i].unique).collect();
            let target = Target::Datas(&members);
            for &i in &group {
                for f in &shapes[i].fields {
                    if let Err((data, why)) = self.positive(f.lvl, &f.ty, &target) {
                        let kind = TypeErrorKind::NotStrictlyPositive { data, ctor: f.ctor.clone(), why };
                        self.error(f.node, kind);
                    }
                }
            }
            self.decide_polarities(shapes, &group);
        }
    }

    /// The parameters of a group of types that each type uses strictly
    /// positively: start with all of them, and demote until stable.
    fn decide_polarities(&mut self, shapes: &[DataShape], group: &[usize]) {
        for &i in group {
            self.positive_params.insert(shapes[i].unique, vec![true; shapes[i].params]);
        }
        loop {
            let mut changed = false;
            for &i in group {
                let d = &shapes[i];
                for p in 0..d.params {
                    if !self.positive_params[&d.unique][p] {
                        continue;
                    }
                    let target = Target::Param(d.params_at + p);
                    if d.fields.iter().any(|f| self.positive(f.lvl, &f.ty, &target).is_err()) {
                        self.positive_params.get_mut(&d.unique).expect("inserted above")[p] = false;
                        changed = true;
                    }
                }
            }
            if !changed {
                return;
            }
        }
    }

    /// Whether `v`, a type in a context of size `l`, is strictly positive in
    /// `target`.
    fn positive(&self, l: Lvl, v: &Rc<Value>, target: &Target) -> Result<(), Violation> {
        let v = self.force(v.clone());
        let Some(found) = self.mentions(l, &v, target) else { return Ok(()) };
        match &*v {
            Value::Pi { dom, cod, .. } => {
                if let Some(name) = self.mentions(l, dom, target) {
                    return Err((name, Positivity::LeftOfArrow));
                }
                self.positive(l + 1, &self.apply(cod, vvar(l, "_")), target)
            }
            Value::BoxTy(_, a) | Value::Comp(a, _) => self.positive(l, a, target),
            Value::Opaque(u, _, sp) if target.is_data(*u) => self.not_in_arguments(l, sp, target),
            Value::Rigid(k, _, sp) if target.is_param(*k) => self.not_in_arguments(l, sp, target),
            Value::Opaque(u, outer, sp) if self.datas.contains_key(u) => {
                let polarity = self.positive_params.get(u);
                for (i, elim) in sp.iter().enumerate() {
                    let Elim::App(arg) = elim else { return Err((found, Positivity::Unknown)) };
                    if let Some(name) = self.mentions(l, arg, target) {
                        if !polarity.and_then(|p| p.get(i)).copied().unwrap_or(false) {
                            return Err((name, Positivity::Nested(outer.clone())));
                        }
                        self.positive(l, arg, target)?;
                    }
                }
                Ok(())
            }
            _ => Err((found, Positivity::Unknown)),
        }
    }

    /// At an occurrence of what is being defined: its own arguments may not
    /// mention it again.
    fn not_in_arguments(&self, l: Lvl, sp: &Spine, target: &Target) -> Result<(), Violation> {
        for elim in sp {
            let found = match elim {
                Elim::App(arg) => self.mentions(l, arg, target),
                _ => None,
            };
            if let Some(name) = found {
                return Err((name, Positivity::OwnArgument));
            }
        }
        Ok(())
    }

    /// The first occurrence of `target` in `v`, by name, looking everywhere.
    fn mentions(&self, l: Lvl, v: &Rc<Value>, target: &Target) -> Option<String> {
        let v = self.force(v.clone());
        match &*v {
            Value::Type(_)
            | Value::LitTy(_)
            | Value::Lit(_)
            | Value::Error
            | Value::Grade(_)
            | Value::GradeRange(..)
            | Value::Handle(..) => None,
            Value::Prim(_, args) => args.iter().find_map(|a| self.mentions(l, a, target)),
            Value::Stuck(h, sp) => self.mentions(l, h, target).or_else(|| self.spine_mentions(l, sp, target)),
            Value::Pi { dom, cod, .. } => self
                .mentions(l, dom, target)
                .or_else(|| self.mentions(l + 1, &self.apply(cod, vvar(l, "_")), target)),
            Value::Lam(_, body) => self.mentions(l + 1, &self.apply(body, vvar(l, "_")), target),
            Value::BoxTy(_, a) | Value::BoxIntro(a) => self.mentions(l, a, target),
            Value::Comp(a, e) => self.mentions(l, a, target).or_else(|| self.mentions(l, e, target)),
            Value::Row(r) => r.labels.iter().chain(&r.tails).find_map(|(_, a)| self.mentions(l, a, target)),
            Value::Rigid(k, name, _) if target.is_param(*k) => Some(name.clone()),
            Value::Opaque(u, name, _) if target.is_data(*u) => Some(name.clone()),
            Value::Flex(_, sp) | Value::Rigid(_, _, sp) | Value::Opaque(_, _, sp) => {
                self.spine_mentions(l, sp, target)
            }
        }
    }

    fn spine_mentions(&self, l: Lvl, sp: &Spine, target: &Target) -> Option<String> {
        sp.iter().find_map(|elim| match elim {
            Elim::App(a) => self.mentions(l, a, target),
            Elim::Unbox => None,
            Elim::Case(env, alts) => alts.iter().find_map(|alt| {
                let n = alt.vars.len();
                let inner = (0..n).fold(env.clone(), |e, i| e.push(vvar(l + i, "_")));
                self.mentions(l + n, &self.eval(&inner, &alt.body), target)
            }),
        })
    }

    /// Every data type among `among` that `v` mentions, anywhere.
    fn data_heads(&self, l: Lvl, v: &Rc<Value>, among: &HashSet<usize>, out: &mut Vec<usize>) {
        for &u in among {
            let one = HashSet::from([u]);
            if self.mentions(l, v, &Target::Datas(&one)).is_some() && !out.contains(&u) {
                out.push(u);
            }
        }
    }
}

/// Tarjan's algorithm.  Components come out dependencies first: a component
/// is emitted only after every component reachable from it.
fn strongly_connected(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    struct State<'a> {
        edges: &'a [Vec<usize>],
        next: usize,
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        out: Vec<Vec<usize>>,
    }

    fn visit(v: usize, s: &mut State) {
        s.index[v] = Some(s.next);
        s.low[v] = s.next;
        s.next += 1;
        s.stack.push(v);
        s.on_stack[v] = true;
        for &w in &s.edges[v] {
            match s.index[w] {
                None => {
                    visit(w, s);
                    s.low[v] = s.low[v].min(s.low[w]);
                }
                Some(iw) if s.on_stack[w] => s.low[v] = s.low[v].min(iw),
                Some(_) => {}
            }
        }
        if Some(s.low[v]) == s.index[v] {
            let mut component = Vec::new();
            loop {
                let w = s.stack.pop().expect("v is on the stack");
                s.on_stack[w] = false;
                component.push(w);
                if w == v {
                    break;
                }
            }
            component.reverse();
            s.out.push(component);
        }
    }

    let n = edges.len();
    let mut s = State {
        edges,
        next: 0,
        index: vec![None; n],
        low: vec![0; n],
        on_stack: vec![false; n],
        stack: Vec::new(),
        out: Vec::new(),
    };
    for v in 0..n {
        if s.index[v].is_none() {
            visit(v, &mut s);
        }
    }
    s.out
}
