//! Lowering: elaborated Core to AxCut.
//!
//! Core is a lambda calculus, with expressions inside expressions and an
//! implicit notion of "where the result goes".  AxCut has neither, so the
//! translation is a call-by-value CPS transform whose target happens to be a
//! sequent calculus:
//!
//! ```text
//! ⟦ \x -> e ⟧ κ   =  new f = { apply(x, k) ⇒ ⟦e⟧ (ret to k) };  κ(f)
//! ⟦ f a ⟧ κ       =  ⟦f⟧ λf. ⟦a⟧ λa. new k = { ret(r) ⇒ κ(r) };  invoke f apply(a, k)
//! ⟦ let x = e in b ⟧ κ  =  ⟦e⟧ λx. ⟦b⟧ κ
//! ⟦ [e] ⟧ κ       =  ⟦e⟧ λv. let b = box(v);  κ(b)
//! ⟦ unbox e ⟧ κ   =  ⟦e⟧ λb. switch b { box(x) ⇒ κ(x) }
//! ⟦ 42 ⟧ κ        =  extern lit 42 { (n) ⇒ κ(n) }
//! ⟦ a + b ⟧ κ     =  ⟦a⟧ λa. ⟦b⟧ λb. extern add(a, b) { (r) ⇒ κ(r) }
//! ```
//!
//! κ is a *meta-level* continuation — a Rust closure building the rest of
//! the statement — so that no administrative redexes are produced: a
//! continuation only becomes a `new` when something has to be handed one,
//! at a call.  And when the continuation in hand is already a variable (we
//! are in tail position, returning to the caller's `k`) it is passed along as
//! it is rather than wrapped, so a tail call stays a jump and a loop runs in
//! constant space.
//!
//! **Recursion.**  A recursive definition is a constant in Core, with its
//! body kept to one side ([`RecDef`]).  Here it becomes a top-level label,
//! `define loop_12 : (captured.., k)`, lambda-lifted over the variables of
//! the context it was defined in; a reference to it jumps there with the
//! current continuation.
//!
//! **Erasure.**  Types and grades are erased to a constant with no fields
//! (which costs no memory).  So is whatever the elaborator marked as never
//! used ([`Note::Erased`]): an argument to a grade-`0` parameter — every
//! implicit type argument, for one — and the contents of a grade-`0` box.
//! They are not computed at all, so erasure is not only about space: an
//! erased argument that would loop forever does no harm.
//!
//! **Grades.**  The variables of the output carry what is known about how
//! often they are used ([`Program::grades`]): a binder's grade where the
//! elaborator noted a constant one ([`Note::Binds`]), and `1` for everything
//! this translation introduces itself — a continuation, the name of an
//! intermediate result — since each is made to be handed on exactly once.
//! [`super::grades`] then checks that the code keeps to them.
//!
//! **Effects.**  `handle` makes the handler a consumer — a `return` clause
//! and one clause per operation — and runs the body under it
//! ([`Stmt::Handle`]); applying an operation to its argument is
//! [`Stmt::Perform`].  What happens then is the machine's business: it keeps
//! the handlers that are in force, and a resumption is a consumer like any
//! other, which can be invoked as often as the program likes.
//!
//! The output uses variables freely; see [`super::linearize`].

use super::{free_vars_where, Branch, Definition, Extern, Label, Program, Stmt, Symbol, Var};
use idyllic_ast::Lit;
use idyllic_builtins::{Builtin, Prim};
use idyllic_typecheck::grade::Grade;
use idyllic_typecheck::core::{CaseAlt, Core, Head, Note, OpClause};
use idyllic_typecheck::handlers::OpInfo;
use idyllic_typecheck::natives::NativeInfo;
use idyllic_typecheck::{CtorInfo, RecDef, TypeCheckResult};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::rc::Rc;

/// Something in the program that cannot be run.  A program that type checks
/// has none, so reaching one means it did not (or that there is a bug here).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LowerError {
    /// A subterm that failed to elaborate.
    ErrorTerm,
    UnsolvedMeta(usize),
    UnboundVariable(String),
    UnknownDefinition(String),
}

impl fmt::Display for LowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LowerError::ErrorTerm => write!(f, "the program contains an ill-typed term"),
            LowerError::UnsolvedMeta(m) => write!(f, "the metavariable ?{m} was never solved"),
            LowerError::UnboundVariable(x) => write!(f, "the variable {x} is not in scope"),
            LowerError::UnknownDefinition(x) => write!(f, "the recursive definition {x} has no body"),
        }
    }
}

type Lowered = Result<Stmt, LowerError>;

/// The rest of a statement, waiting for the variable that names a value.
type Rest = Box<dyn FnOnce(&mut Lowering, Var) -> Lowered>;

/// What a Core variable stands for at run time.
#[derive(Clone, Debug)]
enum Binding {
    Var(Var),
    /// a builtin type: nothing
    Erased,
    Prim(Prim),
}

/// Outermost first, so a de Bruijn *level* indexes it directly.
type Env = Vec<Binding>;

/// Where the value of the term being lowered goes.
enum Kont {
    /// To a continuation variable: the term is in tail position.
    Return(Var),
    /// Into the rest of a statement still being built.
    Then(Rest),
}

impl Kont {
    fn then(f: impl FnOnce(&mut Lowering, Var) -> Lowered + 'static) -> Kont {
        Kont::Then(Box::new(f))
    }

