//! The abstract machine for AxCut (the paper's Figure 5), with the paper's
//! memory management (its §4.5) underneath.
//!
//! A state is a statement under execution and an environment of values:
//! Control and Environment.  The K of a CEK machine — the continuation — is
//! not a third register here, and that is the point of the calculus: a
//! continuation is a *consumer*, a value like a closure, bound in the
//! environment and passed to whoever should return to it.  `invoke k ret` is
//! a return, `invoke f apply` is a call, and neither pushes or pops anything.
//! So the C, E and K are all here, with K living in E; there is no stack to
//! grow, and a loop written as a tail call runs in constant space.
//!
//! ```text
//! (let)     ⟨ let v = m(Γ0); s  ∥ E, E0 ⟩          →  ⟨ s ∥ E, v ↦ {m; E0} ⟩
//! (new)     ⟨ new v = (Γ0)b; s  ∥ E, E0 ⟩          →  ⟨ s ∥ E, v ↦ {E0; b} ⟩
//! (switch)  ⟨ switch v b        ∥ E, v ↦ {m; E0} ⟩ →  ⟨ b(m) ∥ E, E0 ⟩
//! (invoke)  ⟨ invoke v m        ∥ E, v ↦ {E0; b} ⟩ →  ⟨ b(m) ∥ E, E0 ⟩
//! (jump)    ⟨ jump l            ∥ E ⟩              →  ⟨ P(l) ∥ E ⟩
//! (subst)   ⟨ substitute [v' ↦ v, ..]; s ∥ E ⟩     →  ⟨ s ∥ v' ↦ E(v), .. ⟩
//! ```
//!
//! The environment is an ordered list, and each rule takes what it needs
//! from the end.  On real hardware that is all there is to it: positions are
//! registers and nothing has a name.  Here the names are kept, and every rule
//! checks that the entries it takes are the ones the statement names.  AxCut's
//! type system would rule a mismatch out; having no type checker for it, the
//! machine reports one as [`MachineError::Malformed`].  It is how the tests
//! know that [`super::linearize`] got the substitutions right.
//!
//! # Memory
//!
//! There is no garbage collector, and none is needed.  `{m; E0}` and
//! `{E0; b}` are *blocks* on a heap, and the four rules above that mention
//! one are the only places memory is touched:
//!
//! * `let` and `new` **acquire** a block and move `E0` into it;
//! * `switch` and `invoke` **release** one and move `E0` back out.
//!
//! In a linear program that is the whole story: every block has one owner,
//! is released exactly once, and goes straight back on the free list for the
//! next `let` to reuse.  A continuation is such a block — allocated at a
//! call, released by the return — so linear code gets stack-like allocation
//! without a stack.
//!
//! Sharing is what `substitute` is for, and it is the *only* place a
//! reference count changes.  A block counts its references beyond the first:
//!
//! * a variable the substitution uses `n > 1` times is **shared**: its
//!   block's count goes up by `n - 1`;
//! * a variable it drops is **erased**: the count goes down, or, if it was
//!   already `0`, the block is dead.
//!
//! Then releasing a block has two cases.  If its count is `0` it is unique:
//! its fields *move* out and it is freed.  Otherwise someone else still
//! holds it: the count goes down and the fields are copied out, each shared
//! once more.  That check is dynamic because uniqueness is a property of the
//! block, not of the variable: a linear variable may well point at a block
//! its caller shared (see [`super::grades`]).
//!
//! Erasing is **lazy**.  A dead block goes on a *todo* list with its fields
//! still in it, and they are only erased in turn when the block is reused.
//! So dropping a whole structure costs the same as dropping one block, and
//! every operation here takes constant time however much is live or dead.
//!
//! Counting suffices — there is no cycle for it to miss — because a block's
//! fields exist before it does and are never assigned afterwards.
//! [`Memory`] reports what happened, and every run ends with an audit: each
//! live block must be reachable from the answer, and hold exactly the count
//! its references imply.
//!
//! Integers and strings are external values, not blocks; and a producer or
//! consumer with nothing in it (`true()`, a closed function) needs no block
//! at all.
//!
//! Idyll has recursion and, so far, nothing to stop one with, so the machine
//! runs on fuel.

