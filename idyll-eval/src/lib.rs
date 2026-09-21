//! The reference interpreter for Idyll: a CEK machine for elaborated Core.
//!
//! This is the specification of what an Idyll program *computes*.  It is
//! meant to be read, and to be obviously right, rather than to be fast: the
//! compiler lowers the same Core to AxCut, runs it on the paper's abstract
//! machine (`idyllic_seq::machine`), and compiles it to native code, and all
//! of that has to agree with what happens here.  `tests/spec.rs` holds it to
//! that.
//!
//! # The machine
//!
//! A state is a **C**ontrol — a term to evaluate in an environment, or a
//! value being returned — an **E**nvironment, and a **K**ontinuation: an
//! explicit stack of [`Frame`]s saying what to do with the value once there
//! is one.  One step either takes a term apart, pushing a frame for what is
//! left to do, or hands a value to the frame on top.  So the interpreter
//! never recurses, and a deep computation needs a big `Vec`, not a big Rust
//! stack.
//!
//! ```text
//! eval  x          E  K        →  ret E(x)                K
//! eval  \x -> b    E  K        →  ret ⟨\x -> b, E⟩         K
//! eval  f a        E  K        →  eval f  E               arg(a, E) : K
//! ret   v             arg(a, E) : K  →  eval a  E        call(v) : K
//! ret   w             call(⟨\x -> b, E⟩) : K  →  eval b  E, x ↦ w   K
//! eval  let x = e in b   E  K  →  eval e  E               let(b, E) : K
//! ret   v             let(b, E) : K  →  eval b  E, x ↦ v   K
//! eval  case e of alts   E  K  →  eval e  E               case(alts, E) : K
//! ret   C v..         case(alts, E) : K  →  eval body  E, x.. ↦ v..   K
//! ```
//!
//! # What it means to run Core
//!
//! Core is a dependently typed lambda calculus, and most of what is in a
//! term is there for the type checker.  Running it means deciding, rule by
//! rule, what is *computation*:
//!
//! * **Call by value, left to right.**  A function is evaluated before its
//!   argument, an argument before the call, a `let`'s right-hand side before
//!   its body, a scrutinee before its alternatives.
//! * **Types and grades are erased.**  `Type`, `Int`, function types, boxes'
//!   types and grade arithmetic evaluate to [`Value::Erased`], as do the
//!   declared data types.
//! * **What is never used is never computed.**  A term the elaborator marked
//!   [`Note::Erased`] — an argument to a grade-`0` parameter, which includes
//!   every implicit type argument, or the contents of a grade-`0` box — is
//!   [`Value::Erased`] without being looked at, so one that would loop
//!   forever does no harm.  Likewise the type parameters a constructor is
//!   applied to, and anything a type is applied to.
//! * **Recursive definitions are constants** in Core ([`Core::Opaque`]),
//!   with their bodies kept to one side ([`RecDef`]).  Referring to one
//!   evaluates its body again, in the context it was defined in.  For a
//!   function that only builds a closure; for anything else it recomputes
//!   the value, which is also what the compiled code does.
//! * **Metavariables** the checker solved stand for their solutions, which
//!   are closed terms.
//! * Integers are unbounded.
//!
//! The answer is shown as the REPL shows it: numbers, strings, `True`,
//! `()`, boxes as `[v]`, constructors applied to their fields, lists with
//! their `::` written infix.  A function or a type has nothing to show.

use idyllic_ast::Lit;
use idyllic_builtins::{Builtin, Prim};
use idyllic_typecheck::core::{CaseAlt, Core, Head, Note};
use idyllic_typecheck::{CtorInfo, RecDef, TypeCheckResult};
use num_bigint::BigInt;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Values and environments
// ---------------------------------------------------------------------------

/// What a term evaluates to.
#[derive(Clone, Debug)]
pub enum Value {
    Int(BigInt),
    Str(Rc<str>),
    Bool(bool),
    Unit,
    /// A constructor applied to all its fields.
    Con { name: Rc<str>, tag: usize, fields: Vec<Rc<Value>> },
    /// A promoted value, `[v]`.
    Box(Rc<Value>),
    /// A lambda and the environment it was made in.
    Closure { env: Env, body: Rc<Core> },
    /// A primitive operation, waiting for the rest of its arguments.
    Prim { prim: Prim, args: Vec<Rc<Value>> },
    /// A constructor still waiting for arguments: `seen` of them have come,
    /// the first `info.params` of which are erased type parameters.
    Ctor { info: Rc<CtorInfo>, seen: usize, fields: Vec<Rc<Value>> },
    /// A type, a grade, or something never used: nothing at run time.
    Erased,
}

