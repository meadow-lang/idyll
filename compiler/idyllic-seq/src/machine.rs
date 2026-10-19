//! The AxCut machine, as the paper has it: the operational semantics of its
//! §3.3 (Figure 5), carried out the way §4 and Appendix B carry it out on
//! hardware.
//!
//! ```text
//! (let)     ⟨ let v = m(Γ0); s  ∥ E, E0 ∥ P ⟩          →  ⟨ s ∥ E, v ↦ {m; E0} ∥ P ⟩
//! (new)     ⟨ new v = (Γ0)b; s  ∥ E, E0 ∥ P ⟩          →  ⟨ s ∥ E, v ↦ {E0; b} ∥ P ⟩
//! (switch)  ⟨ switch v b        ∥ E, v ↦ {m; E0} ∥ P ⟩ →  ⟨ b(m) ∥ E, E0 ∥ P ⟩
//! (invoke)  ⟨ invoke v m        ∥ E, v ↦ {E0; b} ∥ P ⟩ →  ⟨ b(m) ∥ E, E0 ∥ P ⟩
//! (jump)    ⟨ jump l            ∥ E ∥ P ⟩              →  ⟨ P(l) ∥ E ∥ P ⟩
//! (subst)   ⟨ substitute [v' ↦ v, ..]; s ∥ E ∥ P ⟩     →  ⟨ s ∥ v' ↦ E(v), .. ∥ P ⟩
//! ```
//!
//! There is no stack and no continuation register: a continuation is a
//! consumer like any other, bound in the environment, and `invoke k ret` is
//! a return.  So a loop written as a tail call runs in constant space.
//!
//! # Two steps, as on hardware
//!
//! **Assembly** ([`assemble`]) is the paper's Figure 8.  AxCut's environment
//! is ordered, and at every point in a program which variable is where is
//! known statically, so a variable becomes a *position* — a register — and
//! its name is never needed again.  A label becomes a code address and
//! `jump` a direct jump.  `switch` gets a *jump table*, indexed by a
//! constructor's tag; `new` gets a *virtual table*, indexed by a destructor's
//! position in its signature.  Each clause starts with a `Load`, which moves
//! the fields (or the closure environment) out of the block of the value
//! just dispatched on, and releases the block.  And each `substitute` is
//! compiled to the reference counting it implies — `Share` for each variable
//! used more than once, `Erase` for each one dropped — and a parallel move.
//!
//! It is also where the environment is checked.  AxCut's typing rules demand
//! that `let` and `new` find their fields at the end of the environment,
//! that `switch` and `invoke` find their variable there, and that `invoke`
//! and `jump` leave exactly what the clause or label expects; there being no
//! type checker for AxCut, assembly checks those, and reports a program that
//! breaks them as [`MachineError::Malformed`].  It is how the tests know that
//! [`crate::linearize`] got the substitutions right.
//!
//! **Execution** ([`execute`]) runs the code on a register file and a heap.
//! A value is two words, as it is two registers in the paper: a block (or
//! none) and a word, which is a constructor's tag, a consumer's virtual
//! table, or an external value (an integer or a string, which have no
//! block).  What typing guarantees about the *values* — that a `switch` meets
//! a producer, an `invoke` a consumer with a clause for the destructor —
//! assembly cannot know, and execution checks as it goes.
//!
//! One liberty: a producer's word carries the constructor itself, not only
//! its tag.  On hardware only the tag is there; here the name comes along so
//! that the answer can be shown.
//!
//! # Memory
//!
//! The paper's §4.5, with its Figure 10 for `Acquire` and `Release`.  There
//! is no collector.  A block is a header and fields.  While the block is in
//! use the header is its *reference count* — the references beyond the
//! first, so `0` means unique — and while it is on a list the header is the
//! *link* to the next block, so the lists cost nothing but the blocks
//! themselves.  There are two lists, as there are two registers for them in
//! the paper: the free list (`heap`), of blocks ready to use, and the todo
//! list (`todo`), of dead blocks whose fields have not been erased yet.
//!
//! * `let` and `new` **acquire** a block and store into it: from the free
//!   list, or failing that from the todo list — erasing that block's old
//!   fields now — or failing that from fresh memory.  Nothing is acquired
//!   for no fields.
//! * `switch` and `invoke` **release** one, at the clause's `Load`.  If its
//!   count is `0` it is unique: its fields move into registers and the block
//!   goes on the free list.  Otherwise someone else still holds it: the
//!   count goes down, and the fields are copied, each shared once more.
//! * `substitute` **shares** a variable it uses `n > 1` times (the count
//!   goes up by `n - 1`) and **erases** one it drops (the count goes down;
//!   or, if it was `0`, the block goes on the todo list with its fields
//!   still in it).
//!
//! Nothing else touches memory.  In linear code no count ever changes, and a
//! continuation — acquired at a call, released by the return — is back on
//! the free list in time for the next call: stack-like allocation without a
//! stack.  Erasing is lazy, so dropping a whole structure costs one
//! decrement; every operation takes constant time however much is live or
//! dead.  Counting suffices, because a block's fields exist before it does
//! and are never assigned afterwards, so there are no cycles.
//!
//! The paper's blocks are all one size, with longer environments spread
//! over several; here a block is as large as what it holds, which changes
//! nothing but the bookkeeping.  (The native runtime, `rts/idyll_rt.c`,
//! keeps a free list per size instead.)
//!
//! [`Memory`] reports what happened, and every run ends with an audit: each
//! live block must be reachable from the answer, and hold exactly the count
//! its references imply.
//!
//! An array is a block too, its fields its elements, and its word says it
//! is one; the empty array has no block, as a constructor with no fields has
//! none.  A native consumes what it is given, so an array whose count is `0`
//! is the native's alone: setting an element, pushing one, or popping one
//! changes it in place.  A shared one is copied first, as a shared block is
//! when it is loaded.
//!
//! Idyll has recursion and, so far, nothing to stop one with, so the machine
//! runs on fuel: one unit per rule of Figure 5.

use super::{Extern, Program, Stmt, Symbol, Var};
use idyllic_builtins::{strings, IntResult, Native};
use idyllic_world::{Answer, Arg, Stop, World};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Code
// ---------------------------------------------------------------------------