use super::{Branch, Clauses, Extern, Program, Stmt, Symbol, Var};
use num_bigint::BigInt;
use std::collections::HashMap;
use std::fmt;

type Addr = usize;

/// What a variable holds: on hardware, two registers — a block (or none) and
/// a tag or a table of clauses.
#[derive(Clone, Debug)]
enum Value<'p> {
    /// `{ m; E }`: a symbol, and the block its fields are in.
    Producer(Symbol, Option<Addr>),
    /// `{ E; b }`: the block holding a closure environment, the names the
    /// clauses know it by, and the clauses.
    Consumer(Option<Addr>, &'p [Var], &'p Clauses),
    Int(BigInt),
    Str(String),
}

impl Value<'_> {
    fn block(&self) -> Option<Addr> {
        match self {
            Value::Producer(_, block) | Value::Consumer(block, ..) => *block,
            Value::Int(_) | Value::Str(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockState {
    InUse,
    /// on the free list: nothing in it
    Free,
    /// dead, but its fields have not been erased yet
    Todo,
}

#[derive(Debug)]
struct Block<'p> {
    /// references to this block beyond the first
    count: usize,
    fields: Vec<Value<'p>>,
    state: BlockState,
}

/// What a run did with memory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Memory {
    /// blocks taken from fresh memory
    pub fresh: usize,
    /// blocks reused straight from the free list
    pub reused: usize,
    /// dead blocks reused from the todo list, their fields erased at last
    pub reused_lazily: usize,
    /// the most blocks in use at once
    pub peak: usize,
    /// reference counts incremented by a `substitute` (contraction)
    pub shares: usize,
    /// variables dropped by a `substitute` (weakening)
    pub erases: usize,
    /// releases that found the block unique, and moved its fields out
    pub unique_releases: usize,
    /// releases that found it shared, and copied them
    pub shared_releases: usize,
    /// Audit, at the end: blocks still in use that the answer cannot reach.
    pub leaked: usize,
    /// Audit: blocks whose count is not the number of references to them.
    pub miscounted: usize,
}

impl Memory {
    pub fn acquired(&self) -> usize {
        self.fresh + self.reused + self.reused_lazily
    }

    /// No reference count was ever written: the run was linear.
    pub fn was_linear(&self) -> bool {
        self.shares == 0 && self.erases == 0 && self.shared_releases == 0
    }
}

impl fmt::Display for Memory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "memory: {} blocks acquired ({} fresh, {} reused), at most {} at once; {} shares, {} erases, {} of {} releases unique",
            self.acquired(),
            self.fresh,
            self.reused + self.reused_lazily,
            self.peak,
            self.shares,
            self.erases,
            self.unique_releases,
            self.unique_releases + self.shared_releases,
        )
    }
}

#[derive(Default)]
struct Heap<'p> {
    blocks: Vec<Block<'p>>,
    free: Vec<Addr>,
    todo: Vec<Addr>,
    in_use: usize,
    memory: Memory,
}

impl<'p> Heap<'p> {
    /// Contraction: `n` more references to whatever `v` points at.
    fn share(&mut self, v: &Value<'p>, n: usize) {
        if let Some(a) = v.block() {
            self.blocks[a].count += n;
            self.memory.shares += n;
        }
    }

    /// Weakening: one reference fewer.  The last one leaves the block dead,
    /// but what it holds is only dealt with when the block is wanted again.
    fn erase(&mut self, v: &Value<'p>) {
        let Some(a) = v.block() else { return };
        let block = &mut self.blocks[a];
        if block.count > 0 {
            block.count -= 1;
        } else {
            block.state = BlockState::Todo;
            self.todo.push(a);
            self.in_use -= 1;
        }
    }