/// An environment: a persistent list of values, innermost first, so that a
/// de Bruijn index counts along it.  Extending one shares the rest.
#[derive(Clone, Debug, Default)]
pub struct Env(Option<Rc<Cell>>);

#[derive(Debug)]
struct Cell {
    value: Rc<Value>,
    next: Env,
    /// how many values the environment holds: its innermost one's level + 1
    len: usize,
}

impl Env {
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |c| c.len)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    pub fn push(&self, value: Rc<Value>) -> Env {
        Env(Some(Rc::new(Cell { value, next: self.clone(), len: self.len() + 1 })))
    }

    /// The value at de Bruijn index `i`.
    fn get(&self, mut i: usize) -> Option<Rc<Value>> {
        let mut at = self;
        loop {
            let cell = at.0.as_ref()?;
            if i == 0 {
                return Some(cell.value.clone());
            }
            i -= 1;
            at = &cell.next;
        }
    }

    /// The outermost `lvl` values: the environment as it was when it was
    /// that long, which is where a recursive definition's body belongs.
    fn at_level(&self, lvl: usize) -> Option<Env> {
        let mut at = self.clone();
        while at.len() > lvl {
            at = at.0.as_ref()?.next.clone();
        }
        (at.len() == lvl).then_some(at)
    }
}

// Values and environments nest as deeply as the data does — a list of a
// million elements is a million nested values — and the drop glue Rust
// derives would recurse once per level and overflow the stack.  So both are
// taken apart with an explicit stack instead: whatever a value holds is
// moved out onto it, and dropped from there once nothing else shares it.

/// Something to be dropped, once its last reference is.
enum Garbage {
    Value(Rc<Value>),
    Cell(Rc<Cell>),
}

thread_local! {
    /// What a field holds once its contents have been moved out.
    static HOLLOW: Rc<Value> = Rc::new(Value::Unit);
}

fn hollow() -> Rc<Value> {
    // (a value dropped while the thread is being torn down gets a fresh one)
    HOLLOW.try_with(Rc::clone).unwrap_or_else(|_| Rc::new(Value::Unit))
}

fn dismantle(mut pending: Vec<Garbage>) {
    while let Some(g) = pending.pop() {
        // (what is dropped here has had its contents moved out, so dropping
        // it does not recurse)
        match g {
            Garbage::Value(v) => {
                if let Ok(mut v) = Rc::try_unwrap(v) {
                    v.hand_over(&mut pending);
                }
            }
            Garbage::Cell(c) => {
                if let Ok(mut c) = Rc::try_unwrap(c) {
                    c.hand_over(&mut pending);
                }
            }
        }
    }
}

impl Value {
    /// Move whatever this value holds onto `out`.
    fn hand_over(&mut self, out: &mut Vec<Garbage>) {
        match self {
            Value::Con { fields, .. } | Value::Prim { args: fields, .. } | Value::Ctor { fields, .. } => {
                out.extend(fields.drain(..).map(Garbage::Value));
            }
            Value::Box(inner) => out.push(Garbage::Value(std::mem::replace(inner, hollow()))),
            Value::Closure { env, .. } => out.extend(env.0.take().map(Garbage::Cell)),
            Value::Int(_) | Value::Str(_) | Value::Bool(_) | Value::Unit | Value::Erased => {}
        }
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        self.hand_over(&mut pending);
        dismantle(pending);
    }
}

impl Cell {
    fn hand_over(&mut self, out: &mut Vec<Garbage>) {
        out.push(Garbage::Value(std::mem::replace(&mut self.value, hollow())));
        out.extend(self.next.0.take().map(Garbage::Cell));
    }
}

impl Drop for Cell {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        self.hand_over(&mut pending);
        dismantle(pending);
    }
}

// ---------------------------------------------------------------------------
// The machine
// ---------------------------------------------------------------------------