/// A code address.
type Pc = usize;
/// A register: a position in the environment, counted from the start.
type Reg = usize;
/// A jump table or a virtual table, by its index in [`Code::tables`].
type TableId = usize;

/// One instruction.  Each but `Load` carries out one rule of Figure 5.
#[derive(Clone, Debug)]
enum Instr {
    /// (subst): the reference counts the substitution implies, then the
    /// parallel move.  `moves[i]` is the register the new register `i`
    /// comes from.
    Substitute { share: Vec<(Reg, usize)>, erase: Vec<Reg>, moves: Vec<Reg> },
    /// (let): store the last `fields` registers into a block, and in their
    /// place put the producer — the block and the constructor.
    Let { fields: usize, symbol: Symbol },
    /// (new): store the last `fields` registers, the closure environment,
    /// and in their place put the consumer — the block and its table.
    New { fields: usize, table: TableId },
    /// (switch): jump through `table` by the tag of the last register.
    Switch { table: TableId },
    /// (invoke): jump through the virtual table of the last register, to
    /// the clause for `method`, which is handed the `args` registers
    /// before it.
    Invoke { symbol: Symbol, args: usize },
    /// (jump): straight to a label, whose parameters are the whole
    /// environment.
    Jump { target: Pc },
    /// (extern): compute from `args`, which stay where they are, and push
    /// the result.
    Extern { op: Extern, args: Vec<Reg> },
    /// A native: compute from the last `args` registers, which it consumes,
    /// and put the result in their place.
    Native { native: Native, args: usize },
    /// `extern halt(v) {}`: the program's answer is register `answer`.
    Halt { answer: Reg },
    /// At the entry of a clause: take the block of the last register, which
    /// the dispatch landed here by, and put its `fields` in its place.
    Load { fields: usize, what: &'static str },
    /// `handle`: the last two registers, a handler and where its `handle`
    /// goes, become the newest handler in force; in their place goes the
    /// consumer the body returns to.
    Handle,
    /// `perform op`: from the last two registers, an argument and a
    /// continuation, to the clause for `op` of the newest handler that has
    /// one.  The handlers from it on are taken out of force and kept in the
    /// resumption, with the continuation.
    Perform { op: Symbol, args: usize },
    /// The clause of the consumer a handled body returns to: the newest
    /// handler leaves force, and its `return` gets the value.
    HandlerReturn,
    /// The clause of a resumption, invoked as a function: the handlers it
    /// kept come back into force, the first answering now to the caller,
    /// and the continuation it kept gets the value.
    Resume,
}

/// An entry of a jump table or a virtual table: a clause, and what it is
/// for (checked by `invoke`; a debugger would have the same).
#[derive(Clone, Debug)]
struct Entry {
    symbol: Symbol,
    pc: Pc,
    /// how many parameters the clause binds before the block is loaded
    params: usize,
}

/// A program, assembled.
#[derive(Clone, Debug)]
pub struct Code {
    instrs: Vec<Instr>,
    /// the tables of the two consumers the machine makes itself: the one a
    /// handled body returns to, and a resumption
    return_table: TableId,
    resume_table: TableId,
    /// Indexed by a constructor's tag (a jump table) or by a destructor's
    /// position in its signature (a virtual table); `None` where the table
    /// has no clause.
    tables: Vec<Vec<Option<Entry>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MachineError {
    OutOfFuel,
    /// `/` or `%` by zero.
    DivisionByZero,
    /// An array indexed where it has no element.
    OutOfBounds { index: i64, len: usize },
    /// `arrayPop` of an array with nothing to pop.
    EmptyArray,
    /// A string's byte read where it has none.
    ByteOutOfBounds { index: i64, len: usize },
    /// A code point made a character that is none.
    NotACharacter(i64),
    /// The program ended itself: `exit n`.
    Exited(i32),
    /// `fail message`: a test failed.
    TestFailed(String),
    /// The program broke AxCut's rules; a well-typed one cannot.
    Malformed(String),
}

impl fmt::Display for MachineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MachineError::OutOfFuel => write!(f, "out of fuel"),
            MachineError::DivisionByZero => write!(f, "division by zero"),
            MachineError::OutOfBounds { index, len } => {
                write!(f, "index {index} is out of bounds for an array of length {len}")
            }
            MachineError::EmptyArray => write!(f, "popping an empty array"),
            MachineError::ByteOutOfBounds { index, len } => {
                write!(f, "index {index} is out of bounds for a string of {len} bytes")
            }
            MachineError::NotACharacter(code) => write!(f, "{code} is not a Unicode scalar value"),
            MachineError::Exited(code) => write!(f, "exited with {code}"),
            MachineError::TestFailed(why) => write!(f, "test failed: {why}"),
            MachineError::Malformed(why) => write!(f, "malformed program: {why}"),
        }
    }
}

fn malformed<T>(why: impl Into<String>) -> Result<T, MachineError> {
    Err(MachineError::Malformed(why.into()))
}

/// A destructor's position in its signature: its index in a virtual table.
/// Every consumer lowering makes has a signature of one destructor.
fn method(m: &Symbol) -> usize {
    match m {
        Symbol::Apply | Symbol::Ret => 0,
        other => other.tag(),
    }
}

// ---------------------------------------------------------------------------
// Assembly
// ---------------------------------------------------------------------------

struct Assembler<'p> {
    program: &'p Program,
    instrs: Vec<Instr>,
    tables: Vec<Vec<Option<Entry>>>,
    /// jumps to patch with their label's address, once it is known
    jumps: Vec<(Pc, &'p str)>,
}