    /// The statement that continues with `v`.
    fn apply(self, l: &mut Lowering, v: Var) -> Lowered {
        match self {
            Kont::Return(k) => Ok(Stmt::Invoke(k, Symbol::Ret, vec![v])),
            Kont::Then(f) => f(l, v),
        }
    }
}

/// A recursive function's entry point for a call that supplies all of its
/// arguments: `define f : (captured.., x1, .., xn, k)`.
#[derive(Clone)]
struct Direct {
    label: Label,
    captured: Vec<Var>,
    arity: usize,
}

/// What to do once a run of arguments has been computed.
type Finish = Box<dyn FnOnce(&mut Lowering, Vec<Var>) -> Lowered>;

/// What a curried function does once it has all its arguments and the
/// continuation that came with the last.
type Saturated = Box<dyn FnOnce(&mut Lowering, Vec<Var>, Var) -> Stmt>;

struct Lowering {
    rec_defs: HashMap<usize, RecDef>,
    meta_solutions: Vec<Option<Rc<Core>>>,
    /// recursive definitions already given a label, and what it captures
    labels: HashMap<usize, (Label, Vec<Var>)>,
    /// ... and the label that takes their arguments all at once
    direct: HashMap<usize, Option<Direct>>,
    ctors: HashMap<usize, CtorInfo>,
    /// the program's effects' operations
    ops: HashMap<usize, OpInfo>,
    /// the natives' operations (their types are type constructors)
    natives: HashMap<usize, NativeInfo>,
    type_constructors: HashSet<usize>,
    definitions: Vec<Definition>,
    next_var: usize,
    /// what is known of how often each variable is used
    grades: HashMap<Var, Grade>,
    /// the variables that name an intermediate result, which as such are
    /// used exactly once
    temporaries: HashSet<Var>,
    /// the variables that stand for a continuation
    continuations: HashSet<Var>,
}

/// Lower a checked program.  `None` if it has no expression to run.
pub fn lower(checked: &TypeCheckResult) -> Option<Result<Program, LowerError>> {
    // what the program never uses (most of the prelude, say) is left out
    let (pruned, _) = idyllic_typecheck::prune::prune(checked)?;
    let program = &pruned;
    let mut l = Lowering {
        rec_defs: checked.rec_defs.iter().map(|d| (d.unique, d.clone())).collect(),
        meta_solutions: checked.meta_solutions.clone(),
        labels: HashMap::new(),
        direct: HashMap::new(),
        ctors: checked.ctors.clone(),
        ops: checked.ops.clone(),
        natives: checked.natives.iter().filter(|(_, n)| !n.native.is_type()).map(|(u, n)| (*u, *n)).collect(),
        type_constructors: checked.type_constructors.iter().copied().collect(),
        definitions: Vec::new(),
        next_var: 0,
        grades: HashMap::new(),
        temporaries: HashSet::new(),
        continuations: HashSet::new(),
    };
    // the checker's initial context: the builtin types, then the primitives
    let env: Env = Builtin::ALL
        .iter()
        .map(|_| Binding::Erased)
        .chain(Prim::ALL.iter().map(|p| Binding::Prim(*p)))
        .collect();
    let halt = Kont::then(|_, answer| Ok(Stmt::Extern(Extern::Halt, vec![answer], Vec::new())));
    Some(l.term(&env, program, halt).map(|body| {
        let main = Definition { label: "main".into(), params: Vec::new(), body };
        let mut definitions = vec![main];
        definitions.append(&mut l.definitions);
        prune_labels(&mut definitions);
        Program { definitions, grades: std::mem::take(&mut l.grades), continuations: std::mem::take(&mut l.continuations) }
    }))
}

impl Lowering {
    /// A variable nothing else is called.  The hint is only for the reader.
    fn named(&mut self, hint: &str) -> Var {
        let n = self.next_var;
        self.next_var += 1;
        let hint: String = hint.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
        format!("{}_{n}", if hint.is_empty() || hint == "_" { "x" } else { &hint })
    }

    /// A variable the translation introduces: the name of an intermediate
    /// result, a function value, a continuation.  Each is made to be handed
    /// on exactly once, so each is linear, and says so.
    fn fresh(&mut self, hint: &str) -> Var {
        let v = self.named(hint);
        self.grades.insert(v.clone(), Grade::ONE);
        self.temporaries.insert(v.clone());
        v
    }

    /// A variable that stands for a continuation: linear, like every
    /// temporary, and known for what it is.
    fn continuation(&mut self, hint: &str) -> Var {
        let v = self.fresh(hint);
        self.continuations.insert(v.clone());
        v
    }

    /// A variable the program binds, with the grade its binder was given
    /// (where that was a constant).
    fn bound(&mut self, hint: &str, grade: Option<Grade>) -> Var {
        let v = self.named(hint);
        if let Some(g) = grade {
            self.grades.insert(v.clone(), g);
        }
        v
    }

    /// Hand the continuation over as a variable.  One that already is a
    /// variable is passed as it is — that is the tail call.
    fn with_continuation(&mut self, k: Kont, call: impl FnOnce(Var) -> Stmt) -> Lowered {
        match k {
            Kont::Return(k) => Ok(call(k)),
            Kont::Then(_) => {
                let (kv, r) = (self.continuation("k"), self.fresh("r"));
                let body = k.apply(self, r.clone())?;
                let ret = (Symbol::Ret, Branch { params: vec![r], body });
                Ok(Stmt::New(kv.clone(), Vec::new(), vec![ret], Box::new(call(kv))))
            }
        }
    }

