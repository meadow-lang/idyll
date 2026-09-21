//! AxCut to LLVM IR.
//!
//! The translation is the one in the AxCut paper (its §4 and Figure 8), with
//! LLVM standing in for the assembler.  Almost nothing needs deciding:
//!
//! * A **variable** is two SSA values, as it is two registers in the paper:
//!   a pointer to a block (or null), and a word — a constructor's tag, a
//!   closure's code, or an integer.
//! * A **label**, and each clause of a function, is an LLVM function.  A
//!   clause takes its parameter and the block holding its closure
//!   environment, which it unloads on entry.
//! * **Continuations are the machine stack.**  This is where we part from
//!   the paper, which makes a continuation a heap block like any other
//!   consumer (as [`idyllic_seq::machine`] does).  A continuation is linear, and
//!   is only ever passed down to a call or returned to, so its life is a
//!   stack frame's: `new k = (Γ){ ret(r) ⇒ s }` becomes a basic block for
//!   `s`, with Γ left where it is (in SSA, so in the frame while a call is
//!   out); a function's continuation parameter is not passed, being the
//!   return address; `invoke k ret` is `ret`, or a branch to `k`'s block;
//!   and a `jump` or `invoke .. apply` is a `call` — which LLVM can inline,
//!   as it could not a jump that never comes back.
//! * A call whose continuation is the caller's own is a **tail call**, and
//!   a guaranteed one (`musttail`), so a loop runs in constant stack.  Other
//!   recursion uses the stack, of which a program is linked with plenty
//!   ([`STACK_BYTES`]).
//! * Every function has **one prototype**, under LLVM's `tailcc`
//!   ([`CONVENTION`]): the closure environment and four variables, all in
//!   registers, and a variable back.  A guaranteed tail call needs caller
//!   and callee to agree on that much.  A fifth variable and beyond — rare —
//!   travel through a global overflow area, written before the call and read
//!   on arrival.
//! * `switch` branches on the tag, within the function; its clauses are
//!   basic blocks.
//! * `substitute` costs **nothing** but the reference counts it implies.  On
//!   hardware it is a parallel move between registers; in SSA it is a
//!   renaming, which is no instruction at all.  What is left is what the
//!   paper calls Share and Erase: a count going up for each extra use of a
//!   variable, and down for each variable dropped.
//! * `let` and `new` acquire a block and store their fields; `switch` and
//!   `invoke` load them and release it.  That is all the memory management
//!   there is; see `rts/idyll_rt.c`.
//!
//! Integers are 64-bit here and wrap, where the machine's are unbounded:
//! the one place the two disagree.
//!
//! The output is linked with the runtime under link-time optimisation, so
//! that acquire and release are inlined into the code that uses them.

use idyllic_seq::{Branch, Extern, Program, Stmt, Symbol, Var};
use std::collections::HashSet;
use std::fmt::Write;
use std::rc::Rc;

/// The runtime, to be compiled alongside the program.
pub const RUNTIME: &str = include_str!("../../../rts/idyll_rt.c");

/// How the answer is to be printed: compiled code has no types left to tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultKind {
    Int,
    Bool,
    Unit,
    Str,
    /// a function, a type, a structure: nothing is shown
    Other,
}

/// The stack a program runs on.  It is only address space until it is used;
/// a call that waits on another takes a few dozen bytes of it.
pub const STACK_BYTES: u64 = 1 << 30;

/// How clang is to be run on the IR and the runtime: optimising across the
/// two, and telling the runtime how big a stack to run the program on.
///
/// The runtime makes that stack itself, as a thread's (`rts/idyll_rt.c`),
/// rather than asking the linker for it: `ld64.lld` ignores `-stack_size`,
/// so a macOS program got the default 8 MiB and a deep recursion crashed.
/// Only on Windows, which has no pthreads, does the linker set it.
pub fn clang_arguments() -> Vec<String> {
    let mut arguments: Vec<String> = ["-O2", "-flto", "-fuse-ld=lld", "-Wno-override-module"].map(String::from).into();
    arguments.push(format!("-DIDYLL_STACK_BYTES={STACK_BYTES}"));
    if cfg!(windows) {
        arguments.push(format!("-Wl,/STACK:{STACK_BYTES}"));
    }
    arguments
}