/// Assemble a program: resolve every variable to its register, every label
/// to its address, and every `switch` and `new` to a table — checking, on
/// the way, that each statement finds the environment AxCut's typing rules
/// promise it.  Execution starts at the first definition.
pub fn assemble(program: &Program) -> Result<Code, MachineError> {
    let Some(main) = program.definitions.first() else { return malformed("there is nothing to run") };
    if !main.params.is_empty() {
        return malformed("the entry point takes parameters");
    }
    let mut asm = Assembler { program, instrs: Vec::new(), tables: Vec::new(), jumps: Vec::new() };
    let mut entries: HashMap<&str, Pc> = HashMap::new();
    for d in &program.definitions {
        entries.insert(&d.label, asm.instrs.len());
        asm.statement(&d.body, d.params.clone())?;
    }
    for (at, label) in std::mem::take(&mut asm.jumps) {
        let Instr::Jump { target } = &mut asm.instrs[at] else { unreachable!("only jumps are patched") };
        *target = entries[label];
    }
    let return_table = asm.table();
    let pc = asm.emit(Instr::HandlerReturn);
    asm.enter(return_table, 0, Entry { symbol: Symbol::Ret, pc, params: 1 })?;
    let resume_table = asm.table();
    let pc = asm.emit(Instr::Resume);
    asm.enter(resume_table, 0, Entry { symbol: Symbol::Apply, pc, params: 2 })?;
    Ok(Code { instrs: asm.instrs, tables: asm.tables, return_table, resume_table })
}

/// Where `v` is, searching from the end, as the environment would be read.
fn position(env: &[Var], v: &Var) -> Result<Reg, MachineError> {
    match env.iter().rposition(|name| name == v) {
        Some(at) => Ok(at),
        None => malformed(format!("{v} is not in the environment")),
    }
}

/// Take `names` off the end of `env`, where they must be, in order.
fn take_tail(env: &mut Vec<Var>, names: &[Var], by: &str) -> Result<(), MachineError> {
    let Some(at) = env.len().checked_sub(names.len()) else {
        return malformed(format!("{by} needs ({}), but the environment is shorter", names.join(", ")));
    };
    if env[at..] != *names {
        return malformed(format!("{by} needs ({}) at the end, not ({})", names.join(", "), env[at..].join(", ")));
    }
    env.truncate(at);
    Ok(())
}

impl<'p> Assembler<'p> {
    fn emit(&mut self, i: Instr) -> Pc {
        self.instrs.push(i);
        self.instrs.len() - 1
    }

    fn table(&mut self) -> TableId {
        self.tables.push(Vec::new());
        self.tables.len() - 1
    }

    fn enter(&mut self, table: TableId, index: usize, entry: Entry) -> Result<(), MachineError> {
        let slots = &mut self.tables[table];
        if slots.len() <= index {
            slots.resize(index + 1, None);
        }
        if let Some(taken) = &slots[index] {
            return malformed(format!("{} and {} share a place in one table", taken.symbol, entry.symbol));
        }
        slots[index] = Some(entry);
        Ok(())
    }

