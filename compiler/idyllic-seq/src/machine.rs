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
//! Idyll has recursion and, so far, nothing to stop one with, so the machine
//! runs on fuel: one unit per rule of Figure 5.

use super::{Extern, Program, Stmt, Symbol, Var};
use num_bigint::BigInt;
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
    Invoke { method: usize, symbol: Symbol, args: usize },
    /// (jump): straight to a label, whose parameters are the whole
    /// environment.
    Jump { target: Pc },
    /// (extern): compute from `args`, which stay where they are, and push
    /// the result.
    Extern { op: Extern, args: Vec<Reg> },
    /// `extern halt(v) {}`: the program's answer is register `answer`.
    Halt { answer: Reg },
    /// At the entry of a clause: take the block of the last register, which
    /// the dispatch landed here by, and put its `fields` in its place.
    Load { fields: usize, what: &'static str },
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
    /// Indexed by a constructor's tag (a jump table) or by a destructor's
    /// position in its signature (a virtual table); `None` where the table
    /// has no clause.
    tables: Vec<Vec<Option<Entry>>>,
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
    Ok(Code { instrs: asm.instrs, tables: asm.tables })
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
                self.emit(Instr::Invoke { method: method(m), symbol: m.clone(), args: args.len() });
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
            Stmt::Extern(Extern::Halt, args, _) => match args.as_slice() {
                [answer] => {
                    let answer = position(&env, answer)?;
                    self.emit(Instr::Halt { answer });
                    Ok(())
                }
                _ => malformed(format!("extern halt is given {} arguments", args.len())),
            },
            Stmt::Extern(op, args, branches) => {
                let arity = match op {
                    Extern::LitInt(_) | Extern::LitStr(_) => 0,
                    Extern::Add | Extern::Mul | Extern::Sub | Extern::Lt | Extern::Eq => 2,
                    Extern::Halt => unreachable!("handled above"),
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
    Int(BigInt),
    Str(Rc<str>),
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
                _ => None,
            };
        };
        match (m, self.fields(v.block)) {
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
}

/// Assemble a program and run it from its first definition until it halts,
/// for at most `fuel` steps.
pub fn run(program: &Program, fuel: u64) -> Result<Outcome, MachineError> {
    execute(&assemble(program)?, fuel)
}

fn integer(regs: &[Value], r: Reg) -> Result<BigInt, MachineError> {
    match &regs[r].word {
        Word::Int(n) => Ok(n.clone()),
        _ => malformed(format!("register {r} is not an integer")),
    }
}

/// A comparison's answer: a producer with no fields, and so no block.
fn truth(b: bool) -> Value {
    Value { block: None, word: Word::Tag(if b { Symbol::True } else { Symbol::False }) }
}

/// The table entry for `index`, or the error of there being none.
fn entry<'c>(code: &'c Code, table: TableId, index: usize, m: &Symbol) -> Result<&'c Entry, MachineError> {
    match code.tables[table].get(index) {
        Some(Some(e)) => Ok(e),
        _ => malformed(format!("no clause for {m}")),
    }
}

/// Run assembled code from address `0` until it halts, for at most `fuel`
/// steps.
pub fn execute(code: &Code, mut fuel: u64) -> Result<Outcome, MachineError> {
    let mut pc: Pc = 0;
    let mut regs: Vec<Value> = Vec::new();
    let mut heap = Heap::default();
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
            Instr::Invoke { method, symbol, args } => {
                let Some(Value { word: Word::Table(table), .. }) = regs.last() else {
                    return malformed(format!("invoke {symbol} is on something that is not a consumer"));
                };
                let clause = entry(code, *table, *method, symbol)?;
                if clause.symbol != *symbol {
                    return malformed(format!("no clause for {symbol}"));
                }
                if clause.params != *args {
                    return malformed(format!("an invoked clause binds {} variables, but is given {args}", clause.params));
                }
                clause.pc
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
                    (Extern::LitInt(n), []) => Value { block: None, word: Word::Int(n.clone()) },
                    (Extern::LitStr(s), []) => Value { block: None, word: Word::Str(s.as_str().into()) },
                    (Extern::Add, &[a, b]) => Value { block: None, word: Word::Int(integer(&regs, a)? + integer(&regs, b)?) },
                    (Extern::Mul, &[a, b]) => Value { block: None, word: Word::Int(integer(&regs, a)? * integer(&regs, b)?) },
                    (Extern::Sub, &[a, b]) => Value { block: None, word: Word::Int(integer(&regs, a)? - integer(&regs, b)?) },
                    (Extern::Lt, &[a, b]) => truth(integer(&regs, a)? < integer(&regs, b)?),
                    (Extern::Eq, &[a, b]) => truth(integer(&regs, a)? == integer(&regs, b)?),
                    _ => unreachable!("assembly checked every extern's arity"),
                };
                regs.push(result);
                pc + 1
            }
            Instr::Halt { answer } => {
                let answer = regs.remove(*answer);
                let shown = heap.show(&answer);
                heap.audit(&answer, regs);
                return Ok(Outcome { answer: shown, memory: heap.memory });
            }
        };
    }
}