/// A variable, returned.
const VALUE: &str = "{ ptr, i64 }";

/// The calling convention of every function: LLVM's `tailcc`, which
/// guarantees a tail call where the call is marked `musttail`.  (Not GHC's
/// `ghccc`: on AArch64, LLVM 22 miscompiles a `ghccc` function that returns,
/// and `fib 20` never finished.)
pub const CONVENTION: &str = "tailcc";

/// What the runtime's blocks can hold.
const MAX_FIELDS: usize = 64;

/// How many variables travel in registers; the rest go by [`OVERFLOW`].
const REGISTER_VARS: usize = 4;
const OVERFLOW: &str = "@idyll_overflow";
const OVERFLOW_VARS: usize = 64;

/// The parameters every function has.
fn prototype() -> String {
    let vars: Vec<String> = (0..REGISTER_VARS).map(|i| format!("ptr %r{i}.b, i64 %r{i}.w")).collect();
    format!("ptr %env, {}", vars.join(", "))
}

/// Where variable `slot` of the overflow area is: its block, or its word.
fn overflow_slot(slot: usize, word: bool) -> String {
    format!("getelementptr ([{OVERFLOW_VARS} x {{ ptr, i64 }}], ptr {OVERFLOW}, i64 0, i64 {slot}, i32 {})", u8::from(word))
}

/// An AxCut variable, as native code has it.
#[derive(Clone, Debug)]
enum Val {
    /// Two SSA operands, as it is two registers in the paper.
    Data {
        block: String,
        word: String,
        /// known to have no block (an integer), so sharing and erasing it
        /// need no code at all
        external: bool,
    },
    /// The continuation this function was called with: the return address.
    Return,
    /// A continuation made in this function: a basic block of it.
    Local(Rc<Frame>),
}

/// What a continuation made here closes over.  The values stay where they
/// are, in SSA — which is to say, in this function's stack frame while a
/// call is out.
#[derive(Debug)]
struct Frame {
    id: usize,
    captured: Vec<Val>,
}

type Env = Vec<(Var, Val)>;

/// A function's clause waiting to be emitted as a function of its own.
struct Pending<'p> {
    name: String,
    captured: &'p [Var],
    branch: &'p Branch,
}

struct Emitter<'p> {
    continuations: &'p HashSet<Var>,
    functions: String,
    /// the current function's stack slots, which go at its top
    allocas: String,
    strings: Vec<String>,
    pending: Vec<Pending<'p>>,
    next: usize,
    result: ResultKind,
}