    /// Assemble `s`, in environment `env`.  Its straight-line code comes out
    /// in one run, each instruction falling through to the next until one
    /// that transfers control; the clauses it contains come after.
    fn statement(&mut self, s: &'p Stmt, mut env: Vec<Var>) -> Result<(), MachineError> {
        match s {
            Stmt::Substitute(pairs, rest) => {
                let (mut share, mut erase) = (Vec::new(), Vec::new());
                for (reg, name) in env.iter().enumerate() {
                    match pairs.iter().filter(|(_, old)| old == name).count() {
                        0 => erase.push(reg),
                        1 => {}
                        n => share.push((reg, n - 1)),
                    }
                }
                let moves = pairs.iter().map(|(_, old)| position(&env, old)).collect::<Result<_, _>>()?;
                self.emit(Instr::Substitute { share, erase, moves });
                self.statement(rest, pairs.iter().map(|(new, _)| new.clone()).collect())
            }
            Stmt::Let(v, m, fields, rest) => {
                take_tail(&mut env, fields, "let")?;
                self.emit(Instr::Let { fields: fields.len(), symbol: m.clone() });
                env.push(v.clone());
                self.statement(rest, env)
            }
            Stmt::New(v, captured, clauses, rest) => {
                take_tail(&mut env, captured, "new")?;
                let table = self.table();
                self.emit(Instr::New { fields: captured.len(), table });
                env.push(v.clone());
                self.statement(rest, env)?;
                // the virtual table's clauses, after the code that made it
                for (m, branch) in clauses {
                    let pc = self.emit(Instr::Load { fields: captured.len(), what: "a closure environment" });
                    self.enter(table, method(m), Entry { symbol: m.clone(), pc, params: branch.params.len() })?;
                    let inner = branch.params.iter().chain(captured).cloned().collect();
                    self.statement(&branch.body, inner)?;
                }
                Ok(())
            }
            Stmt::Switch(v, clauses) => {
                take_tail(&mut env, std::slice::from_ref(v), "switch")?;
                let table = self.table();
                self.emit(Instr::Switch { table });
                // the jump table's clauses
                for (m, branch) in clauses {
                    let pc = self.emit(Instr::Load { fields: branch.params.len(), what: "a switch clause" });
                    self.enter(table, m.tag(), Entry { symbol: m.clone(), pc, params: branch.params.len() })?;
                    let inner = env.iter().chain(&branch.params).cloned().collect();
                    self.statement(&branch.body, inner)?;
                }
                Ok(())
            }
            Stmt::Invoke(v, m, args) => {
                take_tail(&mut env, std::slice::from_ref(v), "invoke")?;
                // what is left must be the arguments, exactly
                take_tail(&mut env, args, "invoke")?;
                if !env.is_empty() {
                    return malformed(format!("invoke {v} {m} leaves {} variables behind", env.len()));
                }
                self.emit(Instr::Invoke { symbol: m.clone(), args: args.len() });
                Ok(())
            }
            Stmt::Jump(l, args) => {
                let Some(target) = self.program.definition(l) else { return malformed(format!("no definition of {l}")) };
                take_tail(&mut env, args, "jump")?;
                if !env.is_empty() {
                    return malformed(format!("jump {l} leaves {} variables behind", env.len()));
                }
                if target.params.len() != args.len() {
                    return malformed(format!(
                        "a label binds {} variables, but is given {}",
                        target.params.len(),
                        args.len()
                    ));
                }
                let at = self.emit(Instr::Jump { target: 0 });
                self.jumps.push((at, l));
                Ok(())
            }
            Stmt::Handle(h, k, ret, rest) => {
                take_tail(&mut env, &[h.clone(), k.clone()], "handle")?;
                self.emit(Instr::Handle);
                env.push(ret.clone());
                self.statement(rest, env)
            }
            Stmt::Perform(op, xs, k) => {
                let taken: Vec<Var> = xs.iter().chain([k]).cloned().collect();
                take_tail(&mut env, &taken, "perform")?;
                if !env.is_empty() {
                    return malformed(format!("perform {op} leaves {} variables behind", env.len()));
                }
                self.emit(Instr::Perform { op: op.clone(), args: xs.len() });
                Ok(())
            }
            Stmt::Extern(Extern::Halt, args, _) => match args.as_slice() {
                [answer] => {
                    let answer = position(&env, answer)?;
                    self.emit(Instr::Halt { answer });
                    Ok(())
                }
                _ => malformed(format!("extern halt is given {} arguments", args.len())),
            },
            Stmt::Extern(Extern::Native(native), args, branches) => {
                if args.len() != native.arity() {
                    return malformed(format!("{} is given {} arguments", native.name(), args.len()));
                }
                let [branch] = branches.as_slice() else { return malformed("a native continues one way") };
                if branch.params.len() != 1 {
                    return malformed(format!("a native's clause binds {} variables, but is given 1", branch.params.len()));
                }
                take_tail(&mut env, args, native.name())?;
                self.emit(Instr::Native { native: *native, args: args.len() });
                env.extend(branch.params.iter().cloned());
                self.statement(&branch.body, env)
            }
            Stmt::Extern(op, args, branches) => {
                let arity = match op {
                    Extern::LitInt(_) | Extern::LitStr(_) | Extern::LitChar(_) => 0,
                    Extern::Add
                    | Extern::Mul
                    | Extern::Sub
                    | Extern::Div
                    | Extern::Mod
                    | Extern::Lt
                    | Extern::Eq
                    | Extern::Append => 2,
                    Extern::Halt | Extern::Native(_) => unreachable!("handled above"),
                };
                if args.len() != arity {
                    return malformed(format!("extern {op} is given {} arguments", args.len()));
                }
                let [branch] = branches.as_slice() else { return malformed(format!("extern {op} continues one way")) };
                if branch.params.len() != 1 {
                    return malformed(format!("an extern clause binds {} variables, but is given 1", branch.params.len()));
                }
                let args = args.iter().map(|a| position(&env, a)).collect::<Result<_, _>>()?;
                self.emit(Instr::Extern { op: op.clone(), args });
                env.extend(branch.params.iter().cloned());
                self.statement(&branch.body, env)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Values and the heap
// ---------------------------------------------------------------------------

type Addr = usize;

/// A register's worth: two words, as it is two registers in the paper.
#[derive(Clone, Debug)]
struct Value {
    block: Option<Addr>,
    word: Word,
}

#[derive(Clone, Debug)]
enum Word {
    /// A producer's constructor: its tag, which is all hardware would have,
    /// and its name, for showing the answer.
    Tag(Symbol),
    /// A consumer's virtual table.
    Table(TableId),
    /// 64 bits, wrapping, as native code has it
    Int(i64),
    Str(Rc<str>),
    /// a character: in native code, its code point
    Char(char),
    /// An array, whose elements are its block's fields.
    Array,
}

/// The first field of a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Header {
    /// In use: the references to the block beyond the first.
    Count(usize),
    /// On the free list or the todo list: the next block on it.
    Link(Option<Addr>),
}

#[derive(Debug)]
struct Block {
    header: Header,
    fields: Vec<Value>,
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
struct Heap {
    blocks: Vec<Block>,
    /// the free list's register: blocks ready to use
    heap: Option<Addr>,
    /// the todo list's register: dead blocks, their fields not yet erased
    todo: Option<Addr>,
    in_use: usize,
    memory: Memory,
}

impl Heap {
    /// `ShareBlock`: `n` more references to whatever `v` points at.
    fn share(&mut self, v: &Value, n: usize) {
        let Some(a) = v.block else { return };
        if let Header::Count(count) = &mut self.blocks[a].header {
            *count += n;
            self.memory.shares += n;
        }
    }

    /// `EraseBlock`: one reference fewer.  The last one leaves the block
    /// dead, and puts it on the todo list with what it holds still in it.
    fn erase(&mut self, v: &Value) {
        let Some(a) = v.block else { return };
        match self.blocks[a].header {
            Header::Count(0) => {
                self.blocks[a].header = Header::Link(self.todo);
                self.todo = Some(a);
                self.in_use -= 1;
            }
            Header::Count(n) => self.blocks[a].header = Header::Count(n - 1),
            Header::Link(_) => {}
        }
    }

    /// `Store` and `Acquire`: a block holding `fields`.  From the free list
    /// if it has one; else a dead block from the todo list, whose own fields
    /// are erased now; else fresh memory.
    fn acquire(&mut self, fields: Vec<Value>) -> Option<Addr> {
        if fields.is_empty() {
            return None;
        }
        let a = if let Some(a) = self.heap {
            self.memory.reused += 1;
            self.heap = self.link(a);
            a
        } else if let Some(a) = self.todo {
            self.memory.reused_lazily += 1;
            self.todo = self.link(a);
            for old in std::mem::take(&mut self.blocks[a].fields) {
                self.erase(&old);
            }
            a
        } else {
            self.memory.fresh += 1;
            self.blocks.push(Block { header: Header::Link(None), fields: Vec::new() });
            self.blocks.len() - 1
        };
        self.blocks[a] = Block { header: Header::Count(0), fields };
        self.in_use += 1;
        self.memory.peak = self.memory.peak.max(self.in_use);
        Some(a)
    }

    fn link(&self, a: Addr) -> Option<Addr> {
        match self.blocks[a].header {
            Header::Link(next) => next,
            Header::Count(_) => unreachable!("a block on a list is linked"),
        }
    }

    /// `Release` and `Load`: the fields back out.  A unique block gives them
    /// up and goes on the free list; a shared one keeps them, and they are
    /// shared once more.
    fn release(&mut self, block: Option<Addr>) -> Result<Vec<Value>, MachineError> {
        let Some(a) = block else { return Ok(Vec::new()) };
        match self.blocks[a].header {
            Header::Link(_) => malformed(format!("block {a} is used after it was freed")),
            Header::Count(0) => {
                self.memory.unique_releases += 1;
                self.blocks[a].header = Header::Link(self.heap);
                self.heap = Some(a);
                self.in_use -= 1;
                Ok(std::mem::take(&mut self.blocks[a].fields))
            }
            Header::Count(n) => {
                self.memory.shared_releases += 1;
                self.blocks[a].header = Header::Count(n - 1);
                let fields = self.blocks[a].fields.clone();
                for f in &fields {
                    self.share(f, 1);
                }
                Ok(fields)
            }
        }
    }

    fn fields(&self, block: Option<Addr>) -> &[Value] {
        block.map_or(&[], |a| &self.blocks[a].fields)
    }

    /// As the REPL shows a value, if it is plain data: a function or a type
    /// has nothing worth showing.
    fn show(&self, v: &Value) -> Option<String> {
        let Word::Tag(m) = &v.word else {
            return match &v.word {
                Word::Int(n) => Some(n.to_string()),
                Word::Str(s) => Some(format!("{:?}", &**s)),
                Word::Char(c) => Some(idyllic_ast::show_char(*c)),
                // `#[1; 2]`, as one is written
                Word::Array => {
                    let items = self.fields(v.block).iter().map(|x| self.show(x)).collect::<Option<Vec<String>>>()?;
                    Some(match items.as_slice() {
                        [] => "#[;]".into(),
                        [one] => format!("#[{one};]"),
                        _ => format!("#[{}]", items.join("; ")),
                    })
                }
                _ => None,
            };
        };
        match (m, self.fields(v.block)) {
            (Symbol::True, []) => Some("True".into()),
            (Symbol::False, []) => Some("False".into()),
            (Symbol::Unit, []) => Some("()".into()),
            (Symbol::Box, [inner]) => Some(format!("[{}]", self.show(inner)?)),
            // a pair is a tuple, `(1, True)`, and one nested to the right a
            // longer one, `(1, 2, 3)`
            (Symbol::Con(name, _), [_, _]) if &**name == "Pair" => {
                let mut items = Vec::new();
                let mut at = v.clone();
                while let (Word::Tag(Symbol::Con(name, _)), [first, rest]) = (&at.word, self.fields(at.block)) {
                    if &**name != "Pair" {
                        break;
                    }
                    items.push(self.show(first)?);
                    at = rest.clone();
                }
                items.push(self.show(&at)?);
                Some(format!("({})", items.join(", ")))
            }
            // `x :: xs` is written the way it is read; its head is
            // parenthesised if it is itself compound
            (Symbol::Con(name, _), [head, tail]) if &**name == "::" => {
                let head = self.show(head)?;
                let head = if head.contains(' ') && !head.starts_with(['[', '"', '#', '(']) { format!("({head})") } else { head };
                Some(format!("{head} :: {}", self.show(tail)?))
            }
            (Symbol::Con(name, _), fields) => {
                let mut out = name.to_string();
                for f in fields {
                    let f = self.show(f)?;
                    let compound = f.contains(' ') && !f.starts_with(['[', '"', '#', '(']);
                    out.push_str(&if compound { format!(" ({f})") } else { format!(" {f}") });
                }
                Some(out)
            }
            _ => None,
        }
    }

    /// The end-of-run audit.  Everything but the answer is dropped and the
    /// todo list worked off, which leaves exactly what is truly live; that
    /// must be what the answer can reach, each block counted right.
    fn audit(&mut self, answer: &Value, others: Vec<Value>) {
        for v in &others {
            self.erase(v);
        }
        while let Some(a) = self.todo {
            self.todo = self.link(a);
            for old in std::mem::take(&mut self.blocks[a].fields) {
                self.erase(&old);
            }
            self.blocks[a].header = Header::Link(self.heap);
            self.heap = Some(a);
        }
        // references to each block: from the answer, and from live fields
        let mut references: HashMap<Addr, usize> = HashMap::new();
        let mut pending: Vec<Addr> = answer.block.into_iter().collect();
        if let Some(a) = answer.block {
            references.insert(a, 1);
        }
        while let Some(a) = pending.pop() {
            for f in &self.blocks[a].fields {
                if let Some(b) = f.block {
                    let seen = references.entry(b).or_insert(0);
                    *seen += 1;
                    if *seen == 1 {
                        pending.push(b);
                    }
                }
            }
        }
        for (a, block) in self.blocks.iter().enumerate() {
            let Header::Count(count) = block.header else { continue };
            match references.get(&a) {
                None => self.memory.leaked += 1,
                Some(&n) if n != count + 1 => self.memory.miscounted += 1,
                Some(_) => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

/// How a run ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// The answer as the REPL shows it, if it is plain data: a function or a
    /// type has nothing worth showing.
    pub answer: Option<String>,
    pub memory: Memory,
    /// What the program wrote to the console.
    pub output: String,
}

/// Assemble a program and run it from its first definition until it halts,
/// for at most `fuel` steps.
pub fn run(program: &Program, fuel: u64) -> Result<Outcome, MachineError> {
    let mut world = World::captured();
    let mut outcome = run_in(program, fuel, &mut world)?;
    outcome.output = world.into_output();
    Ok(outcome)
}

/// The same, in `world`, which the world's operations are done in (see
/// [`idyllic_world`]); what they print is there even if the program stops
/// early.  The outcome's `output` is left empty.
pub fn run_in(program: &Program, fuel: u64, world: &mut World) -> Result<Outcome, MachineError> {
    execute(&assemble(program)?, fuel, world)
}

fn integer(regs: &[Value], r: Reg) -> Result<i64, MachineError> {
    match &regs[r].word {
        Word::Int(n) => Ok(*n),
        _ => malformed(format!("register {r} is not an integer")),
    }
}

/// A comparison's answer: a producer with no fields, and so no block.
fn truth(b: bool) -> Value {
    Value { block: None, word: Word::Tag(if b { Symbol::True } else { Symbol::False }) }
}

/// Where `invoke` goes: the clause for `symbol` of the consumer in the last
/// register, which must bind `args` variables.
fn dispatch(code: &Code, regs: &[Value], symbol: &Symbol, args: usize) -> Result<Pc, MachineError> {
    let Some(Value { word: Word::Table(table), .. }) = regs.last() else {
        return malformed(format!("invoke {symbol} is on something that is not a consumer"));
    };
    let clause = entry(code, *table, method(symbol), symbol)?;
    if clause.symbol != *symbol {
        return malformed(format!("no clause for {symbol}"));
    }
    if clause.params != args {
        return malformed(format!("an invoked clause binds {} variables, but is given {args}", clause.params));
    }
    Ok(clause.pc)
}

/// The table entry for `index`, or the error of there being none.
fn entry<'c>(code: &'c Code, table: TableId, index: usize, m: &Symbol) -> Result<&'c Entry, MachineError> {
    match code.tables[table].get(index) {
        Some(Some(e)) => Ok(e),
        _ => malformed(format!("no clause for {m}")),
    }
}

/// An operation nothing in the program handles is one of the world's, the
/// runtime's to do (the type checker has seen to it that no other gets this
/// far): what it gives back, as a value.
fn world(world: &mut World, heap: &mut Heap, op: &Symbol, xs: &[Value]) -> Result<Value, MachineError> {
    let unhandled = || malformed(format!("{op} is performed with no handler for it in force"));
    let Symbol::Op(name, _) = op else { return unhandled() };
    let mut args = Vec::new();
    for x in xs {
        args.push(match &x.word {
            Word::Int(n) => Arg::Int(*n),
            Word::Str(s) => Arg::Str(s.clone()),
            // `()`, and whatever else carries nothing an operation reads
            _ => Arg::Int(0),
        });
    }
    match world.perform(name, &args) {
        None => unhandled(),
        Some(Err(Stop::Exit(code))) => Err(MachineError::Exited(code)),
        Some(Err(Stop::Failed(why))) => Err(MachineError::TestFailed(why)),
        Some(Ok(answer)) => Ok(answer_value(heap, answer)),
    }
}

/// A value of the machine's, for one of the world's answers.
fn answer_value(heap: &mut Heap, answer: Answer) -> Value {
    let word = |word| Value { block: None, word };
    match answer {
        Answer::Unit => word(Word::Tag(Symbol::Unit)),
        Answer::Int(n) => word(Word::Int(n)),
        Answer::Bool(b) => truth(b),
        Answer::Str(s) => word(Word::Str(s)),
        Answer::Array(items) => {
            let items = items.into_iter().map(|x| answer_value(heap, x)).collect();
            heap.array(items)
        }
        Answer::Con(name, tag, fields) => {
            let fields = fields.into_iter().map(|f| answer_value(heap, f)).collect();
            Value { block: heap.acquire(fields), word: Word::Tag(Symbol::Con(name.into(), tag)) }
        }
    }
}

// ---------------------------------------------------------------------------
// Natives
// ---------------------------------------------------------------------------

impl Heap {
    fn array(&mut self, elements: Vec<Value>) -> Value {
        Value { block: self.acquire(elements), word: Word::Array }
    }

    /// The elements of an array that is not consumed.
    fn elements(&self, xs: &Value, by: Native) -> Result<&[Value], MachineError> {
        match xs.word {
            Word::Array => Ok(self.fields(xs.block)),
            _ => malformed(format!("{} is given something that is not an array", by.name())),
        }
    }

    /// The elements of an array that is consumed, each a reference of its
    /// own: moved out of a unique one, whose block is free again (and so
    /// what the next array made takes — in place, in effect), and copied
    /// out of a shared one.
    fn take_elements(&mut self, xs: Value, by: Native) -> Result<Vec<Value>, MachineError> {
        self.elements(&xs, by)?;
        self.release(xs.block)
    }

    /// The element at `i`, shared once more, and the array dropped.
    fn element(&mut self, xs: &Value, i: i64, by: Native) -> Result<Value, MachineError> {
        let items = self.elements(xs, by)?;
        let len = items.len();
        let x = usize::try_from(i).ok().and_then(|i| items.get(i)).cloned().ok_or(MachineError::OutOfBounds { index: i, len })?;
        self.share(&x, 1);
        Ok(x)
    }

    /// `Pair [a] b`: `Std.Tuple`'s one constructor, which is how a native
    /// gives back two things, the first in a box.
    fn boxed_pair(&mut self, a: Value, b: Value) -> Value {
        let boxed = Value { block: self.acquire(vec![a]), word: Word::Tag(Symbol::Box) };
        Value { block: self.acquire(vec![boxed, b]), word: Word::Tag(Symbol::Con("Pair".into(), 0)) }
    }
}

/// Where `i` is in `xs`, or the error of its not being there.
fn index(i: i64, len: usize) -> Result<usize, MachineError> {
    usize::try_from(i).ok().filter(|i| *i < len).ok_or(MachineError::OutOfBounds { index: i, len })
}

/// Do a native, consuming `xs`, its arguments.
fn native(heap: &mut Heap, n: Native, xs: Vec<Value>) -> Result<Value, MachineError> {
    let int = |v: &Value| match v.word {
        Word::Int(i) => Ok(i),
        _ => malformed(format!("{} is given something that is not an integer", n.name())),
    };
    let int_value = |i: i64| Value { block: None, word: Word::Int(i) };
    let text = |v: &Value| match &v.word {
        Word::Str(s) => Ok(s.clone()),
        _ => malformed(format!("{} is given something that is not a string", n.name())),
    };
    let str_value = |s: String| Value { block: None, word: Word::Str(s.into()) };
    let mut xs = xs.into_iter();
    let mut next = || xs.next().ok_or_else(|| MachineError::Malformed(format!("{} is given too little", n.name())));
    Ok(match n {
        Native::Array | Native::MArray => return malformed("a type is not run"),
        Native::ArrayLen => {
            let a = next()?;
            let len = heap.elements(&a, n)?.len();
            heap.erase(&a);
            int_value(len as i64)
        }
        Native::ArrayGet => {
            let (a, i) = (next()?, next()?);
            let x = heap.element(&a, int(&i)?, n)?;
            heap.erase(&a);
            x
        }
        Native::ArrayGetOr => {
            let (d, a, i) = (next()?, next()?, next()?);
            let len = heap.elements(&a, n)?.len();
            let i = int(&i)?;
            let x = if usize::try_from(i).is_ok_and(|i| i < len) {
                heap.erase(&d);
                heap.element(&a, i, n)?
            } else {
                d
            };
            heap.erase(&a);
            x
        }
        Native::ArraySet | Native::MArraySet => {
            let (a, i, x) = (next()?, next()?, next()?);
            let mut items = heap.take_elements(a, n)?;
            let at = index(int(&i)?, items.len())?;
            let old = std::mem::replace(&mut items[at], x);
            heap.erase(&old);
            heap.array(items)
        }
        Native::ArrayPush => {
            let (a, x) = (next()?, next()?);
            let mut items = heap.take_elements(a, n)?;
            items.push(x);
            heap.array(items)
        }
        Native::ArrayPop => {
            let a = next()?;
            let mut items = heap.take_elements(a, n)?;
            let last = items.pop().ok_or(MachineError::EmptyArray)?;
            heap.erase(&last);
            heap.array(items)
        }
        Native::ArraySlice => {
            let (a, from, to) = (next()?, next()?, next()?);
            let mut items = heap.take_elements(a, n)?;
            let len = items.len() as i64;
            let from = int(&from)?.clamp(0, len) as usize;
            let to = (int(&to)?.clamp(0, len) as usize).max(from);
            let kept: Vec<Value> = items.drain(from..to).collect();
            for dropped in &items {
                heap.erase(dropped);
            }
            heap.array(kept)
        }
        Native::ArrayConcat => {
            let (a, b) = (next()?, next()?);
            let mut items = heap.take_elements(a, n)?;
            items.extend(heap.take_elements(b, n)?);
            heap.array(items)
        }
        Native::ArrayFromList => {
            let mut list = next()?;
            let mut items = Vec::new();
            loop {
                match &list.word {
                    Word::Tag(Symbol::Con(_, 0)) => break,
                    Word::Tag(Symbol::Con(_, 1)) => {
                        let mut cell = heap.release(list.block)?.into_iter();
                        let (Some(head), Some(tail)) = (cell.next(), cell.next()) else {
                            return malformed("a list's cell is not a head and a tail");
                        };
                        items.push(head);
                        list = tail;
                    }
                    _ => return malformed("arrayFromList is given something that is not a list"),
                }
            }
            heap.array(items)
        }
        Native::ArrayToList => {
            let a = next()?;
            let items = heap.take_elements(a, n)?;
            let nil = Value { block: None, word: Word::Tag(Symbol::Con("Nil".into(), 0)) };
            items.into_iter().rev().fold(nil, |tail, head| Value {
                block: heap.acquire(vec![head, tail]),
                word: Word::Tag(Symbol::Con("::".into(), 1)),
            })
        }
        Native::MArrayNew => {
            let (count, x) = (next()?, next()?);
            let count = int(&count)?.max(0) as usize;
            match count {
                0 => heap.erase(&x),
                _ => heap.share(&x, count - 1),
            }
            heap.array(vec![x; count])
        }
        Native::MArrayLen => {
            let a = next()?;
            let len = heap.elements(&a, n)?.len();
            heap.boxed_pair(int_value(len as i64), a)
        }
        Native::MArrayGet => {
            let (a, i) = (next()?, next()?);
            let x = heap.element(&a, int(&i)?, n)?;
            heap.boxed_pair(x, a)
        }
        // the same block, under the other type
        Native::MArrayFreeze | Native::MArrayThaw => next()?,
        // a string has no block, so nothing of it is shared or dropped
        Native::StringByteLength => int_value(text(&next()?)?.len() as i64),
        Native::StringByteAt => {
            let (s, i) = (text(&next()?)?, int(&next()?)?);
            int_value(strings::byte_at(&s, i).ok_or(MachineError::ByteOutOfBounds { index: i, len: s.len() })?)
        }
        Native::StringSlice => {
            let (s, from, to) = (text(&next()?)?, int(&next()?)?, int(&next()?)?);
            str_value(strings::slice(&s, from, to))
        }
        Native::StringCompare => {
            let (a, b) = (text(&next()?)?, text(&next()?)?);
            int_value(strings::compare(&a, &b))
        }
        Native::StringIndexOf => {
            let (hay, needle, from) = (text(&next()?)?, text(&next()?)?, int(&next()?)?);
            int_value(strings::index_of(&hay, &needle, from))
        }
        Native::StringToChars => {
            let s = text(&next()?)?;
            let chars = s.chars().map(|c| Value { block: None, word: Word::Char(c) }).collect();
            heap.array(chars)
        }
        Native::CharsToString => {
            let a = next()?;
            let chars = heap
                .elements(&a, n)?
                .iter()
                .map(|v| match v.word {
                    Word::Char(c) => Ok(c),
                    _ => malformed("charsToString is given something that is not a character"),
                })
                .collect::<Result<String, _>>()?;
            heap.erase(&a);
            str_value(chars)
        }
        Native::CharCode => match next()?.word {
            Word::Char(c) => int_value(i64::from(u32::from(c))),
            _ => return malformed("charCode is given something that is not a character"),
        },
        Native::CharFromCode => {
            let code = int(&next()?)?;
            Value { block: None, word: Word::Char(strings::from_code(code).ok_or(MachineError::NotACharacter(code))?) }
        }
    })
}

/// Run assembled code from address `0` until it halts, for at most `fuel`
/// steps.
pub fn execute(code: &Code, mut fuel: u64, world: &mut World) -> Result<Outcome, MachineError> {
    let mut pc: Pc = 0;
    let mut regs: Vec<Value> = Vec::new();
    let mut heap = Heap::default();
    // the handlers in force, newest last: each with where its `handle` goes
    let mut handlers: Vec<(Value, Value)> = Vec::new();
    loop {
        let instr = &code.instrs[pc];
        if !matches!(instr, Instr::Load { .. }) {
            fuel = fuel.checked_sub(1).ok_or(MachineError::OutOfFuel)?;
        }
        pc = match instr {
            Instr::Substitute { share, erase, moves } => {
                for &(r, n) in share {
                    heap.share(&regs[r], n);
                }
                for &r in erase {
                    // (an integer has no block, and nothing to count)
                    heap.memory.erases += usize::from(regs[r].block.is_some());
                    heap.erase(&regs[r]);
                }
                regs = moves.iter().map(|&r| regs[r].clone()).collect();
                pc + 1
            }
            Instr::Let { fields, symbol } => {
                let stored = regs.split_off(regs.len() - fields);
                regs.push(Value { block: heap.acquire(stored), word: Word::Tag(symbol.clone()) });
                pc + 1
            }
            Instr::New { fields, table } => {
                let stored = regs.split_off(regs.len() - fields);
                regs.push(Value { block: heap.acquire(stored), word: Word::Table(*table) });
                pc + 1
            }
            Instr::Switch { table } => {
                let Some(Value { word: Word::Tag(m), .. }) = regs.last() else {
                    return malformed("a switch is on something that is not a producer");
                };
                entry(code, *table, m.tag(), m)?.pc
            }
            Instr::Invoke { symbol, args } => dispatch(code, &regs, symbol, *args)?,
            Instr::Handle => {
                let k = regs.pop().expect("assembly saw to it");
                let h = regs.pop().expect("assembly saw to it");
                handlers.push((h, k));
                regs.push(Value { block: None, word: Word::Table(code.return_table) });
                pc + 1
            }
            Instr::HandlerReturn => {
                regs.pop();
                let Some((h, k)) = handlers.pop() else { return malformed("a handler returned with none in force") };
                regs.push(k);
                regs.push(h);
                dispatch(code, &regs, &Symbol::Return, 2)?
            }
            Instr::Perform { op, args } => 'perform: {
                let k = regs.pop().expect("assembly saw to it");
                let xs = regs.split_off(regs.len() - args);
                let handles = |h: &Value| match &h.word {
                    Word::Table(t) => matches!(code.tables[*t].get(method(op)), Some(Some(e)) if e.symbol == *op),
                    _ => false,
                };
                let Some(at) = handlers.iter().rposition(|(h, _)| handles(h)) else {
                    // the runtime's, then: done, and the rest goes on with
                    // what it gave back
                    let answer = self::world(world, &mut heap, op, &xs)?;
                    xs.iter().for_each(|x| heap.erase(x));
                    regs.push(answer);
                    regs.push(k);
                    break 'perform dispatch(code, &regs, &Symbol::Ret, 1)?;
                };
                let mut taken = handlers.split_off(at).into_iter();
                let (h, outer) = taken.next().expect("at least the one found");
                // the handler is kept in the resumption and entered now
                heap.share(&h, 1);
                let mut kept = vec![k, h.clone()];
                for (h, k) in taken {
                    kept.push(h);
                    kept.push(k);
                }
                let resumption = Value { block: heap.acquire(kept), word: Word::Table(code.resume_table) };
                regs.extend(xs);
                regs.extend([resumption, outer, h]);
                dispatch(code, &regs, op, args + 2)?
            }
            Instr::Resume => {
                let resumption = regs.pop().expect("a clause is entered by dispatching on a value");
                let caller = regs.pop().expect("a resumption is given where to go");
                let mut kept = heap.release(resumption.block)?.into_iter();
                let (Some(k), Some(h)) = (kept.next(), kept.next()) else { return malformed("a resumption keeps too little") };
                handlers.push((h, caller));
                while let (Some(h), Some(k)) = (kept.next(), kept.next()) {
                    handlers.push((h, k));
                }
                regs.push(k);
                dispatch(code, &regs, &Symbol::Ret, 1)?
            }
            Instr::Load { fields, what } => {
                let dispatched = regs.pop().expect("a clause is entered by dispatching on a value");
                let loaded = heap.release(dispatched.block)?;
                if loaded.len() != *fields {
                    return malformed(format!("{what} binds {fields} variables, but is given {}", loaded.len()));
                }
                regs.extend(loaded);
                pc + 1
            }
            Instr::Jump { target } => *target,
            Instr::Extern { op, args } => {
                let result = match (op, args.as_slice()) {
                    (Extern::LitInt(n), []) => Value { block: None, word: Word::Int(*n) },
                    (Extern::LitStr(s), []) => Value { block: None, word: Word::Str(s.as_str().into()) },
                    (Extern::LitChar(c), []) => Value { block: None, word: Word::Char(*c) },
                    (Extern::Append, &[a, b]) => match (&regs[a].word, &regs[b].word) {
                        (Word::Str(x), Word::Str(y)) => Value { block: None, word: Word::Str(format!("{x}{y}").into()) },
                        _ => return malformed("append is given something that is not a string"),
                    },
                    (op, &[a, b]) => {
                        let prim = op.prim().expect("assembly checked every extern's arity");
                        match prim.on_ints(integer(&regs, a)?, integer(&regs, b)?) {
                            Some(IntResult::Int(n)) => Value { block: None, word: Word::Int(n) },
                            Some(IntResult::Bool(t)) => truth(t),
                            None => return Err(MachineError::DivisionByZero),
                        }
                    }
                    _ => unreachable!("assembly checked every extern's arity"),
                };
                regs.push(result);
                pc + 1
            }
            Instr::Native { native, args } => {
                let xs = regs.split_off(regs.len() - args);
                let result = self::native(&mut heap, *native, xs)?;
                regs.push(result);
                pc + 1
            }
            Instr::Halt { answer } => {
                let answer = regs.remove(*answer);
                let shown = heap.show(&answer);
                let others = regs.into_iter().chain(handlers.into_iter().flat_map(|(h, k)| [h, k])).collect();
                heap.audit(&answer, others);
                return Ok(Outcome { answer: shown, memory: heap.memory, output: String::new() });
            }
        };
    }
}
