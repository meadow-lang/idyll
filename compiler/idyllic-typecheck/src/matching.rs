//! The match compiler: equations and `case` alternatives, elaborated to case
//! trees, with dependent matching.
//!
//! This follows Cockx & Abel, *Elaborating Dependent (Co)pattern Matching*
//! (ICFP 2018), without copatterns.  A definition by equations is a list of
//! *problems*, one per equation: patterns waiting to be matched against the
//! arguments.  The first problem decides what happens next:
//!
//! * while it has patterns left over, the goal must be a function type, and
//!   its argument is bound, for every problem's next pattern to be matched
//!   against (**intro**);
//! * once each of its patterns is either a variable (a name for what it is
//!   matched against) or a constructor or literal that agrees with what it is
//!   matched against, it has won: its right-hand side is checked, with its
//!   names bound (**done**);
//! * a constructor that disagrees means the equation does not apply here,
//!   and it is dropped;
//! * a constructor or literal matched against a *variable* is where the tree
//!   branches (**split**).  For each constructor of the variable's type,
//!   the constructor's arguments become new variables, and the variable is
//!   unified with the constructor applied to them — and the type's indices
//!   with the constructor's.
//!
//! That unification is what makes matching dependent.  Its variables are the
//! context's: solving one *refines* the context, defining the variable as
//! its solution everywhere, in the goal and in the types of other variables
//! alike.  Matching `xs : Vec n a` against `Cons x rest` defines `n` as
//! `S m` for a new `m`, so the goal `Vec (n + k) a` becomes `Vec (S m + k) a`.
//! Where unification finds two different constructors, the case cannot
//! happen, and needs no alternative: `head : Vec (S n) a -> a` has no `Nil`
//! case to write.  Where it finds neither agreement nor disagreement (two
//! stuck function calls, say), that is reported.
//!
//! A variable split on is used once, like any scrutinee, and the variables
//! a constructor's arguments become have that argument's grade: the fields
//! of a data type are linear, its erased arguments erased.  A name in a
//! pattern is another name for the variable it matched, not a new variable,
//! so its uses are that variable's.
//!
//! When no problem is left for some branch, that case is missing, and is
//! reported as a pattern; an equation that no branch ever reaches is
//! reported as unreachable.  Axiom K holds: a variable may be unified with
//! itself, and that is simply dropped.

use super::core::{CaseAlt, Core, Head, Lvl, Note};
use super::value::{vvar, Closure, Elim, Env, Poly, VTy, Value};
use super::{BinderInfo, Checker, Ctx, TypeErrorKind, Usage};
use crate::grade::Grade;
use idyllic_ast::{Icit, Lit};
use idyllic_builtins::{Builtin, Prim};
use idyllic_hir::{self as hir, NodeId, PatKind, TermKind};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

/// What a pattern is matched against.
#[derive(Clone)]
enum Target {
    /// A slot of the context, by level: an argument, or a constructor's
    /// argument that matching bound.
    Slot(Lvl),
    /// A part of a value that unification worked out, with its type: the
    /// `k` in `S k`, when a variable was found to be `S (S j)`.  A name
    /// bound to one is erased: it is known only to the types.
    Value(Rc<Value>, VTy),
}