    /// A block holding `fields`: from the free list if it has one, else a
    /// dead block (whose own fields are erased now), else fresh memory.
    /// Nothing is acquired for no fields.
    fn acquire(&mut self, fields: Vec<Value<'p>>) -> Option<Addr> {
        if fields.is_empty() {
            return None;
        }
        let a = if let Some(a) = self.free.pop() {
            self.memory.reused += 1;
            a
        } else if let Some(a) = self.todo.pop() {
            self.memory.reused_lazily += 1;
            for old in std::mem::take(&mut self.blocks[a].fields) {
                self.erase(&old);
            }
            a
        } else {
            self.memory.fresh += 1;
            self.blocks.push(Block { count: 0, fields: Vec::new(), state: BlockState::Free });
            self.blocks.len() - 1
        };
        self.blocks[a] = Block { count: 0, fields, state: BlockState::InUse };
        self.in_use += 1;
        self.memory.peak = self.memory.peak.max(self.in_use);
        Some(a)
    }

    /// Take the fields back out.  A unique block gives them up and is free;
    /// a shared one keeps them, and they become shared too.
    fn release(&mut self, block: Option<Addr>) -> Result<Vec<Value<'p>>, MachineError> {
        let Some(a) = block else { return Ok(Vec::new()) };
        if self.blocks[a].state != BlockState::InUse {
            return malformed(format!("block {a} is used after it was freed"));
        }
        if self.blocks[a].count == 0 {
            self.memory.unique_releases += 1;
            self.blocks[a].state = BlockState::Free;
            self.free.push(a);
            self.in_use -= 1;
            Ok(std::mem::take(&mut self.blocks[a].fields))
        } else {
            self.memory.shared_releases += 1;
            self.blocks[a].count -= 1;
            let fields = self.blocks[a].fields.clone();
            for f in &fields {
                self.share(f, 1);
            }
            Ok(fields)
        }
    }

    fn fields(&self, block: Option<Addr>) -> &[Value<'p>] {
        block.map_or(&[], |a| &self.blocks[a].fields)
    }

    /// As the REPL shows a value, if it is plain data: a function or a type
    /// has nothing worth showing.
    fn show(&self, v: &Value<'p>) -> Option<String> {
        match v {
            Value::Int(n) => Some(n.to_string()),
            Value::Str(s) => Some(format!("{s:?}")),
            Value::Consumer(..) => None,
            Value::Producer(m, block) => match (m, self.fields(*block)) {
                (Symbol::True, []) => Some("True".into()),
                (Symbol::False, []) => Some("False".into()),
                (Symbol::Unit, []) => Some("()".into()),
                (Symbol::Box, [inner]) => Some(format!("[{}]", self.show(inner)?)),
                // `x :: xs` is written the way it is read; its head is
                // parenthesised if it is itself compound
                (Symbol::Con(name, _), [head, tail]) if &**name == "::" => {
                    let head = self.show(head)?;
                    let head = if head.contains(' ') && !head.starts_with(['[', '"']) { format!("({head})") } else { head };
                    Some(format!("{head} :: {}", self.show(tail)?))
                }
                (Symbol::Con(name, _), fields) => {
                    let mut out = name.to_string();
                    for f in fields {
                        let f = self.show(f)?;
                        let compound = f.contains(' ') && !f.starts_with(['[', '"']);
                        out.push_str(&if compound { format!(" ({f})") } else { format!(" {f}") });
                    }
                    Some(out)
                }
                _ => None,
            },
        }
    }

    /// The end-of-run audit.  Everything but the answer is dropped and the
    /// todo list worked off, which leaves exactly what is truly live; that
    /// must be what the answer can reach, each block counted right.
    fn audit(&mut self, answer: &Value<'p>, others: Vec<Value<'p>>) {
        for v in &others {
            self.erase(v);
        }
        while let Some(a) = self.todo.pop() {
            for old in std::mem::take(&mut self.blocks[a].fields) {
                self.erase(&old);
            }
            self.blocks[a].state = BlockState::Free;
            self.free.push(a);
        }
        // references to each block: from the answer, and from live fields
        let mut references: HashMap<Addr, usize> = HashMap::new();
        let mut pending: Vec<Addr> = Vec::new();
        let refer = |a: Addr, references: &mut HashMap<Addr, usize>, pending: &mut Vec<Addr>| {
            let seen = references.entry(a).or_insert(0);
            *seen += 1;
            if *seen == 1 {
                pending.push(a);
            }
        };
        if let Some(a) = answer.block() {
            refer(a, &mut references, &mut pending);
        }
        while let Some(a) = pending.pop() {
            for f in &self.blocks[a].fields {
                if let Some(b) = f.block() {
                    refer(b, &mut references, &mut pending);
                }
            }
        }
        for (a, block) in self.blocks.iter().enumerate().filter(|(_, b)| b.state == BlockState::InUse) {
            match references.get(&a) {
                None => self.memory.leaked += 1,
                Some(&n) if n != block.count + 1 => self.memory.miscounted += 1,
                Some(_) => {}
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MachineError {
    OutOfFuel,
    /// The program broke AxCut's rules; a well-typed one cannot.
    Malformed(String),
}

impl fmt::Display for MachineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MachineError::OutOfFuel => write!(f, "out of fuel"),
            MachineError::Malformed(why) => write!(f, "malformed program: {why}"),
        }
    }
}