/// What to do with a value, once there is one.
#[derive(Debug)]
pub enum Frame {
    /// The function of an application has been evaluated; the argument is
    /// next.
    Arg { arg: Rc<Core>, env: Env },
    /// The argument has been evaluated too; now the call.
    Call { fun: Rc<Value> },
    /// The scrutinee of a `case`; the alternatives are next.
    Case { alts: Rc<Vec<CaseAlt>>, env: Env },
    /// The right-hand side of a `let`; the body is next.
    Let { body: Rc<Core>, env: Env },
    /// The contents of a promotion `[e]`.
    BoxIntro,
    /// What `unbox` is to take apart.
    Unbox,
}

/// The control: a term to evaluate, or a value to return.
enum Control {
    Eval(Rc<Core>, Env),
    Return(Rc<Value>),
}

/// Why evaluation stopped without an answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvalError {
    /// Still going after the steps it was given.
    OutOfFuel,
    /// Nowhere to go: a term that could not have type checked.
    Stuck(String),
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::OutOfFuel => write!(f, "out of fuel"),
            EvalError::Stuck(why) => write!(f, "stuck: {why}"),
        }
    }
}

fn stuck<T>(why: impl Into<String>) -> Result<T, EvalError> {
    Err(EvalError::Stuck(why.into()))
}

/// How evaluation ended.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub value: Rc<Value>,
    /// How the REPL shows it, if it is plain data.
    pub answer: Option<String>,
    /// How many steps it took.
    pub steps: u64,
}

/// What the machine needs to know about the program besides the term: the
/// bodies of the recursive definitions, the constructors, the data types,
/// and the solutions of the metavariables.
struct Program<'c> {
    rec_defs: HashMap<usize, &'c RecDef>,
    ctors: HashMap<usize, Rc<CtorInfo>>,
    type_constructors: Vec<usize>,
    meta_solutions: &'c [Option<Rc<Core>>],
}

/// Evaluate a checked program's trailing expression, taking at most `fuel`
/// steps.  `None` if it has no expression, only declarations.
pub fn evaluate(checked: &TypeCheckResult, fuel: u64) -> Option<Result<Outcome, EvalError>> {
    let term = checked.program.clone()?;
    let program = Program {
        rec_defs: checked.rec_defs.iter().map(|d| (d.unique, d)).collect(),
        ctors: checked.ctors.iter().map(|(u, info)| (*u, Rc::new(info.clone()))).collect(),
        type_constructors: checked.type_constructors.clone(),
        meta_solutions: &checked.meta_solutions,
    };
    Some(program.run(term, initial_env(), fuel))
}

/// The context every program is elaborated in: the builtin types, then the
/// primitive operations.
pub fn initial_env() -> Env {
    let types = Builtin::ALL.iter().map(|_| Value::Erased);
    let prims = Prim::ALL.iter().map(|&prim| Value::Prim { prim, args: Vec::new() });
    types.chain(prims).fold(Env::default(), |env, v| env.push(Rc::new(v)))
}