/// One equation (or alternative) on its way through the match compiler.
#[derive(Clone)]
struct Problem<'a> {
    clause: &'a hir::Clause,
    /// its position among its siblings
    index: usize,
    /// patterns for arguments not bound yet
    pending: &'a [hir::Pattern],
    /// patterns and what they are matched against, not yet settled
    constraints: Vec<(&'a hir::Pattern, Target)>,
    /// names, settled: the pattern they come from, and what they name
    bindings: Vec<(NodeId, &'a hir::Ident, Target)>,
    /// dot patterns, to check once the case is known
    dots: Vec<(&'a hir::Term, Target)>,
}

impl<'a> Problem<'a> {
    fn new(clause: &'a hir::Clause, index: usize, pending: &'a [hir::Pattern], constraints: Vec<(&'a hir::Pattern, Target)>) -> Self {
        Problem { clause, index, pending, constraints, bindings: Vec::new(), dots: Vec::new() }
    }
}

/// What one match elaboration has found out along the way.
struct Matcher {
    /// the `case` or the function, for errors about the whole
    node: NodeId,
    /// which clauses some case reaches
    used: Vec<bool>,
    /// the cases no clause covers, as patterns
    missing: Vec<String>,
    /// the slots of what is matched, in order, to describe a case by
    args: Vec<Lvl>,
    /// and their types, as they were before any matching
    arg_types: Vec<VTy>,
    /// the slots matching bound that no pattern named
    unnamed: std::collections::HashSet<Lvl>,
    /// Something was wrong, and has been reported.  Coverage and usage then
    /// say nothing: the missing cases would be the ones the error hid.
    failed: bool,
}

/// What to do with the first problem.
enum Step {
    /// every pattern settled: this is the case
    Done,
    /// a constructor disagrees: not this equation
    Fails,
    /// a constructor or literal waits on the variable in this slot
    Split(Lvl, Split),
    /// an error, reported
    Failed,
}

enum Split {
    /// into the constructors of its type (a data type, or `Bool`)
    Con,
    /// on whether it is this number
    Lit(Lit),
}

/// The result of unifying a constructor's indices with a variable's.
enum Unified {
    /// possible; the context with what was learned, and the substitution
    /// that learned it (none if nothing was), as an environment
    Refined(Ctx, Option<Env>),
    /// not possible: two different constructors
    Absurd,
    /// cannot tell
    Stuck(Rc<Value>, Rc<Value>),
}

/// A constructor's arguments, bound as new variables for one alternative:
/// each one's slot, usage key, name and grade.
type Telescope = Vec<(Lvl, usize, String, Rc<Value>)>;

impl Checker {
    /// A function defined by equations, against its type.
    pub(super) fn elab_match(&mut self, ctx: &Ctx, nid: NodeId, clauses: &[hir::Clause], goal: VTy) -> (Rc<Core>, Usage) {
        let arity = clauses.first().map_or(0, |c| c.pats.len());
        let mut problems = Vec::new();
        for (index, clause) in clauses.iter().enumerate() {
            if clause.pats.len() == arity {
                problems.push(Problem::new(clause, index, &clause.pats, Vec::new()));
            } else {
                self.error(clause.id, TypeErrorKind::ClauseArity { expected: arity, given: clause.pats.len() });
            }
        }
        // without the equation that was dropped, coverage would be wrong
        let failed = problems.len() < clauses.len();
        self.run_match(ctx, nid, clauses, problems, goal, Vec::new(), failed)
    }

    /// A function defined by equations, with no signature: its type is
    /// worked out from them, starting from nothing but how many arguments
    /// it takes.
    pub(super) fn infer_match(&mut self, ctx: &Ctx, nid: NodeId, clauses: &[hir::Clause]) -> (Rc<Core>, VTy, Usage) {
        let arity = clauses.first().map_or(0, |c| c.pats.len());
        let mut under = ctx.clone();
        let mut doms = Vec::new();
        for _ in 0..arity {
            let d = self.fresh_meta(&under, nid, "the type of this argument");
            let dv = self.eval(&under.env, &d);
            doms.push(d);
            under = under.bind(None, "x", dv);
        }
        let cod = self.fresh_meta(&under, nid, "the result type of this function");
        let effects = self.fresh_meta(&under, nid, "the effects of this function");
        let cod = Rc::new(Core::Comp(cod, effects));
        // (unnamed: an argument matched against a pattern has no name of
        // its own, and the result type does not mention it)
        let ty = doms.into_iter().rev().fold(cod, |acc, dom| {
            Rc::new(Core::Pi {
                name: "_".into(),
                icit: Icit::Explicit,
                grade: Core::grade(Grade::DEFAULT),
                in_type: Core::grade(Grade::DEFAULT_IN_TYPE),
                dom,
                cod: acc,
            })
        });
        let ty = self.eval(&ctx.env, &ty);
        let (core, usage) = self.elab_match(ctx, nid, clauses, ty.clone());
        (core, ty, usage)
    }

    /// `case scrutinee of clauses`, against `expected` if there is one.
    ///
    /// A variable is matched where it is, so the match refines it: in each
    /// alternative, the variable *is* that alternative's constructor, in the
    /// goal and everywhere else.  Anything else is bound to a variable of
    /// its own first, which nothing else mentions.
    pub(super) fn case_of(
        &mut self,
        ctx: &Ctx,
        nid: NodeId,
        scrutinee: &hir::Term,
        clauses: &[hir::Clause],
        expected: Option<VTy>,
    ) -> (Rc<Core>, VTy, Usage) {
        let (s_c, s_ty, s_usage) = self.infer(ctx, scrutinee);
        let (s_c, s_ty) = self.insert(ctx, scrutinee.id, s_c, s_ty);
        if matches!(*self.force(s_ty.clone()), Value::Error) {
            return (Rc::new(Core::Error), Rc::new(Value::Error), Usage::unknown());
        }
        let goal = match expected {
            Some(ty) => ty,
            None => {
                let c = self.fresh_meta(ctx, nid, "the type of this case");
                self.eval(&ctx.env, &c)
            }
        };
        let problems = |slot: Lvl| -> Vec<Problem<'_>> {
            clauses
                .iter()
                .enumerate()
                .map(|(i, c)| Problem::new(c, i, &[], c.pats.iter().map(|p| (p, Target::Slot(slot))).collect()))
                .collect()
        };
        // `if 1 then ..`: say so about the condition, once
        let bool_pattern =
            |c: &hir::Clause| matches!(c.pats.first().map(|p| &peel(p).kind), Some(PatKind::Lit(Lit::Bool(_))));
        if clauses.iter().any(bool_pattern) {
            let want = Rc::new(Value::LitTy(Builtin::Bool));
            if !self.unify(ctx.lvl, s_ty.clone(), want.clone()) {
                let (expected, actual) = (self.quote(ctx.lvl, want), self.quote(ctx.lvl, s_ty));
                self.error(scrutinee.id, TypeErrorKind::Mismatch { expected, actual });
                self.abandon(&goal);
                return (Rc::new(Core::Error), Rc::new(Value::Error), Usage::unknown());
            }
        }
        let variable = match &scrutinee.kind {
            TermKind::Var(x) => ctx.bound_slot(x),
            _ => None,
        };
        match variable {
            // its use is the split's, if there is one
            Some(k) => {
                let (core, usage) = self.run_match(ctx, nid, clauses, problems(k), goal.clone(), vec![k], false);
                (core, goal, usage)
            }
            None => {
                let key = self.fresh_key();
                // a value with nothing inside (a truth value, a number, an
                // enumeration's) is looked at once, which is all a match
                // does with it; one with fields gives them the default grade
                let grade = self.fieldless(&s_ty).then(|| Poly::constant(Grade::ONE).into_value());
                let ctx2 = ctx.bind_keyed(key, "_", s_ty, grade);
                let (core, mut usage) =
                    self.run_match(&ctx2, nid, clauses, problems(ctx.lvl), goal.clone(), vec![ctx.lvl], false);
                // what the scrutinee uses, as often as its variable is used
                let n = usage.take(Some(key));
                (Rc::new(Core::Let("_".into(), s_c, core)), goal, usage + s_usage.scale(&n))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_match(
        &mut self,
        ctx: &Ctx,
        nid: NodeId,
        clauses: &[hir::Clause],
        problems: Vec<Problem<'_>>,
        goal: VTy,
        args: Vec<Lvl>,
        failed: bool,
    ) -> (Rc<Core>, Usage) {
        let arg_types = args.iter().map(|&k| ctx.binders[k].ty.clone()).collect();
        let mut m = Matcher {
            node: nid,
            used: vec![false; clauses.len()],
            missing: Vec::new(),
            args,
            arg_types,
            unnamed: Default::default(),
            failed,
        };
        let (core, usage) = self.tree(&mut m, ctx, problems, goal.clone());
        if m.failed {
            self.abandon(&goal);
            return (core, Usage::unknown());
        }
        if !m.missing.is_empty() {
            self.error(nid, TypeErrorKind::MissingCases(std::mem::take(&mut m.missing)));
        }
        for (clause, used) in clauses.iter().zip(&m.used) {
            if !used && !matches!(clause.rhs, hir::Rhs::Impossible) && self.plausible(ctx, &m, clause) {
                self.error(clause.id, TypeErrorKind::UnreachableClause);
            }
        }
        (core, usage)
    }

    fn tree(&mut self, m: &mut Matcher, ctx: &Ctx, mut problems: Vec<Problem<'_>>, goal: VTy) -> (Rc<Core>, Usage) {
        loop {
            let Some(first) = problems.first_mut() else {
                let case = self.describe(ctx, &m.args);
                if !m.missing.contains(&case) {
                    m.missing.push(case);
                }
                return (Rc::new(Core::Error), Usage::unknown());
            };
            if !first.pending.is_empty() {
                return self.intro(m, ctx, problems, goal);
            }
            match self.simplify(ctx, first) {
                Step::Done => return self.leaf(m, ctx, problems, goal),
                Step::Split(k, how) => return self.split(m, ctx, problems, goal, k, how),
                Step::Fails => {
                    problems.remove(0);
                }
                Step::Failed => {
                    m.failed = true;
                    problems.remove(0);
                }
            }
        }
    }

    /// Bind the next argument, for every problem's next pattern.
    fn intro(&mut self, m: &mut Matcher, ctx: &Ctx, mut problems: Vec<Problem<'_>>, goal: VTy) -> (Rc<Core>, Usage) {
        let goal = self.force(goal);
        match &*goal {
            Value::Pi { name, icit, grade, dom, cod, .. } => {
                let key = self.fresh_key();
                let lvl = ctx.lvl;
                // named after the first equation to name it (an implicit
                // argument has no pattern)
                let named = problems.iter().find_map(|p| match &p.pending[0].kind {
                    PatKind::Var(x) | PatKind::As(x, _) if *icit == Icit::Explicit => Some(x.text.clone()),
                    _ => None,
                });
                if named.is_none() && name == "_" {
                    m.unnamed.insert(ctx.lvl);
                }
                let hint = named.unwrap_or_else(|| if name != "_" { name.clone() } else { "x".to_string() });
                let ctx2 = ctx.bind_keyed(key, &hint, dom.clone(), Some(grade.clone()));
                if *icit == Icit::Explicit {
                    let ty = self.quote(ctx.lvl, dom.clone());
                    for p in &mut problems {
                        let (pat, rest) = p.pending.split_first().expect("every clause has as many patterns");
                        p.pending = rest;
                        self.record(pat.id, ty.clone());
                        p.constraints.push((pat, Target::Slot(lvl)));
                    }
                    m.args.push(lvl);
                    m.arg_types.push(dom.clone());
                }
                let (cod, effects) = self.split_comp(self.apply(cod, vvar(lvl, hint.clone())));
                let (body, mut usage) = self.tree(m, &ctx2, problems, cod);
                // after an error, what was used is anyone's guess
                if m.failed {
                    usage = Usage::unknown();
                }
                // what the equations do is what calling the function does
                self.close_effects(&ctx2, m.node, &mut usage, effects);
                self.leave_key(&ctx2, m.node, (Some(key), &hint), grade.clone(), &mut usage, false);
                (self.lam(ctx, hint, grade, dom, body), usage)
            }
            // not known to be a function yet: make it one
            Value::Flex(..) => {
                let dom_c = self.fresh_meta(ctx, m.node, "the type of this argument");
                let dom = self.eval(&ctx.env, &dom_c);
                let ctx_arg = ctx.bind(None, "x", dom.clone());
                let cod_c = self.fresh_meta(&ctx_arg, m.node, "the result type of this function");
                let effects = self.fresh_meta(&ctx_arg, m.node, "the effects of this function");
                let cod_c = Rc::new(Core::Comp(cod_c, effects));
                let pi = Rc::new(Value::Pi {
                    name: "x".into(),
                    icit: Icit::Explicit,
                    grade: Poly::constant(Grade::DEFAULT).into_value(),
                    in_type: Poly::constant(Grade::DEFAULT_IN_TYPE).into_value(),
                    dom,
                    cod: Closure { env: ctx.env.clone(), body: cod_c },
                });
                if self.unify(ctx.lvl, goal.clone(), pi.clone()) {
                    return self.intro(m, ctx, problems, pi);
                }
                self.too_many_patterns(m, ctx, &problems, goal)
            }
            Value::Error => {
                m.failed = true;
                (Rc::new(Core::Error), Usage::unknown())
            }
            _ => self.too_many_patterns(m, ctx, &problems, goal),
        }
    }

    fn too_many_patterns(&mut self, m: &mut Matcher, ctx: &Ctx, problems: &[Problem<'_>], goal: VTy) -> (Rc<Core>, Usage) {
        let ty = self.quote(ctx.lvl, goal);
        self.error(problems[0].pending[0].id, TypeErrorKind::TooManyPatterns(ty));
        m.failed = true;
        (Rc::new(Core::Error), Usage::unknown())
    }

    /// Settle what can be settled of a problem's patterns, left to right,
    /// until one waits on a variable or disagrees.
    fn simplify<'a>(&mut self, ctx: &Ctx, p: &mut Problem<'a>) -> Step {
        // each step settles the first constraint, or replaces it with what
        // it comes to, so the first is always the one to look at
        let i = 0;
        while i < p.constraints.len() {
            let (pat, target) = p.constraints[i].clone();
            match &pat.kind {
                PatKind::Var(x) => {
                    p.bindings.push((pat.id, x, target));
                    p.constraints.remove(i);
                }
                PatKind::Wild => {
                    p.constraints.remove(i);
                }
                PatKind::As(x, q) => {
                    p.bindings.push((pat.id, x, target.clone()));
                    p.constraints[i] = (q, target);
                }
                PatKind::Dot(e) => {
                    p.dots.push((e, target));
                    p.constraints.remove(i);
                }
                PatKind::Lit(Lit::Str(_)) => {
                    self.error(pat.id, TypeErrorKind::StringPattern);
                    return Step::Failed;
                }
                PatKind::Lit(lit) => {
                    let want = Rc::new(Value::LitTy(super::core::lit_builtin(lit)));
                    let ty = self.target_type(ctx, &target);
                    if !self.unify(ctx.lvl, ty.clone(), want.clone()) {
                        let (expected, actual) = (self.quote(ctx.lvl, ty), self.quote(ctx.lvl, want));
                        self.error(pat.id, TypeErrorKind::Mismatch { expected, actual });
                        return Step::Failed;
                    }
                    if let Lit::Unit = lit {
                        p.constraints.remove(i);
                        continue;
                    }
                    let v = self.force(self.target_value(ctx, &target));
                    match &*v {
                        Value::Lit(l) if l == lit => {
                            p.constraints.remove(i);
                        }
                        Value::Lit(_) => return Step::Fails,
                        Value::Rigid(k, _, sp) if sp.is_empty() => {
                            let how = match lit {
                                Lit::Bool(_) => Split::Con,
                                _ => Split::Lit(lit.clone()),
                            };
                            return Step::Split(*k, how);
                        }
                        _ => return self.cannot_split(ctx, pat, v),
                    }
                }
                PatKind::Con(..) | PatKind::SeqNil | PatKind::SeqCons(_) => {
                    let (cu, args): (usize, &'a [hir::Pattern]) = match &pat.kind {
                        // the renamer reported an unknown name already
                        PatKind::Con(c, args) => {
                            // a name with several meanings: the one of the
                            // type matched against
                            let ty = self.target_type(ctx, &target);
                            match self.pattern_ctor(ctx, pat, c, &ty) {
                                Some(cu) => (cu, args),
                                None => return Step::Failed,
                            }
                        }
                        // `#[p, q]`: taken apart with the constructors of
                        // what it is matched against
                        _ => {
                            let ty = self.target_type(ctx, &target);
                            let Some((nil, cons)) = self.seq_pattern_ctors(ctx, pat.id, ty.clone()) else {
                                let ty = self.force(ty);
                                if !matches!(*ty, Value::Error) {
                                    let ty = self.quote(ctx.lvl, ty);
                                    self.error(pat.id, TypeErrorKind::NotASequence(ty));
                                }
                                return Step::Failed;
                            };
                            match &pat.kind {
                                PatKind::SeqCons(parts) => (cons, parts),
                                _ => (nil, &[]),
                            }
                        }
                    };
                    let Some(info) = self.ctors.get(&cu).cloned() else {
                        let PatKind::Con(c, _) = &pat.kind else { unreachable!("a sequence's constructors are declared") };
                        self.error(pat.id, TypeErrorKind::NotAConstructor(c.text.clone()));
                        return Step::Failed;
                    };
                    let ty = self.force(self.target_type(ctx, &target));
                    match &*ty {
                        Value::Opaque(d, _, _) if *d == info.data => {}
                        Value::Error => return Step::Failed,
                        _ => {
                            let data = self.quote(ctx.lvl, ty.clone());
                            self.error(pat.id, TypeErrorKind::NotAConstructorOf { ctor: info.name.clone(), data });
                            return Step::Failed;
                        }
                    }
                    if args.len() != info.fields {
                        let kind = TypeErrorKind::WrongFieldCount { ctor: info.name.clone(), expected: info.fields, given: args.len() };
                        self.error(pat.id, kind);
                        return Step::Failed;
                    }
                    let v = self.force(self.target_value(ctx, &target));
                    match &*v {
                        Value::Opaque(c2, _, sp) if self.ctors.contains_key(c2) => {
                            if *c2 != cu {
                                return Step::Fails;
                            }
                            let fields = self.field_targets(ctx, &target, cu, sp);
                            p.constraints.splice(i..=i, args.iter().zip(fields));
                        }
                        Value::Rigid(k, _, sp) if sp.is_empty() => return Step::Split(*k, Split::Con),
                        _ => return self.cannot_split(ctx, pat, v),
                    }
                }
            }
        }
        Step::Done
    }

    /// Whether a clause no case reached at least makes sense, as far as its
    /// outermost patterns go: a constructor of some other type is an error,
    /// not merely an unreachable clause.
    fn plausible(&mut self, ctx: &Ctx, m: &Matcher, clause: &hir::Clause) -> bool {
        for (pat, ty) in clause.pats.iter().zip(&m.arg_types) {
            let pat = peel(pat);
            let PatKind::Con(c, _) = &pat.kind else { continue };
            let Some(u) = self.pattern_ctor(ctx, pat, c, ty) else { return false };
            let Some(info) = self.ctors.get(&u) else {
                self.error(pat.id, TypeErrorKind::NotAConstructor(c.text.clone()));
                return false;
            };
            let ty = self.force(ty.clone());
            if !matches!(&*ty, Value::Opaque(d, _, _) if *d == info.data) {
                let data = self.quote(ctx.lvl, ty);
                self.error(pat.id, TypeErrorKind::NotAConstructorOf { ctor: c.text.clone(), data });
                return false;
            }
        }
        true
    }

    fn cannot_split(&mut self, ctx: &Ctx, pat: &hir::Pattern, v: Rc<Value>) -> Step {
        if !matches!(*v, Value::Error) {
            let q = self.quote(ctx.lvl, v);
            self.error(pat.id, TypeErrorKind::CannotSplit(q));
        }
        Step::Failed
    }

    /// What the explicit fields of a constructor value are, to match its
    /// patterns' fields against: the slots a split bound for them, if it was
    /// a split that made it this constructor; otherwise its arguments.
    fn field_targets(&self, ctx: &Ctx, target: &Target, c: usize, sp: &[Elim]) -> Vec<Target> {
        let info = &self.ctors[&c];
        if let Target::Slot(k) = target {
            if let Some((c2, slots)) = ctx.splits.get(k) {
                if *c2 == c {
                    return slots[info.params..].iter().map(|&l| Target::Slot(l)).collect();
                }
            }
        }
        let mut cty = self.constants[&c].clone();
        let mut out = Vec::new();
        for (i, elim) in sp.iter().enumerate() {
            let Elim::App(arg) = elim else { break };
            let forced = self.force(cty);
            let Value::Pi { dom, cod, .. } = &*forced else { break };
            if i >= info.params {
                out.push(match &*self.force(arg.clone()) {
                    Value::Rigid(l, _, sp) if sp.is_empty() => Target::Slot(*l),
                    _ => Target::Value(arg.clone(), dom.clone()),
                });
            }
            cty = self.apply(cod, arg.clone());
        }
        out
    }

    fn target_value(&self, ctx: &Ctx, t: &Target) -> Rc<Value> {
        match t {
            Target::Slot(k) => ctx.value(*k),
            Target::Value(v, _) => v.clone(),
        }
    }

    fn target_type(&self, ctx: &Ctx, t: &Target) -> VTy {
        match t {
            Target::Slot(k) => ctx.binders[*k].ty.clone(),
            Target::Value(_, ty) => ty.clone(),
        }
    }

    /// The first problem has won: check its right-hand side, with its names
    /// bound.
    fn leaf(&mut self, m: &mut Matcher, ctx: &Ctx, mut problems: Vec<Problem<'_>>, goal: VTy) -> (Rc<Core>, Usage) {
        let p = problems.remove(0);
        m.used[p.index] = true;
        if let hir::Rhs::Impossible = p.clause.rhs {
            if let Some(found) = self.empty_split(m, ctx) {
                return found;
            }
            self.error(p.clause.id, TypeErrorKind::NotImpossible);
            m.failed = true;
            return (Rc::new(Core::Error), Usage::unknown());
        }
        let mut inner = ctx.clone();
        let mut lets = Vec::new();
        for (id, x, target) in &p.bindings {
            let ty = self.target_type(&inner, target);
            let quoted = self.quote(inner.lvl, ty.clone());
            self.record(*id, quoted);
            match target {
                Target::Slot(k) => inner.alias(x, *k),
                // known only to the types, and so erased
                Target::Value(v, ty) => {
                    let core = Rc::new(Core::Runtime(Note::Erased, self.quote(inner.lvl, v.clone())));
                    lets.push((*x, core));
                    inner = inner.define(x, ty.clone(), v.clone());
                }
            }
        }
        for (e, target) in &p.dots {
            let ty = self.target_type(&inner, target);
            // never run, and so it must be pure
            let (e_c, not_run) = self.check(&inner, e, ty);
            if !self.pure(&inner, e.id, &not_run) {
                continue;
            }
            let given = self.eval(&inner.env, &e_c);
            let expected = self.target_value(&inner, target);
            if !self.unify(inner.lvl, expected.clone(), given.clone()) {
                let (expected, given) = (self.quote(inner.lvl, expected), self.quote(inner.lvl, given));
                self.error(e.id, TypeErrorKind::DotMismatch { expected, given });
            }
        }
        let (body, mut usage) = match &p.clause.rhs {
            hir::Rhs::Plain(e) => self.check(&inner, e, goal),
            hir::Rhs::Guarded(gs) => self.guards(m, &inner, gs, problems, goal),
            hir::Rhs::Impossible => unreachable!("handled above"),
        };
        let zero = Poly::constant(Grade::ZERO).into_value();
        for (x, _) in lets.iter().rev() {
            self.leave_binder(&inner, p.clause.id, x, zero.clone(), &mut usage, false);
        }
        let core = lets.into_iter().rev().fold(body, |acc, (x, c)| Rc::new(Core::Let(x.text.clone(), c, acc)));
        (core, usage)
    }

    /// `| g = e | ..`: the first guard that holds, and if none does, the
    /// clauses after this one.
    fn guards(
        &mut self,
        m: &mut Matcher,
        ctx: &Ctx,
        gs: &[(hir::Term, hir::Term)],
        rest: Vec<Problem<'_>>,
        goal: VTy,
    ) -> (Rc<Core>, Usage) {
        let Some(((g, e), more)) = gs.split_first() else { return self.tree(m, ctx, rest, goal) };
        let (g_c, g_usage) = self.check(ctx, g, Rc::new(Value::LitTy(Builtin::Bool)));
        let (e_c, e_usage) = self.check(ctx, e, goal.clone());
        // `otherwise`: nothing after it is needed
        if matches!(*self.force(self.eval(&ctx.env, &g_c)), Value::Lit(Lit::Bool(true))) {
            return (e_c, g_usage + e_usage);
        }
        let (f_c, f_usage) = self.guards(m, ctx, more, rest, goal);
        let alts = vec![
            CaseAlt { head: Head::Bool(true), name: "True".into(), vars: Vec::new(), body: e_c },
            CaseAlt { head: Head::Bool(false), name: "False".into(), vars: Vec::new(), body: f_c },
        ];
        let either = self.join_usage(m.node, e_usage, f_usage);
        (Rc::new(Core::Case(g_c, Rc::new(alts))), g_usage + either)
    }

    /// Branch on the variable in slot `k`.
    fn split(
        &mut self,
        m: &mut Matcher,
        ctx: &Ctx,
        problems: Vec<Problem<'_>>,
        goal: VTy,
        k: Lvl,
        how: Split,
    ) -> (Rc<Core>, Usage) {
        let scrutinee = Rc::new(Core::Var(ctx.lvl - k - 1, ctx.binders[k].name.clone()));
        let used = self.split_use(ctx, k);
        let mut forced = false;
        let mut alts = Vec::new();
        // the alternatives' usages, joined once they are all known
        let mut parts: Vec<Usage> = Vec::new();
        let mut join = |u: Usage| parts.push(u);
        match how {
            Split::Lit(lit) => {
                // a character is compared by its code point
                let (scrutinee, against) = match &lit {
                    Lit::Char(c) => match self.native_unique(idyllic_builtins::Native::CharCode) {
                        Some(u) => (
                            Rc::new(Core::App(Rc::new(Core::Opaque(u, "charCode".into())), scrutinee)),
                            Lit::Int(i64::from(u32::from(*c))),
                        ),
                        None => (scrutinee, lit.clone()),
                    },
                    _ => (scrutinee, lit.clone()),
                };
                let test = Rc::new(Core::App(
                    Rc::new(Core::App(Rc::new(Core::Prim(Prim::IntEq)), scrutinee)),
                    Rc::new(Core::Lit(against)),
                ));
                let is = Rc::new(Value::Lit(lit.clone()));
                let (yes, yes_usage) = match self.unify_indices(ctx, vec![(ctx.value(k), is)]) {
                    Unified::Refined(ctx2, sigma) => {
                        let goal2 = self.refine_goal(ctx.lvl, &sigma, &goal);
                        let ps = self.resubst_problems(ctx.lvl, &sigma, problems.clone());
                        self.tree(m, &ctx2, ps, goal2)
                    }
                    Unified::Absurd | Unified::Stuck(..) => (Rc::new(Core::Error), Usage::unknown()),
                };
                // the clauses that wanted this number here are out
                let rest = problems.into_iter().filter(|p| !self.wants_literal(ctx, p, k, &lit)).collect();
                let (no, no_usage) = self.tree(m, ctx, rest, goal);
                let alts = vec![
                    CaseAlt { head: Head::Bool(true), name: "True".into(), vars: Vec::new(), body: yes },
                    CaseAlt { head: Head::Bool(false), name: "False".into(), vars: Vec::new(), body: no },
                ];
                let both = self.join_usage(m.node, yes_usage, no_usage);
                return (Rc::new(Core::Case(test, Rc::new(alts))), used + both);
            }
            Split::Con => {
                let ty = self.force(ctx.binders[k].ty.clone());
                match &*ty {
                    Value::LitTy(Builtin::Bool) => {
                        for b in [true, false] {
                            let is = Rc::new(Value::Lit(Lit::Bool(b)));
                            let Unified::Refined(ctx2, sigma) = self.unify_indices(ctx, vec![(ctx.value(k), is)]) else {
                                continue;
                            };
                            let goal2 = self.refine_goal(ctx.lvl, &sigma, &goal);
                            let ps = self.resubst_problems(ctx.lvl, &sigma, problems.clone());
                            let (body, u) = self.tree(m, &ctx2, ps, goal2);
                            join(u);
                            let name = if b { "True" } else { "False" }.to_string();
                            alts.push(CaseAlt { head: Head::Bool(b), name, vars: Vec::new(), body });
                        }
                    }
                    Value::Opaque(d, _, _) if self.datas.contains_key(d) => {
                        let mut cases = Vec::new();
                        for c in self.datas[d].clone() {
                            let hints = |i: usize| self.field_hint(ctx, &problems, k, c, i);
                            let hints: Vec<Option<String>> = (0..self.ctor_arity(c)).map(hints).collect();
                            cases.push((c, self.instantiate(ctx, k, c, &hints)));
                        }
                        // An erased value can be matched on when its type
                        // leaves it one constructor it could be (or none):
                        // then there is nothing to look at.  That it really
                        // is that constructor rests on it having been made
                        // by pure, and so total, code, which is what the
                        // effect system sees to for everything erased.
                        let possible = cases.iter().filter(|(_, (_, _, u))| !matches!(u, Unified::Absurd)).count();
                        if possible <= 1 && self.erased_slot(ctx, k) {
                            forced = true;
                        }
                        for (c, (inner, delta, unified)) in cases {
                            match unified {
                                Unified::Absurd => {}
                                Unified::Stuck(a, b) => {
                                    let (lhs, rhs) = (self.quote(inner.lvl, a), self.quote(inner.lvl, b));
                                    let at = problems.first().map_or(m.node, |p| p.clause.id);
                                    self.error(at, TypeErrorKind::IndexUnificationStuck { lhs, rhs });
                                    m.failed = true;
                                }
                                Unified::Refined(mut ctx2, sigma) => {
                                    ctx2.splits.insert(k, (c, delta.iter().map(|(l, ..)| *l).collect()));
                                    let goal2 = self.refine_goal(inner.lvl, &sigma, &goal);
                                    let ps = self.resubst_problems(inner.lvl, &sigma, problems.clone());
                                    let (body, mut u) = self.tree(m, &ctx2, ps, goal2);
                                    if m.failed {
                                        u = Usage::unknown();
                                    }
                                    let whole = (!m.unnamed.contains(&k)).then(|| ctx.binders[k].name.clone());
                                    for (_, key, hint, grade) in delta.iter().rev() {
                                        self.leave_field(&ctx2, m.node, (*key, hint), grade.clone(), &mut u, whole.clone());
                                    }
                                    join(u);
                                    let vars = delta.iter().map(|(_, _, hint, _)| hint.clone()).collect();
                                    alts.push(CaseAlt { head: Head::Con(c), name: self.ctors[&c].name.clone(), vars, body });
                                }
                            }
                        }
                    }
                    Value::Error => {
                        m.failed = true;
                        return (Rc::new(Core::Error), Usage::unknown());
                    }
                    _ => {
                        let q = self.quote(ctx.lvl, ty.clone());
                        let at = problems.first().map_or(m.node, |p| p.clause.id);
                        self.error(at, TypeErrorKind::NotADataType(q));
                        m.failed = true;
                        return (Rc::new(Core::Error), Usage::unknown());
                    }
                }
            }
        }
        let usage = parts.into_iter().reduce(|a, b| self.join_usage(m.node, a, b));
        let case = Rc::new(Core::Case(scrutinee, Rc::new(alts)));
        match forced {
            true => (Rc::new(Core::Runtime(Note::Forced, case)), usage.unwrap_or_else(Usage::none)),
            false => (case, used + usage.unwrap_or_else(Usage::none)),
        }
    }

    /// Leaving the variable a constructor's argument became: as with any
    /// binder, but a complaint says where its grade came from.
    fn leave_field(
        &mut self,
        ctx: &Ctx,
        node: NodeId,
        (key, name): (usize, &str),
        allowed: Rc<Value>,
        usage: &mut Usage,
        whole: Option<String>,
    ) {
        let used = usage.take(Some(key));
        if usage.is_unknown() || self.fits_or_waits(ctx, node, name, &used, allowed.clone(), Some(whole.clone())) {
            return;
        }
        let used = self.quote(ctx.lvl, used.into_value());
        let allowed = self.quote(ctx.lvl, allowed);
        let kind = TypeErrorKind::FieldUsage { name: name.to_string(), used, allowed, whole };
        self.error(node, kind);
    }

    /// How often a slot may be used, if known; otherwise as an unannotated
    /// binder may be (as often as wanted, or once in a linear module).
    fn slot_grade(&self, ctx: &Ctx, k: Lvl) -> Rc<Value> {
        ctx.binders[k].grade.clone().unwrap_or_else(|| Poly::constant(self.default_grade()).into_value())
    }

    /// Whether no value of a type has fields: a literal's type, or a data
    /// type whose constructors take none.
    fn fieldless(&self, ty: &VTy) -> bool {
        match &*self.force(ty.clone()) {
            Value::LitTy(_) => true,
            Value::Opaque(d, _, _) => self.datas.get(d).is_some_and(|cs| cs.iter().all(|c| self.ctors[c].fields == 0)),
            _ => false,
        }
    }

    /// Whether a slot is erased: its grade is known to be `0`.
    fn erased_slot(&self, ctx: &Ctx, k: Lvl) -> bool {
        self.to_poly(self.slot_grade(ctx, k)).as_constant().is_some_and(|g| g.is_zero())
    }

    /// What splitting slot `k` uses of it: all of it, since its fields come
    /// out with its grade.  Except that a value that is not there at run
    /// time (grade `0`) cannot be looked at, and looking is a use.
    fn split_use(&self, ctx: &Ctx, k: Lvl) -> Usage {
        let Some(key) = ctx.binders[k].key else { return Usage::none() };
        let grade = self.to_poly(self.slot_grade(ctx, k));
        match grade.as_constant() {
            Some(g) if g.is_zero() => Usage::var(key),
            _ => Usage::var(key).scale(&grade),
        }
    }

    fn ctor_arity(&self, c: usize) -> usize {
        let info = &self.ctors[&c];
        info.params + info.fields
    }

    /// Bind the arguments of constructor `c` as new variables, and unify
    /// the variable in slot `k` with `c` applied to them, indices and all.
    fn instantiate(&mut self, ctx: &Ctx, k: Lvl, c: usize, hints: &[Option<String>]) -> (Ctx, Telescope, Unified) {
        let mut cty = self.constants[&c].clone();
        // the fields of a value have its grade
        let whole = self.slot_grade(ctx, k);
        let mut inner = ctx.clone();
        let mut delta = Vec::new();
        for hint in hints {
            let forced = self.force(cty.clone());
            let Value::Pi { name, grade, dom, cod, .. } = &*forced else { break };
            let hint = hint.clone().unwrap_or_else(|| if name == "_" { "x".to_string() } else { name.clone() });
            let key = self.fresh_key();
            let lvl = inner.lvl;
            let grade = (&self.to_poly(grade.clone()) * &self.to_poly(whole.clone())).into_value();
            inner = inner.bind_keyed(key, &hint, dom.clone(), Some(grade.clone()));
            delta.push((lvl, key, hint.clone(), grade));
            cty = self.apply(cod, vvar(lvl, hint));
        }
        let ty = self.force(ctx.binders[k].ty.clone());
        let (Value::Opaque(_, _, indices), Value::Opaque(_, _, result)) = (&*ty, &*self.force(cty)) else {
            return (inner, delta, Unified::Absurd);
        };
        let args = |sp: &[Elim]| -> Vec<Rc<Value>> {
            sp.iter()
                .filter_map(|e| match e {
                    Elim::App(a) => Some(a.clone()),
                    _ => None,
                })
                .collect()
        };
        let mut eqs: Vec<(Rc<Value>, Rc<Value>)> = args(indices).into_iter().zip(args(result)).collect();
        let spine = delta.iter().map(|(l, _, hint, _)| Elim::App(vvar(*l, hint.clone()))).collect();
        let name = self.ctors[&c].name.clone();
        eqs.push((ctx.value(k), Rc::new(Value::Opaque(c, name, spine))));
        let unified = self.unify_indices(&inner, eqs);
        (inner, delta, unified)
    }

    /// A name for the `i`th argument of constructor `c`, from the first
    /// pattern that names it.
    fn field_hint(&self, ctx: &Ctx, problems: &[Problem<'_>], k: Lvl, c: usize, i: usize) -> Option<String> {
        let params = self.ctors[&c].params;
        problems.iter().flat_map(|p| &p.constraints).find_map(|(pat, target)| {
            if !self.targets_slot(ctx, target, k) {
                return None;
            }
            let pat = peel(pat);
            let PatKind::Con(c2, args) = &pat.kind else { return None };
            if self.pattern_unique(pat, c2) != Some(c) || i < params {
                return None;
            }
            match &peel(args.get(i - params)?).kind {
                PatKind::Var(x) => Some(x.text.clone()),
                _ => None,
            }
        })
    }

    fn targets_slot(&self, ctx: &Ctx, target: &Target, k: Lvl) -> bool {
        matches!(&*self.force(self.target_value(ctx, target)), Value::Rigid(l, _, sp) if *l == k && sp.is_empty())
    }

    /// Whether a problem needs slot `k` to be this number.
    fn wants_literal(&self, ctx: &Ctx, p: &Problem<'_>, k: Lvl, lit: &Lit) -> bool {
        p.constraints
            .iter()
            .any(|(pat, target)| matches!(&peel(pat).kind, PatKind::Lit(l) if l == lit) && self.targets_slot(ctx, target, k))
    }

    /// For `impossible`: a variable of a type none of whose constructors
    /// can be what it is, which a case with no alternatives disposes of.
    fn empty_split(&mut self, m: &Matcher, ctx: &Ctx) -> Option<(Rc<Core>, Usage)> {
        let start = m.args.first().copied().unwrap_or(ctx.lvl);
        for k in (start..ctx.lvl).rev() {
            if !ctx.binders[k].info.unknown() {
                continue;
            }
            let Value::Opaque(d, _, _) = &*self.force(ctx.binders[k].ty.clone()) else { continue };
            let Some(ctors) = self.datas.get(d).cloned() else { continue };
            let empty = ctors.into_iter().all(|c| {
                let hints = vec![None; self.ctor_arity(c)];
                matches!(self.instantiate(ctx, k, c, &hints).2, Unified::Absurd)
            });
            if empty {
                let scrutinee = Rc::new(Core::Var(ctx.lvl - k - 1, ctx.binders[k].name.clone()));
                let case = Rc::new(Core::Case(scrutinee, Rc::new(Vec::new())));
                // on an erased value, not even looked at (see `split`)
                return Some(match self.erased_slot(ctx, k) {
                    true => (Rc::new(Core::Runtime(Note::Forced, case)), Usage::none()),
                    false => (case, self.split_use(ctx, k)),
                });
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // Describing cases
    // -----------------------------------------------------------------------

    /// A case, as the patterns that would match it.
    fn describe(&self, ctx: &Ctx, args: &[Lvl]) -> String {
        let nested = args.len() > 1;
        args.iter().map(|&l| self.show_pattern(ctx.value(l), nested)).collect::<Vec<_>>().join(" ")
    }

    fn show_pattern(&self, v: Rc<Value>, nested: bool) -> String {
        let v = self.force(v);
        match &*v {
            Value::Opaque(c, name, sp) if self.ctors.contains_key(c) => {
                let params = self.ctors[c].params;
                let args: Vec<String> = sp
                    .iter()
                    .skip(params)
                    .filter_map(|e| match e {
                        Elim::App(a) => Some(self.show_pattern(a.clone(), true)),
                        _ => None,
                    })
                    .collect();
                if args.is_empty() {
                    return name.clone();
                }
                let shown = if name.starts_with(':') && args.len() == 2 {
                    format!("{} {name} {}", args[0], args[1])
                } else {
                    format!("{name} {}", args.join(" "))
                };
                if nested {
                    format!("({shown})")
                } else {
                    shown
                }
            }
            Value::Lit(l) => l.to_string(),
            _ => "_".to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // Unifying indices
    // -----------------------------------------------------------------------

    /// Solve equations between values for the context's variables.  Unlike
    /// [`Checker::unify`], which solves metavariables and holds variables
    /// fixed, this treats every bound variable of the context as unknown:
    /// matching is how they become known.
    fn unify_indices(&mut self, ctx: &Ctx, eqs: Vec<(Rc<Value>, Rc<Value>)>) -> Unified {
        let l = ctx.lvl;
        let mut sigma: HashMap<Lvl, Rc<Value>> = HashMap::new();
        let mut work: VecDeque<(Rc<Value>, Rc<Value>)> = eqs.into();
        while let Some((a, b)) = work.pop_front() {
            let a = self.force(self.apply_sigma(ctx, &sigma, a));
            let b = self.force(self.apply_sigma(ctx, &sigma, b));
            let var = |v: &Value| match v {
                Value::Rigid(x, _, sp) if sp.is_empty() && ctx.binders[*x].info.unknown() => Some(*x),
                _ => None,
            };
            match (var(&a), var(&b)) {
                (Some(x), Some(y)) if x == y => continue,
                // two variables: keep the older one, the one more likely
                // to have been named
                (Some(x), Some(y)) => {
                    let (x, t) = if x > y { (x, b) } else { (y, a) };
                    self.assign(ctx, &mut sigma, x, t);
                    continue;
                }
                (Some(x), None) | (None, Some(x)) => {
                    let t = if var(&a).is_some() { b.clone() } else { a.clone() };
                    match self.occurs(l, x, &t) {
                        None => {
                            self.assign(ctx, &mut sigma, x, t);
                            continue;
                        }
                        // `n = S n`
                        Some(true) => return Unified::Absurd,
                        Some(false) => return Unified::Stuck(a, b),
                    }
                }
                (None, None) => {}
            }
            let is_ctor = |c: &usize| self.ctors.contains_key(c);
            match (&*a, &*b) {
                (Value::Opaque(c1, _, s1), Value::Opaque(c2, _, s2)) if is_ctor(c1) && is_ctor(c2) => {
                    if c1 != c2 {
                        return Unified::Absurd;
                    }
                    for pair in s1.iter().zip(s2) {
                        if let (Elim::App(x), Elim::App(y)) = pair {
                            work.push_back((x.clone(), y.clone()));
                        }
                    }
                }
                (Value::Lit(x), Value::Lit(y)) => {
                    if x != y {
                        return Unified::Absurd;
                    }
                }
                _ => {
                    if !self.unify(l, a.clone(), b.clone()) {
                        return Unified::Stuck(a, b);
                    }
                }
            }
        }
        if sigma.is_empty() {
            return Unified::Refined(ctx.clone(), None);
        }
        let env = self.sigma_env(ctx, &sigma);
        Unified::Refined(self.refine(ctx, &sigma, &env), Some(env))
    }

    /// Whether variable `x` occurs in `t`: not at all (`None`), under
    /// constructors only (`Some(true)`, so `x = t` has no solution), or
    /// somewhere else (`Some(false)`, so there is no telling).
    fn occurs(&self, l: Lvl, x: Lvl, t: &Rc<Value>) -> Option<bool> {
        if !mentions(&self.quote(l, t.clone()), l - x - 1) {
            return None;
        }
        Some(self.rigidly_in(x, t))
    }

    fn rigidly_in(&self, x: Lvl, t: &Rc<Value>) -> bool {
        match &*self.force(t.clone()) {
            Value::Rigid(y, _, sp) => *y == x && sp.is_empty(),
            Value::Opaque(c, _, sp) if self.ctors.contains_key(c) => {
                sp.iter().any(|e| matches!(e, Elim::App(a) if self.rigidly_in(x, a)))
            }
            _ => false,
        }
    }

    /// Add `x := t` to a substitution, keeping it idempotent: no solution
    /// mentions a solved variable.
    fn assign(&self, ctx: &Ctx, sigma: &mut HashMap<Lvl, Rc<Value>>, x: Lvl, t: Rc<Value>) {
        let single = self.sigma_env(ctx, &HashMap::from([(x, t.clone())]));
        for s in sigma.values_mut() {
            *s = self.eval(&single, &self.quote(ctx.lvl, s.clone()));
        }
        sigma.insert(x, t);
    }

    /// A substitution as an environment: each solved variable's solution,
    /// every other variable itself.
    fn sigma_env(&self, ctx: &Ctx, sigma: &HashMap<Lvl, Rc<Value>>) -> Env {
        (0..ctx.lvl).fold(Env::empty(), |env, k| {
            env.push(sigma.get(&k).cloned().unwrap_or_else(|| vvar(k, ctx.binders[k].name.clone())))
        })
    }

    fn apply_sigma(&self, ctx: &Ctx, sigma: &HashMap<Lvl, Rc<Value>>, v: Rc<Value>) -> Rc<Value> {
        if sigma.is_empty() {
            return v;
        }
        self.eval(&self.sigma_env(ctx, sigma), &self.quote(ctx.lvl, v))
    }

    /// The goal, in a case that has learned `sigma`.  A goal that is still a
    /// bare metavariable is the result type of a `case` (or a function) with
    /// no annotation, and is left as it is: it is applied to the variables
    /// matched on, and refining them would leave it applied to
    /// constructors, where it could not be solved.  So an unannotated match
    /// has a result type that does not depend on what is matched, which is
    /// all that could be inferred anyway.
    fn refine_goal(&self, l: Lvl, sigma: &Option<Env>, goal: &VTy) -> VTy {
        match &*self.force(goal.clone()) {
            Value::Flex(..) => goal.clone(),
            _ => self.resubst(l, sigma, goal.clone()),
        }
    }

    /// A result type left unsolved by a match that failed: an error, so
    /// that it is not reported again as unsolved.
    pub(super) fn abandon(&mut self, ty: &VTy) {
        if let Value::Flex(m, _) = &*self.force(ty.clone()) {
            if let super::MetaEntry::Unsolved(info) = &self.metas[*m] {
                self.metas[*m] = super::MetaEntry::Solved(Rc::new(Value::Error), info.clone());
            }
        }
    }

    /// Apply a substitution to a value of a context `l` slots long.
    fn resubst(&self, l: Lvl, sigma: &Option<Env>, v: Rc<Value>) -> Rc<Value> {
        match sigma {
            None => v,
            Some(env) => self.eval(env, &self.quote(l, v)),
        }
    }

    fn resubst_problems<'a>(&self, l: Lvl, sigma: &Option<Env>, problems: Vec<Problem<'a>>) -> Vec<Problem<'a>> {
        if sigma.is_none() {
            return problems;
        }
        let target = |t: Target| match t {
            Target::Slot(k) => Target::Slot(k),
            Target::Value(v, ty) => Target::Value(self.resubst(l, sigma, v), self.resubst(l, sigma, ty)),
        };
        problems
            .into_iter()
            .map(|p| Problem {
                constraints: p.constraints.into_iter().map(|(pat, t)| (pat, target(t))).collect(),
                bindings: p.bindings.into_iter().map(|(id, x, t)| (id, x, target(t))).collect(),
                dots: p.dots.into_iter().map(|(e, t)| (e, target(t))).collect(),
                ..p
            })
            .collect()
    }

    /// The context, having learned `sigma`: each solved variable defined as
    /// its solution, and every value and type that could mention one
    /// brought up to date.  A slot's value and type can only mention the
    /// slots before it, so those before the first solved one stay as they
    /// are — which spares the (closed) top-level definitions.
    fn refine(&self, ctx: &Ctx, sigma: &HashMap<Lvl, Rc<Value>>, env: &Env) -> Ctx {
        let first = sigma.keys().copied().min().unwrap_or(ctx.lvl);
        let l = ctx.lvl;
        let old: Vec<Rc<Value>> = ctx.env.iter().collect::<Vec<_>>().into_iter().rev().collect();
        let mut next = ctx.clone();
        let mut new_env = Env::empty();
        for (k, v) in old.into_iter().enumerate() {
            if k < first {
                new_env = new_env.push(v);
                continue;
            }
            new_env = new_env.push(self.eval(env, &self.quote(l, v)));
            next.binders[k].ty = self.eval(env, &self.quote(l, ctx.binders[k].ty.clone()));
            if sigma.contains_key(&k) {
                next.binders[k].info = BinderInfo::Defined;
            }
        }
        next.env = new_env;
        next
    }
}

/// The pattern under any as-patterns.
fn peel(mut p: &hir::Pattern) -> &hir::Pattern {
    while let PatKind::As(_, q) = &p.kind {
        p = q;
    }
    p
}

/// Whether a Core term mentions the variable with de Bruijn index `i`.
pub(super) fn mentions(c: &Core, i: usize) -> bool {
    match c {
        Core::Var(j, _) => *j == i,
        Core::Pi { grade, in_type, dom, cod, .. } => {
            mentions(grade, i) || mentions(in_type, i) || mentions(dom, i) || mentions(cod, i + 1)
        }
        Core::Lam(_, b) => mentions(b, i + 1),
        Core::Let(_, e, b) => mentions(e, i) || mentions(b, i + 1),
        Core::App(a, b)
        | Core::BoxTy(a, b)
        | Core::GradeAdd(a, b)
        | Core::GradeMul(a, b)
        | Core::GradeRange(a, b) => mentions(a, i) || mentions(b, i),
        Core::BoxIntro(a) | Core::Unbox(a) | Core::Runtime(_, a) => mentions(a, i),
        Core::Comp(a, e) => mentions(a, i) || mentions(e, i),
        Core::Handle(b, r, ops) => mentions(b, i) || mentions(r, i) || ops.iter().any(|op| mentions(&op.body, i)),
        Core::Row(labels, tails) => labels.iter().chain(tails).any(|c| mentions(c, i)),
        Core::Case(s, alts) => mentions(s, i) || alts.iter().any(|a| mentions(&a.body, i + a.vars.len())),
        Core::Type(_) | Core::LitTy(_) | Core::Lit(_) | Core::Prim(_) | Core::Opaque(..) | Core::Meta(_) | Core::GradeLit(_) | Core::Error => {
            false
        }
    }
}