    fn primitive(env: &Env, c: &Core) -> Option<Prim> {
        match c {
            Core::Prim(p) => Some(*p),
            Core::Var(i, _) => match env.get(env.len().checked_sub(i + 1)?)? {
                Binding::Prim(p) => Some(*p),
                _ => None,
            },
            _ => None,
        }
    }

    fn extern_of(p: Prim) -> Extern {
        match p {
            Prim::IntAdd => Extern::Add,
            Prim::IntMul => Extern::Mul,
            Prim::IntSub => Extern::Sub,
            Prim::IntDiv => Extern::Div,
            Prim::IntMod => Extern::Mod,
            Prim::IntLt => Extern::Lt,
            Prim::IntEq => Extern::Eq,
            Prim::StrAppend => Extern::Append,
        }
    }

    /// A primitive as a value, for when it is not applied to both arguments:
    /// the curried function that eventually performs it.
    fn primitive_value(&mut self, p: Prim, k: Kont) -> Lowered {
        debug_assert_eq!(p.arity(), 2, "every primitive so far is binary");
        let [f, a, g, b, r] = ["f", "a", "g", "b", "r"].map(|h| self.fresh(h));
        let (k1, k2) = (self.continuation("k"), self.continuation("k"));
        let perform = Stmt::Extern(
            Self::extern_of(p),
            vec![a.clone(), b.clone()],
            vec![Branch { params: vec![r.clone()], body: Stmt::Invoke(k2.clone(), Symbol::Ret, vec![r]) }],
        );
        let inner = Stmt::New(
            g.clone(),
            Vec::new(),
            vec![(Symbol::Apply, Branch { params: vec![b, k2], body: perform })],
            Box::new(Stmt::Invoke(k1.clone(), Symbol::Ret, vec![g])),
        );
        let rest = k.apply(self, f.clone())?;
        let outer = (Symbol::Apply, Branch { params: vec![a, k1], body: inner });
        Ok(Stmt::New(f, Vec::new(), vec![outer], Box::new(rest)))
    }

    /// The label of a recursive definition, and the variables to pass it
    /// (before the continuation).  Its body is lowered the first time it is
    /// asked for.
    fn label_of(&mut self, unique: usize, name: &str, env: &Env) -> Result<(Label, Vec<Var>), LowerError> {
        if let Some(known) = self.labels.get(&unique) {
            return Ok(known.clone());
        }
        let unknown = || LowerError::UnknownDefinition(name.to_string());
        let def = self.rec_defs.get(&unique).cloned().ok_or_else(unknown)?;
        // Every use is inside the definition's scope, so the first `lvl`
        // variables here are the ones it was defined under.
        let outer: Env = env.get(..def.lvl).ok_or_else(unknown)?.to_vec();
        let captured = Self::captured_by(&outer);
        let label = format!("{}_{unique}_value", def.name);
        // known before its body is lowered: the body may refer to it
        self.labels.insert(unique, (label.clone(), captured.clone()));
        let k = self.continuation("k");
        let body = match self.direct_of(unique, name, env)? {
            // a function: its value is closures that collect the arguments
            // and then jump to where the work is done
            Some(direct) => {
                let hints = vec!["x".to_string(); direct.arity];
                let finish = Box::new(move |_: &mut Lowering, args: Vec<Var>, kv: Var| {
                    let all = direct.captured.iter().cloned().chain(args).chain([kv]).collect();
                    Stmt::Jump(direct.label, all)
                });
                self.curried_value(hints, finish, Kont::Return(k.clone()))?
            }
            None => self.term(&outer, &def.core, Kont::Return(k.clone()))?,
        };
        let params = captured.iter().cloned().chain([k]).collect();
        self.definitions.push(Definition { label: label.clone(), params, body });
        Ok((label, captured))
    }

    fn captured_by(outer: &Env) -> Vec<Var> {
        let mut captured: Vec<Var> = Vec::new();
        for b in outer {
            if let Binding::Var(v) = b {
                if !captured.contains(v) {
                    captured.push(v.clone());
                }
            }
        }
        captured
    }

    /// If the recursive definition is a function, the label a saturated call
    /// jumps straight to: no closure is built and nothing is invoked, which
    /// is the difference between a call and a small conversation.
    fn direct_of(&mut self, unique: usize, name: &str, env: &Env) -> Result<Option<Direct>, LowerError> {
        if let Some(known) = self.direct.get(&unique) {
            return Ok(known.clone());
        }
        let unknown = || LowerError::UnknownDefinition(name.to_string());
        let def = self.rec_defs.get(&unique).cloned().ok_or_else(unknown)?;
        let outer: Env = env.get(..def.lvl).ok_or_else(unknown)?.to_vec();
        // peel the lambdas
        let mut params: Vec<(String, Option<Grade>)> = Vec::new();
        let mut body = def.core.clone();
        loop {
            let next = match &*body {
                Core::Lam(x, b) => (x.clone(), None, b.clone()),
                Core::Runtime(Note::Binds { grade, .. }, inner) => match &**inner {
                    Core::Lam(x, b) => (x.clone(), *grade, b.clone()),
                    _ => break,
                },
                _ => break,
            };
            params.push((next.0, next.1));
            body = next.2;
        }
        if params.is_empty() {
            self.direct.insert(unique, None);
            return Ok(None);
        }
        let direct = Direct { label: format!("{}_{unique}", def.name), captured: Self::captured_by(&outer), arity: params.len() };
        // known before the body is lowered: the body may call it
        self.direct.insert(unique, Some(direct.clone()));
        let mut inner = outer;
        let mut xs = Vec::new();
        for (x, grade) in &params {
            let x = self.bound(x, *grade);
            inner.push(Binding::Var(x.clone()));
            xs.push(x);
        }
        let k = self.continuation("k");
        let lowered = self.term(&inner, &body, Kont::Return(k.clone()))?;
        let all = direct.captured.iter().cloned().chain(xs).chain([k]).collect();
        self.definitions.push(Definition { label: direct.label.clone(), params: all, body: lowered });
        Ok(Some(direct))
    }