impl Program<'_> {
    fn run(&self, term: Rc<Core>, env: Env, fuel: u64) -> Result<Outcome, EvalError> {
        let mut control = Control::Eval(term, env);
        let mut kont: Vec<Frame> = Vec::new();
        let mut steps = 0;
        loop {
            if steps == fuel {
                return Err(EvalError::OutOfFuel);
            }
            steps += 1;
            control = match control {
                Control::Eval(term, env) => self.eval(&term, env, &mut kont)?,
                Control::Return(value) => match kont.pop() {
                    None => {
                        let answer = show(&value);
                        return Ok(Outcome { value, answer, steps });
                    }
                    Some(frame) => self.ret(value, frame, &mut kont)?,
                },
            };
        }
    }

    /// Take a term apart: return what it is already, or start on its first
    /// part and push a frame for the rest.
    fn eval(&self, term: &Rc<Core>, env: Env, kont: &mut Vec<Frame>) -> Result<Control, EvalError> {
        let value = |v: Value| Ok(Control::Return(Rc::new(v)));
        match &**term {
            // types and grades are not there at run time
            Core::Type
            | Core::LitTy(_)
            | Core::Pi { .. }
            | Core::BoxTy(..)
            | Core::GradeLit(_)
            | Core::GradeAdd(..)
            | Core::GradeMul(..)
            | Core::GradeRange(..) => value(Value::Erased),
            Core::Lit(lit) => value(match lit {
                Lit::Int(n) => Value::Int(n.clone()),
                Lit::Str(s) => Value::Str(s.as_str().into()),
                Lit::Bool(b) => Value::Bool(*b),
                Lit::Unit => Value::Unit,
            }),
            Core::Prim(prim) => value(Value::Prim { prim: *prim, args: Vec::new() }),
            Core::Var(i, name) => match env.get(*i) {
                Some(v) => Ok(Control::Return(v)),
                None => stuck(format!("{name} is not in the environment")),
            },
            Core::Opaque(unique, name) => {
                if self.type_constructors.contains(unique) {
                    return value(Value::Erased);
                }
                if let Some(info) = self.ctors.get(unique) {
                    return value(saturate(info.clone(), 0, Vec::new()));
                }
                // a recursive definition: its body, where it was defined
                let Some(def) = self.rec_defs.get(unique) else { return stuck(format!("no definition of {name}")) };
                match env.at_level(def.lvl) {
                    Some(outer) => Ok(Control::Eval(def.core.clone(), outer)),
                    None => stuck(format!("{name} is used outside the context it was defined in")),
                }
            }
            Core::Meta(m) => match self.meta_solutions.get(*m).cloned().flatten() {
                Some(solution) => Ok(Control::Eval(solution, env)),
                None => stuck(format!("metavariable ?{m} was never solved")),
            },
            // never looked at, so never computed
            Core::Runtime(Note::Erased, _) => value(Value::Erased),
            Core::Runtime(Note::Binds(_), inner) => Ok(Control::Eval(inner.clone(), env)),
            Core::Lam(_, body) => value(Value::Closure { env, body: body.clone() }),
            Core::App(f, a) => {
                kont.push(Frame::Arg { arg: a.clone(), env: env.clone() });
                Ok(Control::Eval(f.clone(), env))
            }
            Core::Case(scrutinee, alts) => {
                kont.push(Frame::Case { alts: alts.clone(), env: env.clone() });
                Ok(Control::Eval(scrutinee.clone(), env))
            }
            Core::Let(_, e, body) => {
                kont.push(Frame::Let { body: body.clone(), env: env.clone() });
                Ok(Control::Eval(e.clone(), env))
            }
            Core::BoxIntro(e) => {
                kont.push(Frame::BoxIntro);
                Ok(Control::Eval(e.clone(), env))
            }
            Core::Unbox(e) => {
                kont.push(Frame::Unbox);
                Ok(Control::Eval(e.clone(), env))
            }
            Core::Error => stuck("the program contains an error"),
        }
    }

    /// Hand a value to the frame that was waiting for it.
    fn ret(&self, v: Rc<Value>, frame: Frame, kont: &mut Vec<Frame>) -> Result<Control, EvalError> {
        match frame {
            Frame::Arg { arg, env } => {
                if evaluates_argument(&v) {
                    kont.push(Frame::Call { fun: v });
                    Ok(Control::Eval(arg, env))
                } else {
                    apply(&v, Rc::new(Value::Erased))
                }
            }
            Frame::Call { fun } => apply(&fun, v),
            Frame::Case { alts, env } => {
                for alt in alts.iter() {
                    let fields: &[Rc<Value>] = match (alt.head, &*v) {
                        (Head::Bool(b), Value::Bool(c)) if b == *c => &[],
                        (Head::Con(u), Value::Con { tag, fields, .. })
                            if self.ctors.get(&u).is_some_and(|info| info.tag == *tag) =>
                        {
                            fields
                        }
                        _ => continue,
                    };
                    // the last field is the innermost binder
                    let env = fields.iter().fold(env, |env, f| env.push(f.clone()));
                    return Ok(Control::Eval(alt.body.clone(), env));
                }
                stuck("no alternative matches")
            }
            Frame::Let { body, env } => Ok(Control::Eval(body, env.push(v))),
            Frame::BoxIntro => Ok(Control::Return(Rc::new(Value::Box(v)))),
            Frame::Unbox => match &*v {
                Value::Box(inner) => Ok(Control::Return(inner.clone())),
                _ => stuck("unboxing something that is not a box"),
            },
        }
    }
}

/// Whether applying `fun` looks at its argument.  A type applied to
/// anything is still a type, and a constructor's type parameters are erased:
/// neither argument is computed.
fn evaluates_argument(fun: &Value) -> bool {
    match fun {
        Value::Erased => false,
        Value::Ctor { info, seen, .. } => *seen >= info.params,
        _ => true,
    }
}

