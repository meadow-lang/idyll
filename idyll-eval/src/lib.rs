//! The reference interpreter for Idyll: a CEK machine for elaborated Core.
//!
//! This is the specification of what an Idyll program *computes*.  It is
//! meant to be read, and to be obviously right, rather than to be fast: the
//! compiler lowers the same Core to Cut, which Meadow's runtimes run --
//! Glade, its bytecode, and Silo, native code -- and what they compute has
//! to agree with what happens here.  `tests/spec.rs` holds Glade to that.
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
//! * Integers are 64 bits and wrap, as the runtime's are.
//!
//! * **Natives** (`@native arrayGet : ..`) are what their documentation in
//!   `idyllic_builtins::Native` says.  An array is a value holding its
//!   elements; setting one makes a new array, the old one untouched.
//!
//! The answer is shown as the REPL shows it: numbers, strings, `True`,
//! `()`, boxes as `[v]`, constructors applied to their fields, lists with
//! their `::` written infix, arrays as `#[a; b]`.  A function or a type has
//! nothing to show.

use idyllic_ast::Lit;
use idyllic_builtins::num::{self, Number};
use idyllic_builtins::{strings, Builtin, IntResult, Native, Prim};
use idyllic_typecheck::core::{CaseAlt, Core, Head, Note, OpClause};
use idyllic_typecheck::handlers::OpInfo;
use idyllic_typecheck::natives::NativeInfo;
use idyllic_typecheck::{CtorInfo, RecDef, TypeCheckResult};
use idyllic_world::{Answer, Arg, Stop, World};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Values and environments
// ---------------------------------------------------------------------------

/// What a term evaluates to.
#[derive(Clone, Debug)]
pub enum Value {
    Int(i64),
    /// A sized integer or a float (an `Int` is [`Value::Int`]).
    Num(Number),
    Str(Rc<str>),
    /// A character, a Unicode scalar value.
    Char(char),
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
    /// An operation still waiting for arguments: `seen` of them have come,
    /// the first `info.erased` of which are erased, and the rest are `args`.
    /// It is performed once it has `info.arity` of those.
    Op { op: usize, info: Rc<OpInfo>, seen: usize, args: Vec<Rc<Value>> },
    /// An array, of its elements.
    Array(Vec<Rc<Value>>),
    /// A native still waiting for arguments: `seen` of them have come, the
    /// first `info.params` of which are erased, and the rest are `args`.
    Native { info: NativeInfo, seen: usize, args: Vec<Rc<Value>> },
    /// The rest of a handled computation, from where it performed an
    /// operation up to and including its handler: frames to put back.
    Resumption(Rc<Vec<Frame>>),
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
            Value::Con { fields, .. }
            | Value::Prim { args: fields, .. }
            | Value::Ctor { fields, .. }
            | Value::Op { args: fields, .. }
            | Value::Array(fields)
            | Value::Native { args: fields, .. } => {
                out.extend(fields.drain(..).map(Garbage::Value));
            }
            Value::Box(inner) => out.push(Garbage::Value(std::mem::replace(inner, hollow()))),
            Value::Closure { env, .. } => out.extend(env.0.take().map(Garbage::Cell)),
            // (a resumption's frames are few, and dropped the ordinary way)
            Value::Int(_) | Value::Num(_) | Value::Str(_) | Value::Char(_) | Value::Bool(_) | Value::Unit | Value::Erased | Value::Resumption(_) => {}
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
#[derive(Clone, Debug)]
pub enum Frame {
    /// A handled computation: what its value becomes, and what its
    /// operations do.  Performing one takes the frames down to here.
    Handle { ret: Rc<Core>, ops: Rc<Vec<OpClause>>, env: Env },
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
    /// An operation, given its argument, looking for its handler.
    Perform(usize, Vec<Rc<Value>>),
    /// A resumption, given a value: its frames go back on.
    Resume(Rc<Vec<Frame>>, Rc<Value>),
}

/// Why evaluation stopped without an answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvalError {
    /// Still going after the steps it was given.
    OutOfFuel,
    /// Nowhere to go: a term that could not have type checked.
    Stuck(String),
    /// An error of the program's own: dividing by zero, an index outside an
    /// array.
    Stopped(String),
    /// The program ended itself: `exit n`.
    Exited(i32),
    /// `fail message`: a test failed.
    TestFailed(String),
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::OutOfFuel => write!(f, "out of fuel"),
            EvalError::Stuck(why) => write!(f, "stuck: {why}"),
            EvalError::Stopped(why) => f.write_str(why),
            EvalError::Exited(code) => write!(f, "exited with {code}"),
            EvalError::TestFailed(why) => write!(f, "test failed: {why}"),
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
    /// What it wrote to the console.
    pub output: String,
}