    /// A function of `hints.len()` arguments, taken one at a time, that then
    /// does `finish` with them and its last continuation.
    fn curried_value(
        &mut self,
        hints: Vec<String>,
        finish: Saturated,
        k: Kont,
    ) -> Lowered {
        let f = self.fresh("f");
        let clause = self.curried_clause(hints.into(), Vec::new(), finish);
        let rest = k.apply(self, f.clone())?;
        Ok(Stmt::New(f, Vec::new(), vec![clause], Box::new(rest)))
    }

    fn curried_clause(
        &mut self,
        mut hints: VecDeque<String>,
        mut got: Vec<Var>,
        finish: Saturated,
    ) -> (Symbol, Branch) {
        let hint = hints.pop_front().unwrap_or_else(|| "x".into());
        let (x, kv) = (self.fresh(&hint), self.continuation("k"));
        got.push(x.clone());
        let body = if hints.is_empty() {
            finish(self, got, kv.clone())
        } else {
            let g = self.fresh("f");
            let inner = self.curried_clause(hints, got, finish);
            Stmt::New(g.clone(), Vec::new(), vec![inner], Box::new(Stmt::Invoke(kv.clone(), Symbol::Ret, vec![g])))
        };
        (Symbol::Apply, Branch { params: vec![x, kv], body })
    }

    /// Compute `args` left to right, then `finish` with their values.
    fn terms(&mut self, env: &Env, mut args: VecDeque<Rc<Core>>, mut got: Vec<Var>, finish: Finish) -> Lowered {
        match args.pop_front() {
            None => finish(self, got),
            Some(a) => {
                let env2 = env.clone();
                self.term(
                    env,
                    &a,
                    Kont::then(move |l, v| {
                        got.push(v);
                        l.terms(&env2, args, got, finish)
                    }),
                )
            }
        }
    }

    /// `f a b ..`, for a function that is already a value: one call each.
    fn apply_all(&mut self, env: &Env, f: Var, mut args: VecDeque<Rc<Core>>, k: Kont) -> Lowered {
        let Some(a) = args.pop_front() else { return k.apply(self, f) };
        let env2 = env.clone();
        self.term(
            env,
            &a,
            Kont::then(move |l, a| {
                let k = if args.is_empty() { k } else { Kont::then(move |l, r| l.apply_all(&env2, r, args, k)) };
                l.with_continuation(k, |kv| Stmt::Invoke(f, Symbol::Apply, vec![a, kv]))
            }),
        )
    }

    /// An application, looked at whole: what is applied decides how.
    fn application(&mut self, env: &Env, head: &Rc<Core>, mut args: VecDeque<Rc<Core>>, k: Kont) -> Lowered {
        if let Core::Opaque(unique, name) = &**head {
            // a type, applied: still a type
            if self.type_constructors.contains(unique) {
                return self.erased(k);
            }
            // an operation with its argument is performed; what it takes
            // before that is erased
            if let Some(info) = self.ops.get(unique).cloned() {
                if args.len() >= info.erased + info.arity {
                    let mut rest: VecDeque<Rc<Core>> = args.split_off(info.erased + info.arity);
                    let taken: VecDeque<Rc<Core>> = args.split_off(info.erased);
                    let symbol = Symbol::Op(info.name.as_str().into(), info.index);
                    let env2 = env.clone();
                    let k = if rest.is_empty() {
                        k
                    } else {
                        Kont::then(move |l, f| l.apply_all(&env2, f, std::mem::take(&mut rest), k))
                    };
                    return self.terms(
                        env,
                        taken,
                        Vec::new(),
                        Box::new(move |l, xs| l.with_continuation(k, |kv| Stmt::Perform(symbol, xs, kv))),
                    );
                }
            }
            // a native with what it takes is the extern itself; its type
            // arguments are erased
            if let Some(info) = self.natives.get(unique).copied() {
                let arity = info.native.arity();
                if args.len() >= info.params + arity {
                    let mut rest: VecDeque<Rc<Core>> = args.split_off(info.params + arity);
                    let taken: VecDeque<Rc<Core>> = args.split_off(info.params);
                    let env2 = env.clone();
                    let k = if rest.is_empty() {
                        k
                    } else {
                        Kont::then(move |l, f| l.apply_all(&env2, f, std::mem::take(&mut rest), k))
                    };
                    return self.terms(
                        env,
                        taken,
                        Vec::new(),
                        Box::new(move |l, xs| {
                            let r = l.fresh("r");
                            let rest = k.apply(l, r.clone())?;
                            Ok(Stmt::Extern(Extern::Native(info.native), xs, vec![Branch { params: vec![r], body: rest }]))
                        }),
                    );
                }
            }
            // a constructor with everything it needs is a `let`; the type
            // arguments it starts with are erased, and not even computed
            if let Some(info) = self.ctors.get(unique).cloned() {
                if args.len() >= info.params + info.fields {
                    let fields: VecDeque<Rc<Core>> = args.drain(info.params..info.params + info.fields).collect();
                    let symbol = Symbol::Con(info.name.as_str().into(), info.tag);
                    return self.terms(
                        env,
                        fields,
                        Vec::new(),
                        Box::new(move |l, fields| {
                            let v = l.fresh("c");
                            let rest = k.apply(l, v.clone())?;
                            Ok(Stmt::Let(v, symbol, fields, Box::new(rest)))
                        }),
                    );
                }
            } else if let Some(direct) = self.direct_of(*unique, name, env)? {
                // a known function with all its arguments is a jump
                if args.len() >= direct.arity {
                    let rest: VecDeque<Rc<Core>> = args.split_off(direct.arity);
                    let env2 = env.clone();
                    let k = if rest.is_empty() { k } else { Kont::then(move |l, f| l.apply_all(&env2, f, rest, k)) };
                    return self.terms(
                        env,
                        args,
                        Vec::new(),
                        Box::new(move |l, args| {
                            l.with_continuation(k, |kv| {
                                let all = direct.captured.iter().cloned().chain(args).chain([kv]).collect();
                                Stmt::Jump(direct.label, all)
                            })
                        }),
                    );
                }
            }
        }
        // a primitive with both its arguments is the operation itself
        if let (Some(p), 2) = (Self::primitive(env, head), args.len()) {
            return self.terms(
                env,
                args,
                Vec::new(),
                Box::new(move |l, args| {
                    let r = l.fresh("r");
                    let rest = k.apply(l, r.clone())?;
                    Ok(Stmt::Extern(Self::extern_of(p), args, vec![Branch { params: vec![r], body: rest }]))
                }),
            );
        }
        // otherwise: the function first, then each argument and its call
        let env2 = env.clone();
        self.term(env, head, Kont::then(move |l, f| l.apply_all(&env2, f, args, k)))
    }