/// How a run ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// The answer as the REPL shows it, if it is plain data: a function or a
    /// type has nothing worth showing.
    pub answer: Option<String>,
    pub memory: Memory,
}

type Env<'p> = Vec<(&'p Var, Value<'p>)>;

fn malformed<T>(why: impl Into<String>) -> Result<T, MachineError> {
    Err(MachineError::Malformed(why.into()))
}

/// Take `names` off the end of the environment, where they must be, in
/// order.
fn take_tail<'p>(env: &mut Env<'p>, names: &[Var], by: &str) -> Result<Vec<Value<'p>>, MachineError> {
    let Some(at) = env.len().checked_sub(names.len()) else {
        return malformed(format!("{by} needs ({}), but the environment is shorter", names.join(", ")));
    };
    let tail = env.split_off(at);
    if tail.iter().map(|(v, _)| *v).ne(names.iter()) {
        let found: Vec<&str> = tail.iter().map(|(v, _)| v.as_str()).collect();
        return malformed(format!("{by} needs ({}) at the end, not ({})", names.join(", "), found.join(", ")));
    }
    Ok(tail.into_iter().map(|(_, value)| value).collect())
}

fn lookup<'e, 'p>(env: &'e Env<'p>, v: &Var) -> Result<&'e Value<'p>, MachineError> {
    match env.iter().rev().find(|(name, _)| *name == v) {
        Some((_, value)) => Ok(value),
        None => malformed(format!("{v} is not in the environment")),
    }
}

fn clause<'c>(clauses: &'c Clauses, m: &Symbol) -> Result<&'c Branch, MachineError> {
    match clauses.iter().find(|(symbol, _)| symbol == m) {
        Some((_, branch)) => Ok(branch),
        None => malformed(format!("no clause for {m}")),
    }
}

/// Bind `names` to `values`, position by position.
fn bind<'p>(env: &mut Env<'p>, names: &'p [Var], values: Vec<Value<'p>>, what: &str) -> Result<(), MachineError> {
    if names.len() != values.len() {
        return malformed(format!("{what} binds {} variables, but is given {}", names.len(), values.len()));
    }
    env.extend(names.iter().zip(values));
    Ok(())
}

fn integer(env: &Env<'_>, v: &Var) -> Result<BigInt, MachineError> {
    match lookup(env, v)? {
        Value::Int(n) => Ok(n.clone()),
        _ => malformed(format!("{v} is not an integer")),
    }
}

/// A comparison's answer: a producer with no fields, and so no block.
fn truth<'p>(b: bool) -> Value<'p> {
    Value::Producer(if b { Symbol::True } else { Symbol::False }, None)
}