/// A constructor, given `seen` arguments of which these are the `fields`:
/// the value itself once it has them all.
fn saturate(info: Rc<CtorInfo>, seen: usize, fields: Vec<Rc<Value>>) -> Value {
    if seen == info.params + info.fields {
        Value::Con { name: info.name.as_str().into(), tag: info.tag, fields }
    } else {
        Value::Ctor { info, seen, fields }
    }
}

fn apply(fun: &Value, arg: Rc<Value>) -> Result<Control, EvalError> {
    let value = |v: Value| Ok(Control::Return(Rc::new(v)));
    match fun {
        Value::Closure { env, body } => Ok(Control::Eval(body.clone(), env.push(arg))),
        Value::Prim { prim, args } => {
            let mut args = args.clone();
            args.push(arg);
            if args.len() < prim.arity() {
                return value(Value::Prim { prim: *prim, args });
            }
            let (Value::Int(a), Value::Int(b)) = (&*args[0], &*args[1]) else {
                return stuck(format!("{} is applied to something that is not an integer", prim.name()));
            };
            value(match prim {
                Prim::IntAdd => Value::Int(a + b),
                Prim::IntMul => Value::Int(a * b),
                Prim::IntSub => Value::Int(a - b),
                Prim::IntLt => Value::Bool(a < b),
                Prim::IntEq => Value::Bool(a == b),
            })
        }
        Value::Ctor { info, seen, fields } => {
            let mut fields = fields.clone();
            if *seen >= info.params {
                fields.push(arg);
            }
            value(saturate(info.clone(), seen + 1, fields))
        }
        Value::Erased => value(Value::Erased),
        _ => stuck("applying something that is not a function"),
    }
}

// ---------------------------------------------------------------------------
// Showing the answer
// ---------------------------------------------------------------------------

/// A value as the REPL shows it, if it is plain data: a function or a type
/// has nothing worth showing, and neither has anything that contains one.
pub fn show(v: &Value) -> Option<String> {
    // a field that is itself an application is parenthesised
    let atom = |s: String| if s.contains(' ') && !s.starts_with(['[', '"']) { format!("({s})") } else { s };
    match v {
        Value::Int(n) => Some(n.to_string()),
        Value::Str(s) => Some(format!("{:?}", &**s)),
        Value::Bool(true) => Some("True".into()),
        Value::Bool(false) => Some("False".into()),
        Value::Unit => Some("()".into()),
        Value::Box(inner) => Some(format!("[{}]", show(inner)?)),
        // `x :: xs` is written the way it is read
        Value::Con { name, fields, .. } if &**name == "::" && fields.len() == 2 => {
            Some(format!("{} :: {}", atom(show(&fields[0])?), show(&fields[1])?))
        }
        Value::Con { name, fields, .. } => {
            let mut out = name.to_string();
            for f in fields {
                out.push(' ');
                out.push_str(&atom(show(f)?));
            }
            Some(out)
        }
        Value::Closure { .. } | Value::Prim { .. } | Value::Ctor { .. } | Value::Erased => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A value as deep as a long list, and an environment as long, are
    /// dropped without recursing (the derived drop would overflow).
    #[test]
    fn deep_values_and_environments_drop_without_recursing() {
        let mut list = Rc::new(Value::Con { name: "Nil".into(), tag: 0, fields: Vec::new() });
        let mut env = Env::default();
        for n in 0..1_000_000 {
            let head = Rc::new(Value::Int(n.into()));
            list = Rc::new(Value::Con { name: "::".into(), tag: 1, fields: vec![head, list] });
            env = env.push(Rc::new(Value::Box(Rc::new(Value::Unit))));
        }
        let closure = Value::Closure { env, body: Rc::new(Core::Type) };
        drop(list);
        drop(closure);
    }

    #[test]
    fn a_shared_tail_survives_its_first_owner() {
        let tail = Rc::new(Value::Con { name: "Nil".into(), tag: 0, fields: Vec::new() });
        let one = Rc::new(Value::Int(1.into()));
        let list = Value::Con { name: "::".into(), tag: 1, fields: vec![one, tail.clone()] };
        drop(list);
        assert_eq!(show(&tail).as_deref(), Some("Nil"));
    }
}