    /// `switch v { .. }`.  Every alternative continues the same way, and the
    /// continuation can only be written once: so unless it is already a
    /// variable (the `case` is in tail position) it is made one here, a join
    /// point for the alternatives to return to.
    fn switch(&mut self, env: &Env, v: Var, alts: &[CaseAlt], k: Kont) -> Lowered {
        let (join, k) = match k {
            Kont::Return(kv) => (None, kv),
            Kont::Then(_) => {
                let (j, r) = (self.continuation("j"), self.fresh("r"));
                let body = k.apply(self, r.clone())?;
                (Some((j.clone(), (Symbol::Ret, Branch { params: vec![r], body }))), j)
            }
        };
        let mut clauses = Vec::new();
        for alt in alts {
            let (symbol, erased) = match alt.head {
                Head::Bool(true) => (Symbol::True, 0),
                Head::Bool(false) => (Symbol::False, 0),
                Head::Con(u) => match self.ctors.get(&u) {
                    Some(info) => (Symbol::Con(info.name.as_str().into(), info.tag), info.params),
                    None => return Err(LowerError::UnknownDefinition(alt.name.clone())),
                },
            };
            let mut inner = env.clone();
            let mut params = Vec::new();
            // an alternative binds all of a constructor's arguments, and the
            // first are erased: there is nothing for them at run time
            for _ in &alt.vars[..erased.min(alt.vars.len())] {
                inner.push(Binding::Erased);
            }
            for x in &alt.vars[erased.min(alt.vars.len())..] {
                // A field has the grade of what it came out of: once from a
                // linear value, as often as wanted from an unrestricted one.
                // The Core does not say which, so no grade is claimed.
                let x = self.bound(x, None);
                inner.push(Binding::Var(x.clone()));
                params.push(x);
            }
            let body = self.term(&inner, &alt.body, Kont::Return(k.clone()))?;
            clauses.push((symbol, Branch { params, body }));
        }
        let switch = Stmt::Switch(v, clauses);
        Ok(match join {
            None => switch,
            Some((j, ret)) => Stmt::New(j, Vec::new(), vec![ret], Box::new(switch)),
        })
    }