/// Run a program from its first definition until it halts, for at most
/// `fuel` steps.
pub fn run(program: &Program, mut fuel: u64) -> Result<Outcome, MachineError> {
    let Some(main) = program.definitions.first() else { return malformed("there is nothing to run") };
    if !main.params.is_empty() {
        return malformed("the entry point takes parameters");
    }
    let mut control: &Stmt = &main.body;
    let mut env: Env = Vec::new();
    let mut heap = Heap::default();
    loop {
        fuel = fuel.checked_sub(1).ok_or(MachineError::OutOfFuel)?;
        control = match control {
            // the one place reference counts change
            Stmt::Substitute(pairs, rest) => {
                for (name, value) in &env {
                    match pairs.iter().filter(|(_, old)| old == *name).count() {
                        0 => {
                            // (an integer has no block, and nothing to count)
                            heap.memory.erases += usize::from(value.block().is_some());
                            heap.erase(value);
                        }
                        n => heap.share(value, n - 1),
                    }
                }
                env = pairs.iter().map(|(new, old)| Ok((new, lookup(&env, old)?.clone()))).collect::<Result<_, _>>()?;
                rest
            }
            Stmt::Let(v, m, fields, rest) => {
                let fields = take_tail(&mut env, fields, "let")?;
                env.push((v, Value::Producer(m.clone(), heap.acquire(fields))));
                rest
            }
            Stmt::New(v, captured, clauses, rest) => {
                let closure = take_tail(&mut env, captured, "new")?;
                env.push((v, Value::Consumer(heap.acquire(closure), captured, clauses)));
                rest
            }
            Stmt::Switch(v, clauses) => {
                let scrutinee = take_tail(&mut env, std::slice::from_ref(v), "switch")?.remove(0);
                let Value::Producer(m, block) = scrutinee else { return malformed(format!("{v} is not a producer")) };
                let branch = clause(clauses, &m)?;
                bind(&mut env, &branch.params, heap.release(block)?, "a switch clause")?;
                &branch.body
            }
            Stmt::Invoke(v, m, args) => {
                let receiver = take_tail(&mut env, std::slice::from_ref(v), "invoke")?.remove(0);
                let Value::Consumer(block, names, clauses) = receiver else {
                    return malformed(format!("{v} is not a consumer"));
                };
                let branch = clause(clauses, m)?;
                // what is left must be the arguments, exactly
                let args = take_tail(&mut env, args, "invoke")?;
                if !env.is_empty() {
                    return malformed(format!("invoke {v} {m} leaves {} variables behind", env.len()));
                }
                bind(&mut env, &branch.params, args, "an invoked clause")?;
                bind(&mut env, names, heap.release(block)?, "a closure environment")?;
                &branch.body
            }
            Stmt::Jump(l, args) => {
                let Some(target) = program.definition(l) else { return malformed(format!("no definition of {l}")) };
                let args = take_tail(&mut env, args, "jump")?;
                if !env.is_empty() {
                    return malformed(format!("jump {l} leaves {} variables behind", env.len()));
                }
                bind(&mut env, &target.params, args, "a label")?;
                &target.body
            }
            Stmt::Extern(op, args, branches) => {
                let result = match (op, args.as_slice()) {
                    (Extern::LitInt(n), []) => Value::Int(n.clone()),
                    (Extern::LitStr(s), []) => Value::Str(s.clone()),
                    (Extern::Add, [a, b]) => Value::Int(integer(&env, a)? + integer(&env, b)?),
                    (Extern::Mul, [a, b]) => Value::Int(integer(&env, a)? * integer(&env, b)?),
                    (Extern::Sub, [a, b]) => Value::Int(integer(&env, a)? - integer(&env, b)?),
                    (Extern::Lt, [a, b]) => truth(integer(&env, a)? < integer(&env, b)?),
                    (Extern::Eq, [a, b]) => truth(integer(&env, a)? == integer(&env, b)?),
                    (Extern::Halt, [answer]) => {
                        let at = env.iter().rposition(|(name, _)| *name == answer);
                        let Some(at) = at else { return malformed(format!("{answer} is not in the environment")) };
                        let (_, answer) = env.remove(at);
                        let shown = heap.show(&answer);
                        heap.audit(&answer, env.into_iter().map(|(_, v)| v).collect());
                        return Ok(Outcome { answer: shown, memory: heap.memory });
                    }
                    _ => return malformed(format!("extern {op} is given {} arguments", args.len())),
                };
                let [branch] = branches.as_slice() else { return malformed(format!("extern {op} continues one way")) };
                bind(&mut env, &branch.params, vec![result], "an extern clause")?;
                &branch.body
            }
        };
    }
}