pub fn emit(program: &Program, result: ResultKind) -> Result<String, String> {
    let mut e = Emitter {
        continuations: &program.continuations,
        functions: String::new(),
        allocas: String::new(),
        strings: Vec::new(),
        pending: Vec::new(),
        next: 0,
        result,
    };
    for d in &program.definitions {
        e.function(&format!("L.{}", d.label), &d.params, None, &d.body)?;
    }
    while let Some(p) = e.pending.pop() {
        e.function(&p.name, &p.branch.params, Some(p.captured), &p.branch.body)?;
    }
    let Some(main) = program.definitions.first() else { return Err("there is nothing to compile".into()) };

    let mut out = String::new();
    out.push_str("; generated by idyllic from AxCut\n\n");
    for (i, s) in e.strings.iter().enumerate() {
        let bytes: String = s.bytes().map(|b| format!("\\{b:02X}")).collect();
        writeln!(out, "@str.{i} = private unnamed_addr constant [{} x i8] c\"{bytes}\\00\"", s.len() + 1).unwrap();
    }
    out.push_str(
        "\ndeclare ptr @idyll_acquire(i64)\n\
         declare void @idyll_release(ptr)\n\
         declare void @idyll_share(ptr, i64)\n\
         declare void @idyll_erase(ptr)\n\
         declare void @idyll_print_int(i64)\n\
         declare void @idyll_print_bool(i64)\n\
         declare void @idyll_print_unit()\n\
         declare void @idyll_print_string(ptr)\n\
         declare void @idyll_print_opaque()\n\
         declare void @exit(i32) noreturn\n\n",
    );
    writeln!(out, "{OVERFLOW} = internal global [{OVERFLOW_VARS} x {{ ptr, i64 }}] zeroinitializer\n").unwrap();
    out.push_str(&e.functions);
    let nothing = Emitter::call_arguments("undef", &[]);
    // the runtime's `main` calls this, on a stack big enough for it
    writeln!(out, "define void @idyll_run() {{\n  call {CONVENTION} {VALUE} @\"L.{}\"({nothing})\n  ret void\n}}", main.label).unwrap();
    Ok(out)
}

impl<'p> Emitter<'p> {
    fn fresh(&mut self, hint: &str) -> String {
        self.next += 1;
        format!("%{hint}{}", self.next)
    }

    /// One label, or one clause of a function (`captured` is then its
    /// closure environment).  The parameter that is a continuation is not
    /// passed: it is where the function returns to.
    fn function(&mut self, name: &str, params: &'p [Var], captured: Option<&'p [Var]>, body: &'p Stmt) -> Result<(), String> {
        let mut f = String::new();
        let mut env: Env = Vec::new();
        let mut slot = 0;
        for p in params {
            if self.continuations.contains(p) {
                env.push((p.clone(), Val::Return));
                continue;
            }
            let val = if slot < REGISTER_VARS {
                Val::Data { block: format!("%r{slot}.b"), word: format!("%r{slot}.w"), external: false }
            } else {
                // beyond the registers: left in the overflow area by the caller
                let (b, w) = (self.fresh("p"), self.fresh("p"));
                writeln!(f, "  {b} = load ptr, ptr {}", overflow_slot(slot - REGISTER_VARS, false)).unwrap();
                writeln!(f, "  {w} = load i64, ptr {}", overflow_slot(slot - REGISTER_VARS, true)).unwrap();
                Val::Data { block: b, word: w, external: false }
            };
            slot += 1;
            env.push((p.clone(), val));
        }
        if slot > REGISTER_VARS + OVERFLOW_VARS {
            return Err(format!("{name} takes {slot} variables, more than can be passed"));
        }
        // a clause begins by unloading its closure environment, which
        // releases the block it was in
        if let Some(captured) = captured.filter(|c| !c.is_empty()) {
            let loaded = self.load_fields(&mut f, "%env", captured.len());
            f.push_str("  call void @idyll_release(ptr %env)\n");
            env.extend(captured.iter().cloned().zip(loaded));
        }
        self.stmt(&mut f, body, env)?;
        let allocas = std::mem::take(&mut self.allocas);
        write!(self.functions, "define internal {CONVENTION} {VALUE} @\"{name}\"({}) nounwind {{\n{allocas}{f}}}\n\n", prototype()).unwrap();
        Ok(())
    }