    fn term(&mut self, env: &Env, c: &Rc<Core>, k: Kont) -> Lowered {
        match &**c {
            Core::Error => Err(LowerError::ErrorTerm),
            // types and grades are not there at run time
            Core::Type(_)
            | Core::LitTy(_)
            | Core::Pi { .. }
            | Core::BoxTy(..)
            | Core::GradeLit(_)
            | Core::GradeAdd(..)
            | Core::GradeMul(..)
            | Core::GradeRange(..)
            | Core::Comp(..)
            | Core::Row(..) => self.erased(k),
            Core::Handle(body, ret, ops) => self.handle(env, body, ret, ops, k),
            Core::Lit(lit) => {
                let (v, literal) = match lit {
                    Lit::Int(n) => (self.fresh("n"), Ok(Extern::LitInt(*n))),
                    Lit::Char(c) => (self.fresh("c"), Ok(Extern::LitChar(*c))),
                    Lit::Str(s) => (self.fresh("s"), Ok(Extern::LitStr(s.clone()))),
                    Lit::Bool(true) => (self.fresh("b"), Err(Symbol::True)),
                    Lit::Bool(false) => (self.fresh("b"), Err(Symbol::False)),
                    Lit::Unit => (self.fresh("u"), Err(Symbol::Unit)),
                };
                let rest = k.apply(self, v.clone())?;
                Ok(match literal {
                    // integers and strings are external; the rest are data
                    Ok(op) => Stmt::Extern(op, Vec::new(), vec![Branch { params: vec![v], body: rest }]),
                    Err(symbol) => Stmt::Let(v, symbol, Vec::new(), Box::new(rest)),
                })
            }
            Core::Prim(p) => self.primitive_value(*p, k),
            Core::Var(i, name) => {
                let slot = env.len().checked_sub(i + 1).and_then(|l| env.get(l));
                match slot.cloned() {
                    // a variable is already a value: nothing to do but go on
                    Some(Binding::Var(v)) => k.apply(self, v),
                    Some(Binding::Erased) => self.erased(k),
                    Some(Binding::Prim(p)) => self.primitive_value(p, k),
                    None => Err(LowerError::UnboundVariable(name.clone())),
                }
            }
            Core::Opaque(unique, _) if self.type_constructors.contains(unique) => self.erased(k),
            // a constructor on its own: nothing to wait for, or a function
            // that waits for its fields
            Core::Opaque(unique, _) if self.ctors.contains_key(unique) => {
                let info = self.ctors[unique].clone();
                let symbol = Symbol::Con(info.name.as_str().into(), info.tag);
                if info.params + info.fields == 0 {
                    let v = self.fresh("c");
                    let rest = k.apply(self, v.clone())?;
                    return Ok(Stmt::Let(v, symbol, Vec::new(), Box::new(rest)));
                }
                let hints = vec!["x".to_string(); info.params + info.fields];
                let finish = Box::new(move |l: &mut Lowering, mut args: Vec<Var>, kv: Var| {
                    let v = l.fresh("c");
                    let fields = args.split_off(info.params);
                    Stmt::Let(v.clone(), symbol, fields, Box::new(Stmt::Invoke(kv, Symbol::Ret, vec![v])))
                });
                self.curried_value(hints, finish, k)
            }
            // an operation on its own: a function that performs it once it
            // has its arguments
            Core::Opaque(unique, _) if self.ops.contains_key(unique) => {
                let info = self.ops[unique].clone();
                let symbol = Symbol::Op(info.name.as_str().into(), info.index);
                let hints = vec!["x".to_string(); info.erased + info.arity];
                let finish = Box::new(move |_: &mut Lowering, mut args: Vec<Var>, kv: Var| {
                    let xs = args.split_off(args.len() - info.arity);
                    Stmt::Perform(symbol, xs, kv)
                });
                self.curried_value(hints, finish, k)
            }
            // a native on its own: a function that does it once it has what
            // it takes
            Core::Opaque(unique, _) if self.natives.contains_key(unique) => {
                let info = self.natives[unique];
                let hints = vec!["x".to_string(); info.params + info.native.arity()];
                let finish = Box::new(move |l: &mut Lowering, mut args: Vec<Var>, kv: Var| {
                    let xs = args.split_off(info.params);
                    let r = l.fresh("r");
                    let ret = Stmt::Invoke(kv, Symbol::Ret, vec![r.clone()]);
                    Stmt::Extern(Extern::Native(info.native), xs, vec![Branch { params: vec![r], body: ret }])
                });
                self.curried_value(hints, finish, k)
            }
            Core::Opaque(unique, name) => {
                let (label, mut args) = self.label_of(*unique, name, env)?;
                self.with_continuation(k, |kv| {
                    args.push(kv);
                    Stmt::Jump(label, args)
                })
            }
            // Solutions are closed terms; the environment is passed along
            // only so that a recursive definition mentioned in one can find
            // what it captures.
            Core::Meta(m) => match self.meta_solutions.get(*m).cloned().flatten() {
                Some(solution) => self.term(env, &solution, k),
                None => Err(LowerError::UnsolvedMeta(*m)),
            },
            // never looked at, so never computed
            Core::Runtime(Note::Erased, _) => self.erased(k),
            // a match on an erased value its type decides: its one
            // alternative, with nothing for the variables (which are erased
            // too), and without looking at the value
            Core::Runtime(Note::Forced, inner) => match &**inner {
                Core::Case(_, alts) => match alts.first() {
                    Some(alt) => {
                        let mut inner = env.clone();
                        for _ in &alt.vars {
                            inner.push(Binding::Erased);
                        }
                        self.term(&inner, &alt.body, k)
                    }
                    // no alternative: this is never reached
                    None => self.erased(k),
                },
                _ => self.term(env, inner, k),
            },
            Core::Runtime(Note::Binds { grade, .. }, inner) => match &**inner {
                Core::Lam(x, body) => self.lambda(env, x, *grade, body, k),
                Core::Unbox(e) => self.unbox(env, e, *grade, "x", k),
                _ => self.term(env, inner, k),
            },
            Core::Lam(x, body) => self.lambda(env, x, None, body, k),
            Core::App(..) => {
                if let Some(instantiated) = self.solved_hole(env, c) {
                    let (inner, body) = instantiated?;
                    return self.term(&inner, &body, k);
                }
                let mut args = VecDeque::new();
                let mut head = c;
                while let Core::App(g, a) = &**head {
                    args.push_front(a.clone());
                    head = g;
                }
                self.application(env, head, args, k)
            }
            Core::Case(scrutinee, alts) => {
                let (env2, alts) = (env.clone(), alts.clone());
                self.term(env, scrutinee, Kont::then(move |l, v| l.switch(&env2, v, &alts, k)))
            }
            // a `let` binds a value that has been computed: a name for a
            // variable, and no statement at all
            // A type, or a function computing one (an effect alias with
            // parameters): nothing at run time, and every use of it is in a
            // type, or erased.
            Core::Let(_, e, body) if Self::type_former(e) => {
                let mut inner = env.clone();
                inner.push(Binding::Erased);
                self.term(&inner, body, k)
            }
            Core::Let(name, e, body) => {
                let (env2, body) = (env.clone(), body.clone());
                let rest = Kont::then(move |l: &mut Lowering, v| {
                    // An intermediate result was going to be used once.  Now
                    // it has a name, and is used as often as the name is —
                    // which no grade records: a `let` has none.
                    if l.temporaries.remove(&v) {
                        l.grades.remove(&v);
                    }
                    let mut inner = env2;
                    inner.push(Binding::Var(v));
                    l.term(&inner, &body, k)
                });
                // `let [x] = b in ..`: what comes out of the box is `x`
                match &**e {
                    Core::Unbox(b) => self.unbox(env, b, None, name, rest),
                    Core::Runtime(Note::Binds { grade, .. }, inner) if matches!(**inner, Core::Unbox(_)) => {
                        let Core::Unbox(b) = &**inner else { unreachable!() };
                        self.unbox(env, b, *grade, name, rest)
                    }
                    _ => self.term(env, e, rest),
                }
            }
            Core::BoxIntro(e) => self.term(
                env,
                e,
                Kont::then(move |l, v| {
                    let b = l.fresh("box");
                    let rest = k.apply(l, b.clone())?;
                    Ok(Stmt::Let(b, Symbol::Box, vec![v], Box::new(rest)))
                }),
            ),
            Core::Unbox(e) => self.unbox(env, e, None, "x", k),
        }
    }