/// What the machine needs to know about the program besides the term: the
/// bodies of the recursive definitions, the constructors, the data types,
/// and the solutions of the metavariables.
struct Program<'c> {
    rec_defs: HashMap<usize, &'c RecDef>,
    ctors: HashMap<usize, Rc<CtorInfo>>,
    ops: HashMap<usize, Rc<OpInfo>>,
    natives: HashMap<usize, NativeInfo>,
    type_constructors: Vec<usize>,
    meta_solutions: &'c [Option<Rc<Core>>],
}

/// Evaluate a checked program's trailing expression, taking at most `fuel`
/// steps.  `None` if it has no expression, only declarations.
pub fn evaluate(checked: &TypeCheckResult, fuel: u64) -> Option<Result<Outcome, EvalError>> {
    let mut world = World::captured();
    let result = evaluate_in(checked, fuel, &mut world)?;
    Some(result.map(|outcome| Outcome { output: world.into_output(), ..outcome }))
}

/// The same, in `world`, which does the operations of the world's effects
/// that the program does not handle itself (see [`idyllic_world`]).  What
/// they print is there even if the program stops early; the outcome's
/// `output` is left empty.
pub fn evaluate_in(checked: &TypeCheckResult, fuel: u64, world: &mut World) -> Option<Result<Outcome, EvalError>> {
    let term = checked.program.clone()?;
    let program = Program {
        rec_defs: checked.rec_defs.iter().map(|d| (d.unique, d)).collect(),
        ctors: checked.ctors.iter().map(|(u, info)| (*u, Rc::new(info.clone()))).collect(),
        ops: checked.ops.iter().map(|(u, info)| (*u, Rc::new(info.clone()))).collect(),
        natives: checked.natives.clone(),
        type_constructors: checked.type_constructors.clone(),
        meta_solutions: &checked.meta_solutions,
    };
    Some(program.run(term, initial_env(), fuel, world))
}

/// The context every program is elaborated in: the builtin types, then the
/// primitive operations.
pub fn initial_env() -> Env {
    let types = Builtin::ALL.iter().map(|_| Value::Erased);
    let prims = Prim::ALL.iter().map(|&prim| Value::Prim { prim, args: Vec::new() });
    types.chain(prims).fold(Env::default(), |env, v| env.push(Rc::new(v)))
}