    fn lookup<'e>(env: &'e Env, v: &Var) -> Result<&'e Val, String> {
        env.iter().rev().find(|(name, _)| name == v).map(|(_, val)| val).ok_or_else(|| format!("{v} is not in scope"))
    }

    /// The two operands of a value that is one.  A continuation is not: it
    /// lives on the stack, and cannot be stored or passed as an argument.
    fn data(v: &Val) -> Result<(&str, &str), String> {
        match v {
            Val::Data { block, word, .. } => Ok((block, word)),
            Val::Return | Val::Local(_) => Err("a continuation is used as a value".into()),
        }
    }

    /// Field `i` of a block is two words, after a header of two.
    fn field_offsets(i: usize) -> (usize, usize) {
        (16 + 16 * i, 24 + 16 * i)
    }

    fn load_fields(&mut self, f: &mut String, block: &str, n: usize) -> Vec<Val> {
        (0..n)
            .map(|i| {
                let (at_block, at_word) = Self::field_offsets(i);
                let (pb, pw, b, w) = (self.fresh("a"), self.fresh("a"), self.fresh("f"), self.fresh("f"));
                writeln!(f, "  {pb} = getelementptr i8, ptr {block}, i64 {at_block}").unwrap();
                writeln!(f, "  {b} = load ptr, ptr {pb}").unwrap();
                writeln!(f, "  {pw} = getelementptr i8, ptr {block}, i64 {at_word}").unwrap();
                writeln!(f, "  {w} = load i64, ptr {pw}").unwrap();
                Val::Data { block: b, word: w, external: false }
            })
            .collect()
    }

    /// Acquire a block and move `fields` into it.  Nothing to store needs no
    /// block.
    fn store_fields(&mut self, f: &mut String, fields: &[Val]) -> Result<String, String> {
        if fields.is_empty() {
            return Ok("null".to_string());
        }
        if fields.len() > MAX_FIELDS {
            return Err(format!("a block of {} fields is more than the runtime's {MAX_FIELDS}", fields.len()));
        }
        let block = self.fresh("b");
        writeln!(f, "  {block} = call ptr @idyll_acquire(i64 {})", fields.len()).unwrap();
        for (i, v) in fields.iter().enumerate() {
            let (field_block, field_word) = Self::data(v)?;
            let (at_block, at_word) = Self::field_offsets(i);
            let (pb, pw) = (self.fresh("a"), self.fresh("a"));
            writeln!(f, "  {pb} = getelementptr i8, ptr {block}, i64 {at_block}").unwrap();
            writeln!(f, "  store ptr {field_block}, ptr {pb}").unwrap();
            writeln!(f, "  {pw} = getelementptr i8, ptr {block}, i64 {at_word}").unwrap();
            writeln!(f, "  store i64 {field_word}, ptr {pw}").unwrap();
        }
        Ok(block)
    }

    /// Take `names` from the end of the environment.
    fn take(env: &mut Env, names: &[Var]) -> Result<Vec<Val>, String> {
        let at = env.len().checked_sub(names.len()).ok_or("the environment is too short")?;
        let tail = env.split_off(at);
        if tail.iter().map(|(v, _)| v).ne(names.iter()) {
            return Err(format!("expected ({}) at the end of the environment", names.join(", ")));
        }
        Ok(tail.into_iter().map(|(_, v)| v).collect())
    }

    /// The arguments of a call, in the one prototype: the environment, then
    /// the variables that fit in registers (the rest must already be in the
    /// overflow area), then nothing in particular.
    fn call_arguments(env: &str, values: &[(&str, &str)]) -> String {
        let mut out = vec![format!("ptr {env}")];
        for i in 0..REGISTER_VARS {
            out.push(match values.get(i) {
                Some((block, word)) => format!("ptr {block}, i64 {word}"),
                None => "ptr undef, i64 undef".to_string(),
            });
        }
        out.join(", ")
    }

    /// Let go of a variable that is dropped.  A continuation that will never
    /// be returned to lets go of what it was keeping.
    fn erase(f: &mut String, v: &Val) {
        match v {
            Val::Data { external: true, .. } | Val::Return => {}
            Val::Data { block, .. } => writeln!(f, "  call void @idyll_erase(ptr {block})").unwrap(),
            Val::Local(frame) => frame.captured.iter().for_each(|c| Self::erase(f, c)),
        }
    }

    /// Go on with a continuation, given its result.
    fn resume(&mut self, f: &mut String, k: &Val, block: &str, word: &str) -> Result<(), String> {
        match k {
            Val::Return => {
                let (only_block, both) = (self.fresh("v"), self.fresh("v"));
                writeln!(f, "  {only_block} = insertvalue {VALUE} undef, ptr {block}, 0").unwrap();
                writeln!(f, "  {both} = insertvalue {VALUE} {only_block}, i64 {word}, 1").unwrap();
                writeln!(f, "  ret {VALUE} {both}").unwrap();
            }
            Val::Local(frame) => {
                let id = frame.id;
                writeln!(f, "  store ptr {block}, ptr %k{id}.b").unwrap();
                writeln!(f, "  store i64 {word}, ptr %k{id}.w").unwrap();
                writeln!(f, "  br label %kont{id}").unwrap();
            }
            Val::Data { .. } => return Err("a continuation is expected, and this is a value on the heap".into()),
        }
        Ok(())
    }

    /// Call `callee` with `values`, one of which may be the continuation:
    /// where the call returns to.  If that is where *this* function returns
    /// to, the call is a tail call, and a guaranteed one.
    fn call(&mut self, f: &mut String, callee: &str, env: &str, values: &[Val]) -> Result<(), String> {
        let (arguments, continuations): (Vec<&Val>, Vec<&Val>) = values.iter().partition(|v| matches!(v, Val::Data { .. }));
        if continuations.len() > 1 {
            return Err(format!("a call to {callee} passes {} continuations", continuations.len()));
        }
        if arguments.len() > REGISTER_VARS + OVERFLOW_VARS {
            return Err(format!("a call passes {} variables, more than can be passed", arguments.len()));
        }
        let arguments = arguments.into_iter().map(Self::data).collect::<Result<Vec<_>, _>>()?;
        // what does not fit in registers goes where the callee will look
        for (slot, (block, word)) in arguments.iter().skip(REGISTER_VARS).enumerate() {
            writeln!(f, "  store ptr {block}, ptr {}", overflow_slot(slot, false)).unwrap();
            writeln!(f, "  store i64 {word}, ptr {}", overflow_slot(slot, true)).unwrap();
        }
        let arguments = Self::call_arguments(env, &arguments);
        let r = self.fresh("r");
        match continuations.first() {
            Some(k @ Val::Local(_)) => {
                let (block, word) = (self.fresh("r"), self.fresh("r"));
                writeln!(f, "  {r} = call {CONVENTION} {VALUE} {callee}({arguments})").unwrap();
                writeln!(f, "  {block} = extractvalue {VALUE} {r}, 0").unwrap();
                writeln!(f, "  {word} = extractvalue {VALUE} {r}, 1").unwrap();
                self.resume(f, k, &block, &word)
            }
            // (a callee that takes no continuation never returns)
            _ => {
                writeln!(f, "  {r} = musttail call {CONVENTION} {VALUE} {callee}({arguments})").unwrap();
                writeln!(f, "  ret {VALUE} {r}").unwrap();
                Ok(())
            }
        }
    }

    fn stmt(&mut self, f: &mut String, s: &'p Stmt, mut env: Env) -> Result<(), String> {
        match s {
            // a renaming, and the reference counts that go with it
            Stmt::Substitute(pairs, rest) => {
                for (name, v) in &env {
                    match (pairs.iter().filter(|(_, old)| old == name).count(), v) {
                        (0, _) => Self::erase(f, v),
                        (1, _) | (_, Val::Data { external: true, .. }) => {}
                        (n, Val::Data { block, .. }) => writeln!(f, "  call void @idyll_share(ptr {block}, i64 {})", n - 1).unwrap(),
                        (_, Val::Return | Val::Local(_)) => return Err(format!("the continuation {name} is used twice")),
                    }
                }
                let renamed = pairs
                    .iter()
                    .map(|(new, old)| Ok((new.clone(), Self::lookup(&env, old)?.clone())))
                    .collect::<Result<Env, String>>()?;
                self.stmt(f, rest, renamed)
            }
            Stmt::Let(v, m, fields, rest) => {
                let fields = Self::take(&mut env, fields)?;
                let block = self.store_fields(f, &fields)?;
                env.push((v.clone(), Val::Data { block, word: m.tag().to_string(), external: false }));
                self.stmt(f, rest, env)
            }
            Stmt::New(v, captured, clauses, rest) => {
                let [(symbol, branch)] = clauses.as_slice() else {
                    return Err("a consumer with several clauses needs a table of them, which is not implemented".into());
                };
                let closure = Self::take(&mut env, captured)?;
                self.next += 1;
                let id = self.next;
                // a function is a block and its code, as in the paper
                if *symbol != Symbol::Ret {
                    let block = self.store_fields(f, &closure)?;
                    let name = format!("clause.{id}");
                    let word = format!("ptrtoint (ptr @\"{name}\" to i64)");
                    self.pending.push(Pending { name, captured, branch });
                    env.push((v.clone(), Val::Data { block, word, external: false }));
                    return self.stmt(f, rest, env);
                }
                // a continuation is the rest of *this* function: a basic
                // block, entered with the result in a stack slot
                let [result] = branch.params.as_slice() else { return Err("a continuation takes one result".into()) };
                writeln!(self.allocas, "  %k{id}.b = alloca ptr\n  %k{id}.w = alloca i64").unwrap();
                let frame = Rc::new(Frame { id, captured: closure });
                env.push((v.clone(), Val::Local(frame.clone())));
                self.stmt(f, rest, env)?;
                let (block, word) = (self.fresh("r"), self.fresh("r"));
                writeln!(f, "kont{id}:").unwrap();
                writeln!(f, "  {block} = load ptr, ptr %k{id}.b").unwrap();
                writeln!(f, "  {word} = load i64, ptr %k{id}.w").unwrap();
                let mut inner: Env = vec![(result.clone(), Val::Data { block, word, external: false })];
                inner.extend(captured.iter().cloned().zip(frame.captured.iter().cloned()));
                self.stmt(f, &branch.body, inner)
            }
            Stmt::Switch(v, clauses) => {
                let scrutinee = Self::take(&mut env, std::slice::from_ref(v))?.remove(0);
                let (scrutinee_block, scrutinee_word) = Self::data(&scrutinee)?;
                let labels: Vec<String> = clauses.iter().map(|_| self.fresh("alt").replace('%', "")).collect();
                match labels.as_slice() {
                    [only] => writeln!(f, "  br label %{only}").unwrap(),
                    _ => {
                        let targets: Vec<String> =
                            clauses.iter().zip(&labels).map(|((m, _), l)| format!("i64 {}, label %{l}", m.tag())).collect();
                        let unreachable = self.fresh("none").replace('%', "");
                        writeln!(f, "  switch i64 {scrutinee_word}, label %{unreachable} [ {} ]", targets.join(" ")).unwrap();
                        writeln!(f, "{unreachable}:\n  unreachable").unwrap();
                    }
                }
                for ((_, branch), label) in clauses.iter().zip(&labels) {
                    writeln!(f, "{label}:").unwrap();
                    let mut inner = env.clone();
                    if !branch.params.is_empty() {
                        let fields = self.load_fields(f, scrutinee_block, branch.params.len());
                        writeln!(f, "  call void @idyll_release(ptr {scrutinee_block})").unwrap();
                        inner.extend(branch.params.iter().cloned().zip(fields));
                    }
                    self.stmt(f, &branch.body, inner)?;
                }
                Ok(())
            }
            Stmt::Invoke(v, m, args) => {
                let receiver = Self::take(&mut env, std::slice::from_ref(v))?.remove(0);
                let values = Self::take(&mut env, args)?;
                if !env.is_empty() {
                    return Err(format!("invoke {v} leaves variables behind"));
                }
                match (m, values.as_slice()) {
                    // a return
                    (Symbol::Ret, [result]) => {
                        let (block, word) = Self::data(result)?;
                        self.resume(f, &receiver, block, word)
                    }
                    // a call, of a function known only when it is made
                    (Symbol::Apply, _) => {
                        let (block, word) = Self::data(&receiver)?;
                        let code = self.fresh("code");
                        writeln!(f, "  {code} = inttoptr i64 {word} to ptr").unwrap();
                        self.call(f, &code, block, &values)
                    }
                    _ => Err(format!("invoke {v} {m} is neither a call nor a return")),
                }
            }
            // a call, of a function known now
            Stmt::Jump(l, args) => {
                let values = Self::take(&mut env, args)?;
                if !env.is_empty() {
                    return Err(format!("jump {l} leaves variables behind"));
                }
                self.call(f, &format!("@\"L.{l}\""), "undef", &values)
            }
            Stmt::Extern(op, args, branches) => {
                let words: Vec<String> =
                    args.iter().map(|a| Ok(Self::data(Self::lookup(&env, a)?)?.1.to_string())).collect::<Result<_, String>>()?;
                let binary = |this: &mut Self, f: &mut String, instruction: &str| {
                    let r = this.fresh("r");
                    writeln!(f, "  {r} = {instruction} i64 {}, {}", words[0], words[1]).unwrap();
                    r
                };
                let compare = |this: &mut Self, f: &mut String, condition: &str| {
                    let (c, r) = (this.fresh("c"), this.fresh("r"));
                    writeln!(f, "  {c} = icmp {condition} i64 {}, {}", words[0], words[1]).unwrap();
                    // `False` is tag 0 and `True` tag 1
                    writeln!(f, "  {r} = zext i1 {c} to i64").unwrap();
                    r
                };
                let word = match op {
                    Extern::LitInt(n) => i64::try_from(n).map_err(|_| format!("the literal {n} does not fit in 64 bits"))?.to_string(),
                    Extern::LitStr(s) => {
                        self.strings.push(s.clone());
                        format!("ptrtoint (ptr @str.{} to i64)", self.strings.len() - 1)
                    }
                    Extern::Add => binary(self, f, "add"),
                    Extern::Sub => binary(self, f, "sub"),
                    Extern::Mul => binary(self, f, "mul"),
                    Extern::Lt => compare(self, f, "slt"),
                    Extern::Eq => compare(self, f, "eq"),
                    // the end of the program, wherever on the stack it is
                    Extern::Halt => {
                        let answer = &words[0];
                        match self.result {
                            ResultKind::Int => writeln!(f, "  call void @idyll_print_int(i64 {answer})").unwrap(),
                            ResultKind::Bool => writeln!(f, "  call void @idyll_print_bool(i64 {answer})").unwrap(),
                            ResultKind::Unit => f.push_str("  call void @idyll_print_unit()\n"),
                            ResultKind::Str => {
                                let s = self.fresh("s");
                                writeln!(f, "  {s} = inttoptr i64 {answer} to ptr").unwrap();
                                writeln!(f, "  call void @idyll_print_string(ptr {s})").unwrap();
                            }
                            ResultKind::Other => f.push_str("  call void @idyll_print_opaque()\n"),
                        }
                        f.push_str("  call void @exit(i32 0)\n  unreachable\n");
                        return Ok(());
                    }
                };
                let [branch] = branches.as_slice() else { return Err(format!("extern {op} continues one way")) };
                let [result] = branch.params.as_slice() else { return Err(format!("extern {op} binds one variable")) };
                env.push((result.clone(), Val::Data { block: "null".into(), word, external: true }));
                self.stmt(f, &branch.body, env)
            }
        }
    }
}