    /// Whether a term is a type, a grade or a row, or a function giving one.
    fn type_former(c: &Core) -> bool {
        match c {
            Core::Type(_)
            | Core::LitTy(_)
            | Core::Pi { .. }
            | Core::BoxTy(..)
            | Core::GradeLit(_)
            | Core::GradeAdd(..)
            | Core::GradeMul(..)
            | Core::GradeRange(..)
            | Core::Comp(..)
            | Core::Row(..) => true,
            Core::Lam(_, body) | Core::Runtime(Note::Binds { .. }, body) => Self::type_former(body),
            _ => false,
        }
    }

    /// `handle body with clauses`: the handler as a consumer, then the body
    /// under it, sending its value to the handler's `return`.
    fn handle(&mut self, env: &Env, body: &Rc<Core>, ret: &Rc<Core>, ops: &[OpClause], k: Kont) -> Lowered {
        let (h, r) = (self.fresh("h"), self.continuation("ret"));
        let mut clauses = Vec::new();
        // `return(x, k)`
        let (inner, params, body_of) = self.open(env, ret, 1)?;
        let kc = self.continuation("k");
        let ret_body = self.term(&inner, &body_of, Kont::Return(kc.clone()))?;
        clauses.push((Symbol::Return, Branch { params: params.into_iter().chain([kc]).collect(), body: ret_body }));
        // `op(x, .., r, k)`, the erased arguments first bound to nothing
        for (index, op) in ops.iter().enumerate() {
            let mut outer = env.clone();
            let mut clause = op.body.clone();
            for _ in 0..op.erased {
                outer.push(Binding::Erased);
                clause = Self::under_lambda(&clause).ok_or(LowerError::ErrorTerm)?;
            }
            let arity = self.ops.get(&op.op).map_or(1, |info| info.arity);
            let (inner, params, body_of) = self.open(&outer, &clause, arity + 1)?;
            let kc = self.continuation("k");
            let op_body = self.term(&inner, &body_of, Kont::Return(kc.clone()))?;
            let symbol = Symbol::Op(op.name.as_str().into(), index);
            clauses.push((symbol, Branch { params: params.into_iter().chain([kc]).collect(), body: op_body }));
        }
        let body = self.term(env, body, Kont::Return(r.clone()))?;
        self.with_continuation(k, |kv| {
            Stmt::New(h.clone(), Vec::new(), clauses, Box::new(Stmt::Handle(h, kv, r, Box::new(body))))
        })
    }

    /// What is under a lambda, looking through the note on it.
    fn under_lambda(c: &Rc<Core>) -> Option<Rc<Core>> {
        match &**c {
            Core::Lam(_, body) => Some(body.clone()),
            Core::Runtime(_, inner) => Self::under_lambda(inner),
            _ => None,
        }
    }

    /// The first `n` lambdas of `c` as parameters: the environment under
    /// them, the variables they bind, and the body.
    fn open(&mut self, env: &Env, c: &Rc<Core>, n: usize) -> Result<(Env, Vec<Var>, Rc<Core>), LowerError> {
        let mut inner = env.clone();
        let mut params = Vec::new();
        let mut c = c.clone();
        for _ in 0..n {
            let (x, grade, body) = match &*c {
                Core::Lam(x, body) => (x.clone(), None, body.clone()),
                Core::Runtime(Note::Binds { grade, .. }, lam) => match &**lam {
                    Core::Lam(x, body) => (x.clone(), *grade, body.clone()),
                    _ => return Err(LowerError::ErrorTerm),
                },
                _ => return Err(LowerError::ErrorTerm),
            };
            let v = self.bound(&x, grade);
            inner.push(Binding::Var(v.clone()));
            params.push(v);
            c = body;
        }
        Ok((inner, params, c))
    }