impl Program<'_> {
    fn run(&self, term: Rc<Core>, env: Env, fuel: u64, world: &mut World) -> Result<Outcome, EvalError> {
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
                        return Ok(Outcome { value, answer, steps, output: String::new() });
                    }
                    Some(frame) => self.ret(value, frame, &mut kont)?,
                },
                Control::Perform(op, arg) => self.perform(op, arg, &mut kont, world)?,
                Control::Resume(frames, value) => {
                    kont.extend(frames.iter().cloned());
                    Control::Return(value)
                }
            };
        }
    }

    /// Take a term apart: return what it is already, or start on its first
    /// part and push a frame for the rest.
    fn eval(&self, term: &Rc<Core>, env: Env, kont: &mut Vec<Frame>) -> Result<Control, EvalError> {
        let value = |v: Value| Ok(Control::Return(Rc::new(v)));
        match &**term {
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
            | Core::Row(..) => value(Value::Erased),
            Core::Handle(body, ret, ops) => {
                kont.push(Frame::Handle { ret: ret.clone(), ops: ops.clone(), env: env.clone() });
                Ok(Control::Eval(body.clone(), env))
            }
            Core::Lit(lit) => value(match lit {
                Lit::Int(n) => Value::Int(*n),
                Lit::Word(w, b) => Value::Num(Number::Word(*w, *b)),
                Lit::Float(x) => Value::Num(Number::Float(f64::from_bits(*x))),
                Lit::Float32(x) => Value::Num(Number::Float32(f32::from_bits(*x))),
                Lit::Str(s) => Value::Str(s.as_str().into()),
                Lit::Char(c) => Value::Char(*c),
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
                if let Some(info) = self.ops.get(unique) {
                    return value(Value::Op { op: *unique, info: info.clone(), seen: 0, args: Vec::new() });
                }
                if let Some(info) = self.natives.get(unique) {
                    return value(Value::Native { info: *info, seen: 0, args: Vec::new() });
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
            // a match its type decides: the one alternative, all erased
            Core::Runtime(Note::Forced, inner) => match &**inner {
                Core::Case(_, alts) => match alts.first() {
                    Some(alt) => {
                        let env = alt.vars.iter().fold(env, |env, _| env.push(Rc::new(Value::Erased)));
                        Ok(Control::Eval(alt.body.clone(), env))
                    }
                    None => stuck("a case that cannot happen happened"),
                },
                _ => Ok(Control::Eval(inner.clone(), env)),
            },
            Core::Runtime(Note::Binds { .. }, inner) => Ok(Control::Eval(inner.clone(), env)),
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
    /// An operation performed: the frames down to its handler become the
    /// resumption, and the handler's clause runs where the handler's value
    /// goes.
    ///
    /// With no handler in the program, it is one of the world's (the
    /// prelude's `Console`), which the runtime handles: it writes to
    /// `output`, and gives back `()`.
    fn perform(&self, op: usize, args: Vec<Rc<Value>>, kont: &mut Vec<Frame>, world: &mut World) -> Result<Control, EvalError> {
        let handles = |f: &Frame| matches!(f, Frame::Handle { ops, .. } if ops.iter().any(|c| c.op == op));
        let Some(at) = kont.iter().rposition(handles) else {
            let name = self.ops.get(&op).map_or("", |info| info.name.as_str());
            let args: Vec<Arg> = args
                .iter()
                .map(|a| match &**a {
                    Value::Int(n) => Arg::Int(*n),
                    Value::Str(s) => Arg::Str(s.clone()),
                    // `()`, and whatever else carries nothing an operation reads
                    _ => Arg::Int(0),
                })
                .collect();
            return match world.perform(name, &args) {
                None => stuck("an operation with no handler for it"),
                Some(Err(Stop::Exit(code))) => Err(EvalError::Exited(code)),
                Some(Err(Stop::Failed(why))) => Err(EvalError::TestFailed(why)),
                Some(Ok(answer)) => Ok(Control::Return(Rc::new(answer_value(answer)))),
            };
        };
        let frames = kont.split_off(at);
        let Frame::Handle { ops, env, .. } = &frames[0] else { unreachable!("found above") };
        let clause = ops.iter().find(|c| c.op == op).expect("found above");
        let mut env = env.clone();
        let mut body = clause.body.clone();
        for _ in 0..clause.erased {
            body = under_lambda(&body)?;
            env = env.push(Rc::new(Value::Erased));
        }
        for arg in args {
            body = under_lambda(&body)?;
            env = env.push(arg);
        }
        body = under_lambda(&body)?;
        env = env.push(Rc::new(Value::Resumption(Rc::new(frames))));
        Ok(Control::Eval(body, env))
    }

    fn ret(&self, v: Rc<Value>, frame: Frame, kont: &mut Vec<Frame>) -> Result<Control, EvalError> {
        match frame {
            // the handled computation's value, to the `return` clause
            Frame::Handle { ret, env, .. } => Ok(Control::Eval(under_lambda(&ret)?, env.push(v))),
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
                    // the alternative binds the constructor's erased
                    // arguments first, then its fields; the last field is
                    // the innermost binder
                    let erased = alt.vars.len().saturating_sub(fields.len());
                    let env = (0..erased).fold(env, |env, _| env.push(Rc::new(Value::Erased)));
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
        Value::Native { info, seen, .. } => *seen >= info.params,
        _ => true,
    }
}

/// A constructor, given `seen` arguments of which these are the `fields`:
/// the value itself once it has them all.
/// A value, for one of the world's answers.
fn answer_value(answer: Answer) -> Value {
    match answer {
        Answer::Unit => Value::Unit,
        Answer::Int(n) => Value::Int(n),
        Answer::Bool(b) => Value::Bool(b),
        Answer::Str(s) => Value::Str(s),
        Answer::Con(name, tag, fields) => {
            Value::Con { name: name.into(), tag, fields: fields.into_iter().map(|f| Rc::new(answer_value(f))).collect() }
        }
        Answer::Array(items) => Value::Array(items.into_iter().map(|x| Rc::new(answer_value(x))).collect()),
    }
}

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
            if let (Prim::StrAppend, Value::Str(a), Value::Str(b)) = (prim, &*args[0], &*args[1]) {
                return value(Value::Str(format!("{a}{b}").into()));
            }
            let (Value::Int(a), Value::Int(b)) = (&*args[0], &*args[1]) else {
                return stuck(format!("{} is applied to something that is not an integer", prim.name()));
            };
            value(match prim.on_ints(*a, *b) {
                Some(IntResult::Int(n)) => Value::Int(n),
                Some(IntResult::Bool(t)) => Value::Bool(t),
                None => return Err(EvalError::Stopped("division by zero".into())),
            })
        }
        Value::Ctor { info, seen, fields } => {
            let mut fields = fields.clone();
            if *seen >= info.params {
                fields.push(arg);
            }
            value(saturate(info.clone(), seen + 1, fields))
        }
        Value::Op { op, info, seen, args } if *seen < info.erased => {
            value(Value::Op { op: *op, info: info.clone(), seen: seen + 1, args: args.clone() })
        }
        Value::Op { op, info, seen, args } => {
            let mut args = args.clone();
            args.push(arg);
            if args.len() < info.arity {
                return value(Value::Op { op: *op, info: info.clone(), seen: seen + 1, args });
            }
            Ok(Control::Perform(*op, args))
        }
        Value::Native { info, seen, args } if *seen < info.params => {
            value(Value::Native { info: *info, seen: seen + 1, args: args.clone() })
        }
        Value::Native { info, seen, args } => {
            let mut args = args.clone();
            args.push(arg);
            if args.len() < info.native.arity() {
                return value(Value::Native { info: *info, seen: seen + 1, args });
            }
            native(info.native, &args).map(|v| Control::Return(Rc::new(v)))
        }
        Value::Resumption(frames) => Ok(Control::Resume(frames.clone(), arg)),
        Value::Erased => value(Value::Erased),
        _ => stuck("applying something that is not a function"),
    }
}

/// A native, given what it takes.
fn native(n: Native, args: &[Rc<Value>]) -> Result<Value, EvalError> {
    let int = |v: &Rc<Value>| match **v {
        Value::Int(i) => Ok(i),
        _ => stuck(format!("{} is given something that is not an integer", n.name())),
    };
    let array = |v: &Rc<Value>| match &**v {
        Value::Array(items) => Ok(items.clone()),
        _ => stuck(format!("{} is given something that is not an array", n.name())),
    };
    let at = |items: &[Rc<Value>], i: i64| {
        usize::try_from(i).ok().filter(|i| *i < items.len()).ok_or_else(|| {
            EvalError::Stopped(format!("index {i} is out of bounds for an array of length {}", items.len()))
        })
    };
    let text = |v: &Rc<Value>| match &**v {
        Value::Str(s) => Ok(s.clone()),
        _ => stuck(format!("{} is given something that is not a string", n.name())),
    };
    // `Pair [a] b`: how a native gives back two things, the first boxed
    let pair = |a: Rc<Value>, b: Rc<Value>| Value::Con { name: "Pair".into(), tag: 0, fields: vec![Rc::new(Value::Box(a)), b] };
    Ok(match n {
        Native::Num(op, ty) => {
            let numbers = args
                .iter()
                .map(|a| match &**a {
                    Value::Int(n) => Ok(Number::Int(*n)),
                    Value::Num(n) => Ok(*n),
                    _ => stuck(format!("{} is given something that is not a number", n.name())),
                })
                .collect::<Result<Vec<_>, _>>()?;
            match num::apply(op, ty, &numbers).map_err(EvalError::Stopped)? {
                num::Answer::Num(Number::Int(n)) => Value::Int(n),
                num::Answer::Num(n) => Value::Num(n),
                num::Answer::Bool(b) => Value::Bool(b),
                num::Answer::Text(s) => Value::Str(s.into()),
            }
        }
        Native::Array | Native::MArray => Value::Erased,
        Native::ArrayLen => Value::Int(array(&args[0])?.len() as i64),
        Native::ArrayGet => {
            let items = array(&args[0])?;
            items[at(&items, int(&args[1])?)?].as_ref().clone()
        }
        Native::ArrayGetOr => {
            let items = array(&args[1])?;
            match at(&items, int(&args[2])?) {
                Ok(i) => items[i].as_ref().clone(),
                Err(_) => args[0].as_ref().clone(),
            }
        }
        Native::ArraySet | Native::MArraySet => {
            let mut items = array(&args[0])?;
            let i = at(&items, int(&args[1])?)?;
            items[i] = args[2].clone();
            Value::Array(items)
        }
        Native::ArrayPush => {
            let mut items = array(&args[0])?;
            items.push(args[1].clone());
            Value::Array(items)
        }
        Native::ArrayPop => {
            let mut items = array(&args[0])?;
            if items.pop().is_none() {
                return Err(EvalError::Stopped("popping an empty array".into()));
            }
            Value::Array(items)
        }
        Native::ArraySlice => {
            let items = array(&args[0])?;
            let len = items.len() as i64;
            let from = int(&args[1])?.clamp(0, len) as usize;
            let to = (int(&args[2])?.clamp(0, len) as usize).max(from);
            Value::Array(items[from..to].to_vec())
        }
        Native::ArrayConcat => {
            let mut items = array(&args[0])?;
            items.extend(array(&args[1])?);
            Value::Array(items)
        }
        Native::ArrayFromList => {
            let mut items = Vec::new();
            let mut list = args[0].clone();
            loop {
                let next = match &*list {
                    Value::Con { tag: 0, .. } => break,
                    Value::Con { tag: 1, fields, .. } if fields.len() == 2 => {
                        items.push(fields[0].clone());
                        fields[1].clone()
                    }
                    _ => return stuck("arrayFromList is given something that is not a list"),
                };
                list = next;
            }
            Value::Array(items)
        }
        Native::ArrayToList => array(&args[0])?.into_iter().rev().fold(
            Value::Con { name: "Nil".into(), tag: 0, fields: Vec::new() },
            |tail, head| Value::Con { name: "::".into(), tag: 1, fields: vec![head, Rc::new(tail)] },
        ),
        Native::MArrayNew => {
            let count = int(&args[0])?.max(0) as usize;
            Value::Array(vec![args[1].clone(); count])
        }
        Native::MArrayLen => pair(Rc::new(Value::Int(array(&args[0])?.len() as i64)), args[0].clone()),
        Native::MArrayGet => {
            let items = array(&args[0])?;
            pair(items[at(&items, int(&args[1])?)?].clone(), args[0].clone())
        }
        Native::MArrayFreeze | Native::MArrayThaw => args[0].as_ref().clone(),
        Native::StringByteLength => Value::Int(text(&args[0])?.len() as i64),
        Native::StringByteAt => {
            let (s, i) = (text(&args[0])?, int(&args[1])?);
            Value::Int(strings::byte_at(&s, i).ok_or_else(|| {
                EvalError::Stopped(format!("index {i} is out of bounds for a string of {} bytes", s.len()))
            })?)
        }
        Native::StringSlice => Value::Str(strings::slice(&text(&args[0])?, int(&args[1])?, int(&args[2])?).into()),
        Native::StringCompare => Value::Int(strings::compare(&text(&args[0])?, &text(&args[1])?)),
        Native::StringIndexOf => Value::Int(strings::index_of(&text(&args[0])?, &text(&args[1])?, int(&args[2])?)),
        Native::StringToChars => Value::Array(text(&args[0])?.chars().map(|c| Rc::new(Value::Char(c))).collect()),
        Native::CharsToString => {
            let chars = array(&args[0])?
                .iter()
                .map(|v| match **v {
                    Value::Char(c) => Ok(c),
                    _ => stuck("charsToString is given something that is not a character"),
                })
                .collect::<Result<String, _>>()?;
            Value::Str(chars.into())
        }
        Native::CharCode => match *args[0] {
            Value::Char(c) => Value::Int(i64::from(u32::from(c))),
            _ => return stuck("charCode is given something that is not a character"),
        },
        Native::CharFromCode => {
            let code = int(&args[0])?;
            Value::Char(
                strings::from_code(code)
                    .ok_or_else(|| EvalError::Stopped(format!("{code} is not a Unicode scalar value")))?,
            )
        }
        Native::StringHash => Value::Int(idyllic_builtins::hash::string(&text(&args[0])?)),
        Native::CharHash => match *args[0] {
            Value::Char(c) => Value::Int(idyllic_builtins::hash::char(c)),
            _ => return stuck("charHash is given something that is not a character"),
        },
        Native::BoolHash => match *args[0] {
            Value::Bool(b) => Value::Int(idyllic_builtins::hash::bool(b)),
            _ => return stuck("boolHash is given something that is not a truth value"),
        },
    })
}

/// What is under a lambda, looking through the note on it.
fn under_lambda(c: &Rc<Core>) -> Result<Rc<Core>, EvalError> {
    match &**c {
        Core::Lam(_, body) => Ok(body.clone()),
        Core::Runtime(_, inner) => under_lambda(inner),
        _ => stuck("a handler clause that is not a function"),
    }
}

// ---------------------------------------------------------------------------
// Showing the answer
// ---------------------------------------------------------------------------

/// A value as the REPL shows it, if it is plain data: a function or a type
/// has nothing worth showing, and neither has anything that contains one.
pub fn show(v: &Value) -> Option<String> {
    // a field that is itself an application is parenthesised
    let atom = |s: String| if s.contains(' ') && !s.starts_with(['[', '"', '#', '(']) { format!("({s})") } else { s };
    match v {
        Value::Int(n) => Some(n.to_string()),
        Value::Num(n) => Some(n.to_string()),
        Value::Str(s) => Some(format!("{:?}", &**s)),
        Value::Char(c) => Some(idyllic_ast::show_char(*c)),
        Value::Bool(true) => Some("True".into()),
        Value::Bool(false) => Some("False".into()),
        Value::Unit => Some("()".into()),
        Value::Box(inner) => Some(format!("[{}]", show(inner)?)),
        // a pair is a tuple, `(1, True)`, and one nested to the right a
        // longer one, `(1, 2, 3)`
        Value::Con { name, fields, .. } if &**name == "Pair" && fields.len() == 2 => {
            let mut items = vec![show(&fields[0])?];
            let mut rest = fields[1].clone();
            loop {
                let next = match &*rest {
                    Value::Con { name, fields, .. } if &**name == "Pair" && fields.len() == 2 => {
                        items.push(show(&fields[0])?);
                        fields[1].clone()
                    }
                    _ => break,
                };
                rest = next;
            }
            items.push(show(&rest)?);
            Some(format!("({})", items.join(", ")))
        }
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
        // `#[1; 2]`, as one is written
        Value::Array(items) => {
            let items = items.iter().map(|x| show(x)).collect::<Option<Vec<String>>>()?;
            Some(match items.as_slice() {
                [] => "#[;]".into(),
                [one] => format!("#[{one};]"),
                _ => format!("#[{}]", items.join("; ")),
            })
        }
        Value::Closure { .. }
        | Value::Prim { .. }
        | Value::Ctor { .. }
        | Value::Op { .. }
        | Value::Native { .. }
        | Value::Resumption(_)
        | Value::Erased => None,
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
            let head = Rc::new(Value::Int(n));
            list = Rc::new(Value::Con { name: "::".into(), tag: 1, fields: vec![head, list] });
            env = env.push(Rc::new(Value::Box(Rc::new(Value::Unit))));
        }
        let closure = Value::Closure { env, body: Rc::new(Core::Type(0)) };
        drop(list);
        drop(closure);
    }

    #[test]
    fn a_shared_tail_survives_its_first_owner() {
        let tail = Rc::new(Value::Con { name: "Nil".into(), tag: 0, fields: Vec::new() });
        let one = Rc::new(Value::Int(1));
        let list = Value::Con { name: "::".into(), tag: 1, fields: vec![one, tail.clone()] };
        drop(list);
        assert_eq!(show(&tail).as_deref(), Some("Nil"));
    }
}