    fn lambda(&mut self, env: &Env, x: &str, grade: Option<Grade>, body: &Rc<Core>, k: Kont) -> Lowered {
        let (f, x, kv) = (self.fresh("f"), self.bound(x, grade), self.continuation("k"));
        let mut inner = env.clone();
        inner.push(Binding::Var(x.clone()));
        let body = self.term(&inner, body, Kont::Return(kv.clone()))?;
        let rest = k.apply(self, f.clone())?;
        let apply = (Symbol::Apply, Branch { params: vec![x, kv], body });
        Ok(Stmt::New(f, Vec::new(), vec![apply], Box::new(rest)))
    }

    /// `grade` is the box's: what comes out of it may be used that often.
    fn unbox(&mut self, env: &Env, e: &Rc<Core>, grade: Option<Grade>, hint: &str, k: Kont) -> Lowered {
        let hint = hint.to_string();
        self.term(
            env,
            e,
            Kont::then(move |l, b| {
                let x = l.bound(&hint, grade);
                let rest = k.apply(l, x.clone())?;
                Ok(Stmt::Switch(b, vec![(Symbol::Box, Branch { params: vec![x], body: rest })]))
            }),
        )
    }

    /// A hole the checker filled in, `?m x y z` with `?m := \a b c. body`.
    ///
    /// A metavariable is applied to *every* variable in scope, whether or not
    /// its solution mentions them.  Lowered as the application it looks
    /// like, that would hand each of them to a function that mostly ignores
    /// them — copying linear variables for nothing.  So it is instantiated
    /// here instead: the body is lowered with its parameters standing for the
    /// arguments, and only what the solution really uses is touched.
    ///
    /// `None` if `c` is not of that shape (then it is lowered as it stands).
    fn solved_hole(&self, env: &Env, c: &Rc<Core>) -> Option<Result<(Env, Rc<Core>), LowerError>> {
        let mut args: Vec<Binding> = Vec::new();
        let mut head = c;
        while let Core::App(f, a) = &**head {
            let Core::Var(i, _) = &**a else { return None };
            args.push(env.get(env.len().checked_sub(i + 1)?)?.clone());
            head = f;
        }
        let Core::Meta(m) = &**head else { return None };
        let Some(mut body) = self.meta_solutions.get(*m).cloned().flatten() else {
            return Some(Err(LowerError::UnsolvedMeta(*m)));
        };
        // the solution abstracts over the spine, outermost argument first
        args.reverse();
        let mut inner = env.clone();
        for arg in args {
            let Core::Lam(_, next) = &*body else { return None };
            let next = next.clone();
            inner.push(arg);
            body = next;
        }
        Some(Ok((inner, body)))
    }

    fn erased(&mut self, k: Kont) -> Lowered {
        let v = self.fresh("t");
        let rest = k.apply(self, v.clone())?;
        Ok(Stmt::Let(v, Symbol::Erased, Vec::new(), Box::new(rest)))
    }
}

// ---------------------------------------------------------------------------
// Labels take only what they use
// ---------------------------------------------------------------------------

/// A recursive definition is lifted over *every* variable in scope where it
/// was defined, because its body is not known when the first jump to it is
/// lowered.  Most of those it never touches, and passing them anyway would
/// copy a linear variable at each reference for nothing.  So afterwards,
/// each label's parameters are cut down to the ones its body needs — a
/// fixed point, since a label needs what the labels it jumps to need.
fn prune_labels(definitions: &mut [Definition]) {
    let mut needed: HashMap<Label, Vec<bool>> =
        definitions.iter().map(|d| (d.label.clone(), vec![false; d.params.len()])).collect();
    loop {
        let passed = |l: &Label, args: &[Var]| -> Vec<Var> {
            let keep = needed.get(l);
            args.iter().enumerate().filter(|(i, _)| keep.is_none_or(|k| k[*i])).map(|(_, a)| a.clone()).collect()
        };
        let next: HashMap<Label, Vec<bool>> = definitions
            .iter()
            .map(|d| {
                let free = free_vars_where(&d.body, &passed);
                (d.label.clone(), d.params.iter().map(|p| free.contains(p)).collect())
            })
            .collect();
        if next == needed {
            break;
        }
        needed = next;
    }
    fn keep<T>(items: &mut Vec<T>, mask: &[bool]) {
        let mut mask = mask.iter();
        items.retain(|_| *mask.next().unwrap_or(&true));
    }
    fn jumps(s: &mut Stmt, needed: &HashMap<Label, Vec<bool>>) {
        match s {
            Stmt::Jump(l, args) => {
                if let Some(mask) = needed.get(l) {
                    keep(args, mask);
                }
            }
            Stmt::Substitute(_, rest) | Stmt::Let(_, _, _, rest) => jumps(rest, needed),
            Stmt::New(_, _, clauses, rest) => {
                clauses.iter_mut().for_each(|(_, b)| jumps(&mut b.body, needed));
                jumps(rest, needed);
            }
            Stmt::Switch(_, clauses) => clauses.iter_mut().for_each(|(_, b)| jumps(&mut b.body, needed)),
            Stmt::Extern(_, _, branches) => branches.iter_mut().for_each(|b| jumps(&mut b.body, needed)),
            Stmt::Handle(_, _, _, rest) => jumps(rest, needed),
            Stmt::Invoke(..) | Stmt::Perform(..) => {}
        }
    }
    for d in definitions.iter_mut() {
        keep(&mut d.params, &needed[&d.label]);
        jumps(&mut d.body, &needed);
    }
}
