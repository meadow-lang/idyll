//! A checked Idyll program, lowered to Cut.
//!
//! The elaborated program is Core: the top-level definitions as `let`s
//! around the program's expression, and the recursive ones apart.  Each
//! top-level definition becomes a `def` if it is a function and a `val` if
//! not; the expression becomes the entry, a `def` with nothing but its
//! continuation.
//!
//! Terms are lowered in continuation-passing style: a term and the consumer
//! its value goes to make a statement, and an argument is computed into a
//! variable first, so that arguments are computed left to right, before the
//! call, as Idyll's are.  What erasure promised is kept: an argument of grade
//! `0` is never computed, and a binder of grade `0` is no parameter, except
//! that a type parameter becomes a descriptor parameter, and a type argument
//! a descriptor, which is how a function generic in a type is told how values
//! of it are represented.  Every binder's representation comes from its type,
//! by the checker's [`Typer`].
//!
//! A recursive definition inside another is lifted out to a `def` of its
//! own, since Cut has no local recursion (see [`Lower::lift`]): what it uses
//! of where it was defined becomes its first parameters, and every use of
//! it passes them.
//!
//! The program's answer is written out as the REPL writes it, since the
//! runtimes print only strings: see [`show`].
//!
//! A native the runtime has no primitive for is a `def` of the program's
//! own: see [`natives`].

mod natives;
mod show;

use super::*;
use idyllic_ast::Lit;
use idyllic_builtins::{Builtin, Native, NumOp, NumTy, Prim};
use idyllic_typecheck::natives::NativeInfo;
use idyllic_typecheck::core::{Core, Head, Note};
use idyllic_typecheck::grade::Grade;
use idyllic_typecheck::typer::{self, Scope, Typer};
use idyllic_typecheck::{RecDef, TypeCheckResult};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::rc::Rc;

/// The runtime's operation for each world operation of the prelude's that
/// has one of the same shape.
const NATIVE: &[(&str, &str)] = &[
    ("putStr", "Console.writeOutput"),
    ("putErr", "Console.writeError"),
    ("readLine", "Console.readLine"),
    ("readToString", "Fs.readToString"),
    ("writeString", "Fs.writeString"),
    ("appendString", "Fs.appendString"),
    ("removeFile", "Fs.removeFile"),
    ("createDir", "Fs.createDir"),
    ("createDirAll", "Fs.createDirAll"),
    ("removeDir", "Fs.removeDir"),
    ("removeDirAll", "Fs.removeDirAll"),
    ("rename", "Fs.rename"),
    ("copy", "Fs.copy"),
    ("listDir", "Fs.readDir"),
    ("exists", "Fs.exists"),
    ("isFile", "Fs.isFile"),
    ("isDir", "Fs.isDir"),
    ("exit", "Process.exit"),
    ("currentPid", "Process.currentPid"),
    ("argv", "Process.argv"),
    ("getEnv", "Process.getEnv"),
    ("setEnv", "Process.setEnv"),
    ("removeEnv", "Process.removeEnv"),
    ("isTerminal", "Process.isTerminal"),
    ("nextInt", "Random.nextInt"),
    ("intBetween", "Random.intBetween"),
    ("nextSeed", "Random.nextSeed"),
    ("now", "Time.now"),
    ("monotonic", "Time.monotonic"),
    ("sleep", "Time.sleep"),
    ("fail", "Test.fail"),
];

/// The roles the prelude's types play, by their paths there.
const ROLES: &[(&str, &str)] = &[
    ("none", "Maybe.Nothing"),
    ("some", "Maybe.Just"),
    ("err", "Result.Err"),
    ("ok", "Result.Ok"),
    ("nil", "List.Nil"),
    ("cons", "List.::"),
];

/// What a parameter of a function becomes.
#[derive(Clone, Debug, PartialEq)]
enum Param {
    /// a value, with its representation
    Value(Rep),
    /// a descriptor, for a type parameter
    Desc,
    /// a record with fields that are types (a trait's dictionary with
    /// associated types): the record, with its representation, and then a
    /// descriptor for each of those fields, of the constructor's fields at
    /// these places (see `Typer::type_fields`)
    Dict(Rep, usize, Vec<usize>),
    /// nothing: erased
    Skip,
}

/// What a variable of Core's context is, in Cut.
#[derive(Clone, Debug)]
enum Slot {
    Var(String),
    /// a type parameter: the descriptor variable carrying it, if there is
    /// one, and the representation variable it is
    TypeVar(Option<String>, String),
    /// nothing at run time
    Erased,
    /// a top-level value
    Val(Symbol),
    /// a top-level function, with its parameters
    Def(Symbol, Rc<Vec<Param>>),
    Prim(Prim),
    /// a handler clause's resumption
    Resume(String),
}

#[derive(Clone)]
struct Env {
    slots: Vec<Slot>,
    scope: Scope,
    /// the fields that are types of the records bound at these levels: the
    /// variable at that level, the field's place, and the descriptor
    /// variable passed for it, if there is one, and the representation
    /// variable it is (`typer::Rep::Field`)
    fields: Vec<(usize, usize, Option<String>, String)>,
}

struct Lower<'t> {
    typer: &'t mut Typer,
    symbols: &'t HashMap<usize, (String, String)>,
    ctors: &'t HashMap<usize, idyllic_typecheck::CtorInfo>,
    ops: &'t HashMap<usize, idyllic_typecheck::handlers::OpInfo>,
    natives: &'t HashMap<usize, NativeInfo>,
    recs: HashMap<usize, RecDef>,
    /// each recursive definition's parameters, worked out when first needed
    rec_params: HashMap<usize, Rc<Vec<Param>>>,
    /// each recursive definition inside another: the level the top-level
    /// scope it is in reaches, and the levels from there to its own that
    /// it uses (see [`local_free_levels`])
    local: HashMap<usize, (usize, Uses)>,
    /// each such definition once lifted: its symbol, its parameters (what
    /// it captures first), and the levels it captures
    lifted: HashMap<usize, (Symbol, Rc<Vec<Param>>, Vec<usize>)>,
    package: String,
    /// the unique of each top-level `let`
    toplevel: Vec<usize>,
    /// the scope at each level the top-level `let`s reach
    levels: Vec<Env>,
    fresh: usize,
    defs: Vec<Def>,
    vals: Vec<Val>,
    datas: BTreeSet<usize>,
    effects: BTreeSet<usize>,
    performed: BTreeSet<usize>,
    errors: Vec<String>,
    /// what shows each type the answer has in it (see [`show`])
    shows: HashMap<String, Option<show::ShowDefs>>,
    /// an operation the entry handles itself (`abort`), which so needs no
    /// runtime operation
    handled_by_entry: Option<usize>,
    /// the natives with no primitive whose `def`s have been made
    natives_made: std::collections::HashSet<Native>,
    /// the types whose `#atom` has been made
    atoms: std::collections::HashSet<String>,
    show_defs: Vec<Def>,
    /// the prelude's `showInt`, kept for showing an answer
    show_int: Option<Symbol>,
}

/// `c` with each solved metavariable replaced by its solution (see
/// [`Core::zonked`]).
fn zonk(solutions: &[Option<Rc<Core>>], c: &Rc<Core>) -> Rc<Core> {
    c.zonked(solutions)
}

/// Lower a checked (and linked) program, whose own package is `package`.

pub fn lower(tc: &mut TypeCheckResult, package: &str) -> Result<Program, Vec<String>> {
    lower_with(tc, package, None)
}

/// The same, with `mark` written in front of the answer, so that whoever
/// runs it can tell the answer from what the program printed before it:
/// Meadow writes the answer right after.
pub fn lower_marked(tc: &mut TypeCheckResult, package: &str, mark: &str) -> Result<Program, Vec<String>> {
    lower_with(tc, package, Some(mark))
}

fn lower_with(tc: &mut TypeCheckResult, package: &str, mark: Option<&str>) -> Result<Program, Vec<String>> {
    // an answer is shown with the prelude's `showInt`, so it stays
    let show_int = tc.symbols.iter().find(|(_, (p, path))| p == "Prelude" && path == "showInt").map(|(u, _)| *u);
    if !tc.ran_main {
        tc.roots.extend(show_int);
    }
    // a hole is lowered as what it was solved with: first, so that what
    // only a solution uses is kept
    let solutions = tc.meta_solutions.clone();
    tc.program = tc.program.as_ref().map(|p| zonk(&solutions, p));
    for d in &mut tc.rec_defs {
        d.core = zonk(&solutions, &d.core);
    }
    // what the program never uses (most of the prelude, say) is left out
    let Some((program, live)) = idyllic_typecheck::prune::prune(tc) else {
        return Err(vec!["there is no program: it only declares".into()]);
    };
    let recs: HashMap<usize, RecDef> =
        tc.rec_defs.iter().filter(|d| live.contains(&d.unique)).map(|d| (d.unique, d.clone())).collect();
    let toplevel = tc.toplevel.clone();
    let (ran_main, body_type) = (tc.ran_main, tc.body_type.clone());
    let mut l = Lower {
        typer: &mut tc.typer,
        symbols: &tc.symbols,
        ctors: &tc.ctors,
        ops: &tc.ops,
        natives: &tc.natives,
        recs,
        rec_params: HashMap::new(),
        local: HashMap::new(),
        lifted: HashMap::new(),
        package: package.to_string(),
        toplevel: toplevel.clone(),
        levels: Vec::new(),
        fresh: 0,
        defs: Vec::new(),
        vals: Vec::new(),
        datas: BTreeSet::new(),
        effects: BTreeSet::new(),
        performed: BTreeSet::new(),
        errors: Vec::new(),
        shows: HashMap::new(),
        atoms: std::collections::HashSet::new(),
        natives_made: std::collections::HashSet::new(),
        handled_by_entry: None,
        show_defs: Vec::new(),
        show_int: None,
    };
    l.show_int = show_int.and_then(|u| l.symbol(u));
    // the context every program starts in: the builtins, then the primitives
    let mut env = Env { slots: Vec::new(), scope: l.typer.scope(), fields: Vec::new() };
    env.slots.extend(Builtin::ALL.iter().map(|_| Slot::Erased));
    env.slots.extend(Prim::ALL.iter().map(|p| Slot::Prim(*p)));
    // the top-level definitions, and the scope at each level they reach,
    // which is where recursive definitions are; the ones defined inside
    // another are lifted, where they are first used
    let base = env.slots.len();
    l.local = local_free_levels(&l.recs, &owners(&program, toplevel.len(), base, &l.recs));
    l.levels.push(env.clone());
    let mut c = program.clone();
    let mut i = 0;
    // (only the top-level definitions' `let`s: what comes after them is the
    // expression's own)
    while let (Core::Let(x, rhs, body), true) = (&*c, i < toplevel.len()) {
        let symbol = l.symbol(toplevel[i]).unwrap_or_else(|| Symbol::idyll(package, x));
        let slot = l.toplevel(&env, &symbol, rhs);
        env.scope = l.typer.define(&env.scope, x, rhs);
        env.slots.push(slot);
        l.levels.push(env.clone());
        c = body.clone();
        i += 1;
    }
    // in the order they were named, so that the same program is always the
    // same Cut (a build is known by what it is made of)
    let mut recs: Vec<(usize, RecDef)> = l.recs.clone().into_iter().collect();
    recs.sort_by_key(|(u, _)| *u);
    for (u, d) in recs {
        // one inside another is lifted where it is used, if it is
        if d.local {
            continue;
        }
        let Some(at) = l.levels.iter().find(|e| e.slots.len() == d.lvl).cloned() else {
            l.errors.push(format!("{} is a recursive definition the lowering cannot place", d.name));
            continue;
        };
        let symbol = l.symbol(u).unwrap_or_else(|| Symbol::idyll(package, &d.name));
        let def = l.def(&symbol, &at, &d.core);
        l.defs.push(def);
    }
    // the entry: the program's expression, written out as the REPL writes
    // it (see [`show`]), given to `halt`
    let k = l.name("k");
    let shown = match &body_type {
        Some(ty) if !ran_main => {
            let ty = l.typer.eval_type(&env.scope, ty);
            let k_answer = match mark {
                Some(mark) => {
                    let s = l.name("s");
                    let marked = Statement::Prim(
                        CONCAT.into(),
                        prim_args(CONCAT, vec![Producer::Str(mark.to_string()), Producer::Var(s.clone())]),
                        vec![Consumer::Var(k.clone())],
                    );
                    Consumer::MuTilde(Binder { name: s, rep: Rep::Str }, Box::new(marked))
                }
                None => Consumer::Var(k.clone()),
            };
            l.show_answer(&env.scope, &ty, k_answer)
        }
        _ => None,
    };
    let (answer, k_entry) = match shown {
        Some((_, shown)) => (Answer::Str, shown),
        None => (Answer::None, Consumer::Var(k.clone())),
    };
    if answer == Answer::Str {
        let shows = std::mem::take(&mut l.show_defs);
        l.defs.extend(shows);
    }
    let entry = Symbol::idyll(package, "#entry");
    // `abort`, which no runtime operation does, given up on in the program's
    // own code: a handler around everything, as Idyll's world does it
    let abort = l.ops.iter().find(|(u, op)| op.name == "abort" && l.symbols.get(u).is_some_and(|(p, _)| p == "Prelude")).map(|(u, _)| *u);
    let aborts = abort.filter(|u| mentions(&c, *u) || l.recs.values().any(|d| mentions(&d.core, *u)) || program_lets_mention(&program, toplevel.len(), *u));
    let body = match aborts {
        Some(abort) => {
            let b = l.name("b");
            let body = l.term(&env, &c, Consumer::Var(b.clone()));
            let rep = l.rep_of(&env, &c);
            l.handled_by_entry = Some(abort);
            l.abort_handler(abort, b, body, rep, k_entry)
        }
        None => l.term(&env, &c, k_entry),
    };
    l.defs.push(Def { symbol: entry.clone(), rep_vars: Vec::new(), effect_vars: Vec::new(), params: Vec::new(), conts: vec![k], body });
    let out = l.declarations(Some(entry), answer);
    if l.errors.is_empty() {
        Ok(out)
    } else {
        Err(l.errors)
    }
}

/// Whether `c` mentions the constant `u`.
fn mentions(c: &Core, u: usize) -> bool {
    match c {
        Core::Opaque(v, _) => *v == u,
        Core::Pi { grade, in_type, dom, cod, .. } => [grade, in_type, dom, cod].iter().any(|c| mentions(c, u)),
        Core::Lam(_, b) | Core::BoxIntro(b) | Core::Unbox(b) | Core::Runtime(_, b) => mentions(b, u),
        Core::App(a, b)
        | Core::BoxTy(a, b)
        | Core::Let(_, a, b)
        | Core::GradeAdd(a, b)
        | Core::GradeMul(a, b)
        | Core::GradeRange(a, b)
        | Core::Comp(a, b) => mentions(a, u) || mentions(b, u),
        Core::Case(s, alts) => mentions(s, u) || alts.iter().any(|a| mentions(&a.body, u)),
        Core::Row(a, b) => a.iter().chain(b).any(|c| mentions(c, u)),
        Core::Handle(b, r, ops) => mentions(b, u) || mentions(r, u) || ops.iter().any(|o| mentions(&o.body, u)),
        _ => false,
    }
}

/// Whether one of the program's first `n` (top-level) definitions mentions
/// `u`.
fn program_lets_mention(program: &Rc<Core>, n: usize, u: usize) -> bool {
    let mut at = program;
    for _ in 0..n {
        let Core::Let(_, rhs, rest) = &**at else { return false };
        if mentions(rhs, u) {
            return true;
        }
        at = rest;
    }
    false
}

impl Lower<'_> {
    /// `body`, sending its value to `b`, run under a handler for the
    /// prelude's `Abort` that does what Idyll's world does: the message on
    /// standard error, then exit status 1.
    fn abort_handler(&mut self, abort: usize, b: String, body: Statement, rep: Rep, k: Consumer) -> Statement {
        let prelude_op = |l: &Self, name: &str| {
            l.ops.iter().find(|(u, op)| op.name == name && l.symbols.get(u).is_some_and(|(p, _)| p == "Prelude")).map(|(u, op)| (*u, op.effect))
        };
        let (Some((put_err, console)), Some((exit, process))) = (prelude_op(self, "putErr"), prelude_op(self, "exit")) else {
            return body;
        };
        for (op, effect) in [(put_err, console), (exit, process), (abort, self.ops[&abort].effect)] {
            self.performed.insert(op);
            self.effects.insert(effect);
        }
        let (Some(abort_s), Some(put_err_s), Some(exit_s)) = (self.symbol(abort), self.symbol(put_err), self.symbol(exit)) else {
            return body;
        };
        let (message, line, r, kc, u, w) = (self.name("message"), self.name("line"), self.name("r"), self.name("k"), self.name("u"), self.name("u"));
        let then_exit = Statement::Perform(
            exit_s,
            vec![Producer::Int(1)],
            Consumer::MuTilde(Binder { name: w.clone(), rep: Rep::Unit }, Box::new(Statement::Cut(Producer::Var(w), Consumer::Var(kc.clone())))),
        );
        let clause_body = Statement::Prim(
            CONCAT.into(),
            prim_args(CONCAT, vec![Producer::Var(message.clone()), Producer::Str("\n".into())]),
            vec![Consumer::MuTilde(
                Binder { name: line.clone(), rep: Rep::Str },
                Box::new(Statement::Perform(
                    put_err_s,
                    vec![Producer::Var(line)],
                    Consumer::MuTilde(Binder { name: u, rep: Rep::Unit }, Box::new(then_exit)),
                )),
            )],
        );
        let clause = Clause { op: abort_s, params: vec![Binder { name: message, rep: Rep::Str }], resumption: r, cont: kc, body: clause_body };
        let (x, kr) = (self.name("x"), self.name("k"));
        let ret = (Binder { name: x.clone(), rep }, kr.clone(), Statement::Cut(Producer::Var(x), Consumer::Var(kr)));
        Statement::Handle(Box::new(Handle { clauses: vec![clause], ret, body_cont: b, body, cont: k }))
    }

    fn name(&mut self, hint: &str) -> String {
        self.fresh += 1;
        let clean: String = hint.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        let clean = if clean.starts_with(|c: char| c.is_ascii_alphabetic()) { clean } else { format!("v{clean}") };
        format!("{clean}_{}", self.fresh)
    }

    fn symbol(&self, u: usize) -> Option<Symbol> {
        self.symbols.get(&u).map(|(p, path)| Symbol::idyll(p, path))
    }

    /// A representation the typer gave, in terms of `env`'s variables.
    fn rep(&self, env: &Env, r: typer::Rep) -> Rep {
        match r {
            typer::Rep::I64 => Rep::I64,
            typer::Rep::Word(w) => Rep::Word(w),
            typer::Rep::F64 => Rep::F64,
            typer::Rep::F32 => Rep::F32,
            typer::Rep::Bool => Rep::Bool,
            typer::Rep::Unit | typer::Rep::Erased => Rep::Unit,
            typer::Rep::Str => Rep::Str,
            typer::Rep::Char => Rep::Char,
            typer::Rep::Ptr => Rep::Ptr,
            typer::Rep::Any => Rep::Any,
            typer::Rep::Desc => Rep::Desc,
            typer::Rep::Var(lvl) => match env.slots.get(lvl) {
                Some(Slot::TypeVar(Some(_), a)) => Rep::Var(a.clone()),
                _ => Rep::Any,
            },
            typer::Rep::Field(lvl, i) => match env.fields.iter().find(|f| f.0 == lvl && f.1 == i) {
                Some((_, _, Some(_), a)) => Rep::Var(a.clone()),
                _ => Rep::Any,
            },
        }
    }

    /// The representation of a term's value.
    fn rep_of(&mut self, env: &Env, c: &Rc<Core>) -> Rep {
        let r = self.typer.rep_of(&env.scope, c);
        self.rep(env, r)
    }

    /// A top-level definition: a `def` for a function, a `val` otherwise.
    fn toplevel(&mut self, env: &Env, symbol: &Symbol, rhs: &Rc<Core>) -> Slot {
        // a type, a row or a type family is nothing at run time
        if self.typer.is_type_former(&env.scope, rhs) {
            return Slot::Erased;
        }
        if lambda(rhs).is_some() {
            let params = Rc::new(self.params_of(env, rhs));
            let def = self.def(symbol, env, rhs);
            self.defs.push(def);
            Slot::Def(symbol.clone(), params)
        } else {
            let rep = self.rep_of(env, rhs);
            let body = self.term(env, rhs, Consumer::Halt);
            self.vals.push(Val { symbol: symbol.clone(), rep, body });
            Slot::Val(symbol.clone())
        }
    }

    /// What each of a function's leading lambdas binds, in `env`.
    fn params_of(&mut self, env: &Env, c: &Rc<Core>) -> Vec<Param> {
        let mut env = env.clone();
        let mut out = Vec::new();
        let mut c = c.clone();
        while let Some((x, grade, ty, body)) = lambda(&c) {
            let (mut params, mut rep_vars) = (Vec::new(), Vec::new());
            let lvl = env.slots.len();
            self.bind_param(&mut env, &x, grade, ty.as_ref(), &mut params, &mut rep_vars);
            let fields: Vec<usize> = env.fields.iter().filter(|f| f.0 == lvl).map(|f| f.1).collect();
            out.push(match (params.first(), rep_vars.is_empty()) {
                (Some(b), _) if !fields.is_empty() => {
                    let ty = self.typer.var_type(&env.scope, lvl);
                    let ctor = self.typer.record_type_fields(&ty).map_or(0, |(c, _)| c);
                    Param::Dict(b.rep.clone(), ctor, fields)
                }
                (Some(_), false) => Param::Desc,
                (Some(b), true) => Param::Value(b.rep.clone()),
                (None, _) => Param::Skip,
            });
            c = body;
        }
        out
    }

    /// A recursive definition's parameters, in the scope it is defined in.
    fn rec_params(&mut self, u: usize) -> Option<Rc<Vec<Param>>> {
        if let Some(p) = self.rec_params.get(&u) {
            return Some(p.clone());
        }
        let d = self.recs.get(&u)?.clone();
        let at = self.levels.iter().find(|e| e.slots.len() == d.lvl)?.clone();
        let params = Rc::new(self.params_of(&at, &d.core));
        self.rec_params.insert(u, params.clone());
        Some(params)
    }

    /// A recursive definition inside another, as a `def` of its own: lifted
    /// the first time it is used, from `env`, the scope of the use.  Cut has
    /// no local recursion: a closure that holds itself is a cycle reference
    /// counting cannot free.  So the variables of the enclosing definition
    /// that it uses (which a use sees at the same levels as the definition
    /// did) become its first parameters: a value as a value, a type as its
    /// descriptor.  The `def` is named under the top-level definition it
    /// was in (`Main/sumTo.go`).
    fn lift(&mut self, u: usize, env: &Env) -> Option<(Symbol, Rc<Vec<Param>>, Vec<usize>)> {
        if let Some(done) = self.lifted.get(&u) {
            return Some(done.clone());
        }
        let d = self.recs.get(&u)?.clone();
        let (top, uses) = self.local.get(&u)?.clone();
        let Some(base) = self.levels.iter().find(|e| e.slots.len() == top).cloned() else {
            self.errors.push(format!("{} is defined where the lowering cannot place it", d.name));
            return None;
        };
        if env.slots.len() < d.lvl {
            self.errors.push(format!("{} is used outside where it is defined", d.name));
            return None;
        }
        // where it was defined, with only what it uses of it
        let mut at = Env { slots: base.slots.clone(), scope: env.scope.prefix(d.lvl), fields: base.fields.clone() };
        let (mut binders, mut rep_vars, mut params, mut captured) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for lvl in top..d.lvl {
            let slot = match &env.slots[lvl] {
                _ if !uses.all.contains(&lvl) => Slot::Erased,
                // a value it never computes with is not passed
                Slot::Var(_) if !uses.values.contains(&lvl) => Slot::Erased,
                Slot::Var(x) => {
                    let v = self.name(x.rsplit_once('_').map_or(x.as_str(), |(h, _)| h));
                    let ty = self.typer.var_type(&env.scope, lvl);
                    let r = self.typer.rep(&env.scope, &ty);
                    binders.push(Binder { name: v.clone(), rep: self.rep(&at, r) });
                    // a record with fields that are types: their descriptors too
                    let fields: Vec<(usize, Option<String>, String)> =
                        env.fields.iter().filter(|f| f.0 == lvl).map(|f| (f.1, f.2.clone(), f.3.clone())).collect();
                    match self.typer.record_type_fields(&ty) {
                        Some((ctor, _)) if !fields.is_empty() && fields.iter().all(|f| f.1.is_some()) => {
                            for (i, _, a) in &fields {
                                let hint = a.rsplit_once('_').map_or(a.as_str(), |(h, _)| h).to_string();
                                let (dv, av) = (self.name(&format!("d_{hint}")), self.name(&hint));
                                binders.push(Binder { name: dv.clone(), rep: Rep::Desc });
                                rep_vars.push((av.clone(), dv.clone()));
                                at.fields.push((lvl, *i, Some(dv), av));
                            }
                            params.push(Param::Dict(self.rep(&at, r), ctor, fields.iter().map(|f| f.0).collect()));
                        }
                        _ => params.push(Param::Value(self.rep(&at, r))),
                    }
                    captured.push(lvl);
                    Slot::Var(v)
                }
                Slot::TypeVar(Some(_), a) => {
                    let hint = a.rsplit_once('_').map_or(a.as_str(), |(h, _)| h).to_string();
                    let (dv, av) = (self.name(&format!("d_{hint}")), self.name(&hint));
                    binders.push(Binder { name: dv.clone(), rep: Rep::Desc });
                    rep_vars.push((av.clone(), dv.clone()));
                    params.push(Param::Desc);
                    captured.push(lvl);
                    Slot::TypeVar(Some(dv), av)
                }
                Slot::Resume(_) => {
                    self.errors.push(format!("{} uses a handler's resumption, which is not lowered yet", d.name));
                    Slot::Erased
                }
                _ => Slot::Erased,
            };
            at.slots.push(slot);
        }
        let enclosing = match self.levels.iter().position(|e| e.slots.len() == top).and_then(|i| self.toplevel.get(i)) {
            Some(t) => self.symbol(*t).unwrap_or_else(|| Symbol::idyll(&self.package, "#entry")),
            None => Symbol::idyll(&self.package, "#entry"),
        };
        let mut symbol = enclosing.child(&d.name);
        let mut n = 1;
        while self.lifted.values().any(|(s, ..)| *s == symbol) || self.defs.iter().any(|def| def.symbol == symbol) {
            n += 1;
            symbol = enclosing.child(&format!("{}#{n}", d.name));
        }
        params.extend(self.params_of(&at, &d.core));
        let params = Rc::new(params);
        // known before its body is lowered, which may call it
        self.lifted.insert(u, (symbol.clone(), params.clone(), captured.clone()));
        let mut def = self.def(&symbol, &at, &d.core);
        binders.append(&mut def.params);
        def.params = binders;
        rep_vars.append(&mut def.rep_vars);
        def.rep_vars = rep_vars;
        self.defs.push(def);
        Some((symbol, params, captured))
    }

    /// A function as a `def`: its leading lambdas are its parameters.
    fn def(&mut self, symbol: &Symbol, env: &Env, c: &Rc<Core>) -> Def {
        let mut env = env.clone();
        let (mut params, mut rep_vars) = (Vec::new(), Vec::new());
        let mut effect_vars = Vec::new();
        let mut c = c.clone();
        while let Some((x, grade, ty, body)) = lambda(&c) {
            // an effect variable: declared, `<e: Effect>`, and erased
            if let Some(many) = ty.as_ref().and_then(|t| self.typer.effect_binder_kind(&env.scope, t)) {
                effect_vars.push((self.name(&x), if many { Kind::ManyEffect } else { Kind::Effect }));
            }
            self.bind_param(&mut env, &x, grade, ty.as_ref(), &mut params, &mut rep_vars);
            c = body;
        }
        let k = self.name("k");
        let body = self.term(&env, &c, Consumer::Var(k.clone()));
        Def { symbol: symbol.clone(), rep_vars, effect_vars, params, conts: vec![k], body }
    }

    /// Bind a lambda's variable in `env`: a value parameter, a descriptor
    /// parameter for a type, or nothing for anything else erased.
    fn bind_param(
        &mut self,
        env: &mut Env,
        x: &str,
        grade: Option<Grade>,
        ty: Option<&Rc<Core>>,
        params: &mut Vec<Binder>,
        rep_vars: &mut Vec<(String, String)>,
    ) {
        let is_type = ty.is_some_and(|t| self.typer.is_type_binder(&env.scope, t));
        env.scope = match ty {
            Some(t) => self.typer.bind(&env.scope, x, t),
            None => self.typer.bind_unknown(&env.scope, x),
        };
        if is_type {
            let (d, a) = (self.name(&format!("d_{x}")), self.name(x));
            params.push(Binder { name: d.clone(), rep: Rep::Desc });
            rep_vars.push((a.clone(), d.clone()));
            env.slots.push(Slot::TypeVar(Some(d), a));
        } else if grade.is_some_and(|g| g.is_zero()) {
            env.slots.push(Slot::Erased);
        } else {
            let v = self.name(x);
            let lvl = env.scope.len() - 1;
            let ty = self.typer.var_type(&env.scope, lvl);
            let r = self.typer.rep(&env.scope, &ty);
            params.push(Binder { name: v.clone(), rep: self.rep(env, r) });
            env.slots.push(Slot::Var(v));
            // a record with fields that are types, a dictionary with
            // associated types: a descriptor for each, which the types of
            // what comes after may read (`Elem c`)
            if let Some((_, fields)) = self.typer.record_type_fields(&ty) {
                for (i, name) in fields {
                    let hint = format!("{x}_{name}");
                    let (d, a) = (self.name(&format!("d_{hint}")), self.name(&hint));
                    params.push(Binder { name: d.clone(), rep: Rep::Desc });
                    rep_vars.push((a.clone(), d.clone()));
                    env.fields.push((lvl, i, Some(d), a));
                }
            }
        }
    }

    /// A method's parameter (a closure's, or a handler clause's): as
    /// [`Self::bind_param`], but a method binds no representation variable
    /// of its own (`docs/CUT.md`: only a `def`'s signature does), so a type
    /// parameter's values are boxed, `any`, and what it is a descriptor of
    /// is passed on as `desc(any)`.
    fn bind_method_param(
        &mut self,
        env: &mut Env,
        x: &str,
        grade: Option<Grade>,
        ty: Option<&Rc<Core>>,
        params: &mut Vec<Binder>,
        rep_vars: &mut Vec<(String, String)>,
    ) {
        let before = rep_vars.len();
        let (params_before, lvl) = (params.len(), env.slots.len());
        self.bind_param(env, x, grade, ty, params, rep_vars);
        if rep_vars.len() > before {
            rep_vars.truncate(before);
            if let Some(Slot::TypeVar(_, a)) = env.slots.last().cloned() {
                *env.slots.last_mut().expect("just bound") = Slot::TypeVar(None, a);
            }
            // a record's descriptors are not passed to a method: what its
            // fields that are types hold is `any` there
            if env.fields.iter().any(|f| f.0 == lvl) {
                params.truncate(params_before + 1);
                for f in env.fields.iter_mut().filter(|f| f.0 == lvl) {
                    f.2 = None;
                }
            }
        }
    }

    /// `c`'s value, given to `k`.
    fn term(&mut self, env: &Env, c: &Rc<Core>, k: Consumer) -> Statement {
        match &**c {
            Core::Var(..) | Core::Lit(_) | Core::Prim(_) | Core::Opaque(..) | Core::App(..) => self.application(env, c, k),
            Core::Runtime(Note::Erased, _) => Statement::Cut(Producer::Unit, k),
            Core::Runtime(Note::Forced, inner) => match &**inner {
                Core::Case(_, alts) => match alts.first() {
                    // the one alternative its type allows, nothing looked at
                    Some(alt) => {
                        let mut inner_env = env.clone();
                        for x in &alt.vars {
                            inner_env.scope = self.typer.bind_unknown(&inner_env.scope, x);
                            inner_env.slots.push(Slot::Erased);
                        }
                        self.term(&inner_env, &alt.body, k)
                    }
                    None => Statement::Error("a case with no alternatives was reached".into()),
                },
                _ => self.term(env, inner, k),
            },
            Core::Runtime(Note::Binds { .. }, inner) if matches!(**inner, Core::Unbox(_)) => {
                let Core::Unbox(e) = &**inner else { unreachable!() };
                self.unbox(env, c, e, k)
            }
            Core::Runtime(_, inner) if lambda(c).is_none() => self.term(env, inner, k),
            Core::Runtime(..) | Core::Lam(..) => {
                let p = self.closure(env, c);
                Statement::Cut(p, k)
            }
            Core::Let(x, rhs, body) => {
                let v = self.name(x);
                let rep = self.rep_of(env, rhs);
                let mut inner = env.clone();
                inner.scope = self.typer.define(&env.scope, x, rhs);
                inner.slots.push(Slot::Var(v.clone()));
                let rest = self.term(&inner, body, k);
                self.term(env, rhs, Consumer::MuTilde(Binder { name: v, rep }, Box::new(rest)))
            }
            Core::Case(scrutinee, alts) => self.case(env, scrutinee, alts, k),
            Core::BoxIntro(e) => self.atom(env, e, &mut |_, p| Statement::Cut(Producer::Tuple(vec![p]), k.clone())),
            Core::Unbox(e) => self.unbox(env, c, e, k),
            Core::Handle(body, ret, clauses) => self.handle(env, body, ret, clauses, k),
            // a type, a grade or an effect is nothing at run time
            Core::Type(_)
            | Core::LitTy(_)
            | Core::Pi { .. }
            | Core::BoxTy(..)
            | Core::Comp(..)
            | Core::Row(..)
            | Core::GradeLit(_)
            | Core::GradeAdd(..)
            | Core::GradeMul(..)
            | Core::GradeRange(..) => Statement::Cut(Producer::Unit, k),
            Core::Meta(_) => {
                self.errors.push("a hole's solution is not lowered yet".into());
                Statement::Error("a hole".into())
            }
            Core::Error => Statement::Error("this does not type check".into()),
        }
    }

    /// `[x] = e`: the box's one field.
    fn unbox(&mut self, env: &Env, whole: &Rc<Core>, e: &Rc<Core>, k: Consumer) -> Statement {
        let x = self.name("x");
        let rep = self.rep_of(env, whole);
        let arm = Arm { ctor: None, fields: vec![Binder { name: x.clone(), rep }], body: Statement::Cut(Producer::Var(x), k) };
        self.term(env, e, Consumer::Case(vec![arm]))
    }

    /// `c`'s value as a producer, for `rest` to use: as it is if it is a
    /// value already, or computed into a variable first.
    fn atom(&mut self, env: &Env, c: &Rc<Core>, rest: &mut dyn FnMut(&mut Self, Producer) -> Statement) -> Statement {
        if let Some(p) = self.value(env, c) {
            return rest(self, p);
        }
        let v = self.name("v");
        let rep = self.rep_of(env, c);
        let body = rest(self, Producer::Var(v.clone()));
        self.term(env, c, Consumer::MuTilde(Binder { name: v, rep }, Box::new(body)))
    }

    /// Several terms' values, in order.
    fn atoms(&mut self, env: &Env, cs: &[Rc<Core>], rest: &mut dyn FnMut(&mut Self, Vec<Producer>) -> Statement) -> Statement {
        self.atoms_from(env, cs, Vec::new(), rest)
    }

    fn atoms_from(
        &mut self,
        env: &Env,
        cs: &[Rc<Core>],
        done: Vec<Producer>,
        rest: &mut dyn FnMut(&mut Self, Vec<Producer>) -> Statement,
    ) -> Statement {
        let Some((first, others)) = cs.split_first() else { return rest(self, done) };
        let env2 = env.clone();
        self.atom(env, first, &mut |this, p| {
            let mut done = done.clone();
            done.push(p);
            this.atoms_from(&env2, others, done, rest)
        })
    }

    /// A term that is a value without computing anything, as a producer.
    fn value(&mut self, env: &Env, c: &Rc<Core>) -> Option<Producer> {
        Some(match &**c {
            Core::Var(i, _) => match env.slots.get(env.slots.len().checked_sub(i + 1)?)? {
                Slot::Var(v) | Slot::Resume(v) => Producer::Var(v.clone()),
                Slot::Val(s) => Producer::Val(s.clone()),
                Slot::Erased | Slot::TypeVar(..) => Producer::Unit,
                Slot::Def(..) | Slot::Prim(_) => return None,
            },
            Core::Lit(Lit::Int(n)) => Producer::Int(*n),
            Core::Lit(Lit::Float(x)) => Producer::Float(f64::from_bits(*x)),
            Core::Lit(Lit::Str(s)) => Producer::Str(s.clone()),
            Core::Lit(Lit::Char(c)) => Producer::Char(*c),
            Core::Lit(Lit::Bool(b)) => Producer::Bool(*b),
            Core::Lit(Lit::Unit) => Producer::Unit,
            Core::Runtime(Note::Erased, _) => Producer::Unit,
            _ if lambda(c).is_some() => self.closure(env, c),
            _ => return None,
        })
    }

    /// A lambda as codata: an object whose `apply` takes its argument.  A
    /// type parameter's is a descriptor; one erased otherwise is no method
    /// at all.
    fn closure(&mut self, env: &Env, c: &Rc<Core>) -> Producer {
        let Some((x, grade, ty, body)) = lambda(c) else { unreachable!("a closure is a lambda") };
        let mut inner = env.clone();
        let (mut params, mut rep_vars) = (Vec::new(), Vec::new());
        self.bind_method_param(&mut inner, &x, grade, ty.as_ref(), &mut params, &mut rep_vars);
        if params.is_empty() {
            // erased: what it computes is the body's value itself
            return match self.value(&inner, &body) {
                Some(p) => p,
                None => {
                    let k = self.name("k");
                    Producer::Mu(k.clone(), Box::new(self.term(&inner, &body, Consumer::Var(k))))
                }
            };
        }
        let k = self.name("k");
        let s = self.term(&inner, &body, Consumer::Var(k.clone()));
        Producer::Cocase(vec![Method { name: "apply".into(), params, conts: vec![k], body: s }])
    }

    /// An application, or a head applied to nothing: a primitive, a
    /// constructor, an operation, a top-level function, or a value applied
    /// one argument at a time.
    fn application(&mut self, env: &Env, c: &Rc<Core>, k: Consumer) -> Statement {
        // Cut writes no literal of a sized integer or a `Float32`: an `Int`
        // or a `Float`, converted
        match &**c {
            Core::Lit(Lit::Word(w, b)) => {
                let op = NumOp::FromInt.prim(NumTy::Word(*w));
                return Statement::Prim(op, vec![Producer::Int(w.value(*b) as i64)], vec![k]);
            }
            Core::Lit(Lit::Float32(x)) => {
                return Statement::Prim("toFloat32".into(), vec![Producer::Float(f32::from_bits(*x).into())], vec![k]);
            }
            _ => {}
        }
        let mut args = Vec::new();
        let mut head = c.clone();
        while let Core::App(f, a) = &*head {
            args.push(a.clone());
            head = f.clone();
        }
        args.reverse();
        // (a lambda read back from a value, its binders' types from where
        // it is passed)
        let args = self.typer.annotate_args(&env.scope, &head, &args);
        match &*head {
            Core::Prim(p) => return self.prim(env, c, *p, &args, k),
            Core::Var(i, _) => {
                let slot = env.slots.get(env.slots.len().saturating_sub(i + 1)).cloned();
                match slot {
                    Some(Slot::Prim(p)) => return self.prim(env, c, p, &args, k),
                    Some(Slot::Def(s, params)) => return self.call(env, c, s, &params, &args, k),
                    Some(Slot::Resume(r)) if args.len() == 1 => {
                        return self.atom(env, &args[0], &mut |_, v| {
                            Statement::Cut(Producer::Var(r.clone()), Consumer::Method("resume".into(), vec![v], vec![k.clone()]))
                        });
                    }
                    _ => {}
                }
            }
            Core::Opaque(u, name) => {
                if let Some(info) = self.ctors.get(u).cloned() {
                    return self.construct(env, c, *u, &info, &args, k);
                }
                if let Some(info) = self.ops.get(u).cloned() {
                    return self.perform(env, c, *u, &info, &args, k);
                }
                if let Some(info) = self.natives.get(u).copied() {
                    return self.native(env, c, info, &args, k);
                }
                if self.local.contains_key(u) {
                    let Some((s, params, captured)) = self.lift(*u, env) else {
                        return Statement::Error(name.to_string());
                    };
                    // what it captured, from where it is used, first
                    let mut all: Vec<Rc<Core>> =
                        captured.iter().map(|&lvl| Rc::new(Core::Var(env.slots.len() - 1 - lvl, "captured".into()))).collect();
                    all.extend(args.iter().cloned());
                    return self.call(env, c, s, &params, &all, k);
                }
                if let Some(params) = self.rec_params(*u) {
                    let s = self.symbol(*u).unwrap_or_else(|| Symbol::idyll("", name));
                    return self.call(env, c, s, &params, &args, k);
                }
                self.errors.push(format!("{name} is not a function the lowering knows"));
                return Statement::Error(name.to_string());
            }
            _ => {}
        }
        // a value applied one argument at a time
        let Some(f) = self.value(env, &head) else {
            let fv = self.name("f");
            let rep = Rep::Ptr;
            let rest = self.apply_args(env, Producer::Var(fv.clone()), &args, k);
            return self.term(env, &head, Consumer::MuTilde(Binder { name: fv, rep }, Box::new(rest)));
        };
        self.apply_args(env, f, &args, k)
    }

    /// `f a b ..`, `f` a value: `<f | apply(a; μ̃ g. <g | apply(b; k)>)>`.
    /// An erased argument is a descriptor if it is a type, and nothing
    /// otherwise.
    fn apply_args(&mut self, env: &Env, f: Producer, args: &[Rc<Core>], k: Consumer) -> Statement {
        let Some((first, rest)) = args.split_first() else { return Statement::Cut(f, k) };
        if let Core::Runtime(Note::Erased, a) = &**first {
            if !self.typer.is_type(&env.scope, a) {
                return self.apply_args(env, f, rest, k);
            }
            let d = self.descriptor(env, a);
            return self.apply_one(env, f, d, rest, k);
        }
        let env2 = env.clone();
        let rest: Vec<Rc<Core>> = rest.to_vec();
        self.atom(env, first, &mut |this, a| this.apply_one(&env2, f.clone(), a, &rest, k.clone()))
    }

    fn apply_one(&mut self, env: &Env, f: Producer, a: Producer, rest: &[Rc<Core>], k: Consumer) -> Statement {
        if rest.is_empty() {
            return Statement::Cut(f, Consumer::Method("apply".into(), vec![a], vec![k]));
        }
        let g = self.name("g");
        let then = self.apply_args(env, Producer::Var(g.clone()), rest, k);
        Statement::Cut(f, Consumer::Method("apply".into(), vec![a], vec![Consumer::MuTilde(Binder { name: g, rep: Rep::Ptr }, Box::new(then))]))
    }

    /// The descriptor for a type argument.
    fn descriptor(&mut self, env: &Env, ty: &Rc<Core>) -> Producer {
        let r = self.typer.rep_of_type(&env.scope, ty);
        self.descriptor_of(env, r)
    }

    /// The descriptor of a representation the typer gave.
    fn descriptor_of(&mut self, env: &Env, r: typer::Rep) -> Producer {
        match r {
            typer::Rep::Var(lvl) => match env.slots.get(lvl) {
                Some(Slot::TypeVar(Some(d), _)) => Producer::Var(d.clone()),
                // a record's field that is a type, which holds its descriptor
                Some(Slot::Var(d)) => Producer::Var(d.clone()),
                _ => Producer::Desc(Rep::Any),
            },
            typer::Rep::Field(lvl, i) => match env.fields.iter().find(|f| f.0 == lvl && f.1 == i) {
                Some((_, _, Some(d), _)) => Producer::Var(d.clone()),
                _ => Producer::Desc(Rep::Any),
            },
            r => Producer::Desc(self.rep(env, r)),
        }
    }

    /// A known function's arguments, as its parameters take them.
    fn arguments(&mut self, env: &Env, params: &[Param], args: &[Rc<Core>], rest: &mut dyn FnMut(&mut Self, Vec<Producer>) -> Statement) -> Statement {
        let mut values = Vec::new();
        let mut descs: HashMap<usize, Producer> = HashMap::new();
        // the descriptors that come after a record, for its fields that are types
        let mut after: HashMap<usize, Vec<Producer>> = HashMap::new();
        for (i, (p, a)) in params.iter().zip(args).enumerate() {
            match p {
                Param::Skip => {}
                Param::Desc => {
                    let ty = match &**a {
                        Core::Runtime(Note::Erased, t) => t.clone(),
                        _ => a.clone(),
                    };
                    descs.insert(i, self.descriptor(env, &ty));
                }
                Param::Value(_) => values.push((i, a.clone())),
                Param::Dict(_, ctor, fields) => {
                    values.push((i, a.clone()));
                    let mut ds = Vec::new();
                    for f in fields {
                        let r = self.typer.field_rep(&env.scope, a, *ctor, *f);
                        ds.push(self.descriptor_of(env, r));
                    }
                    after.insert(i, ds);
                }
            }
        }
        let cores: Vec<Rc<Core>> = values.iter().map(|(_, a)| a.clone()).collect();
        let positions: Vec<usize> = values.iter().map(|(i, _)| *i).collect();
        let n = params.len().min(args.len());
        self.atoms(env, &cores, &mut |this, ps| {
            let mut by_position: HashMap<usize, Producer> = positions.iter().copied().zip(ps).collect();
            by_position.extend(descs.clone());
            let mut ordered = Vec::new();
            for i in 0..n {
                ordered.extend(by_position.remove(&i));
                ordered.extend(after.get(&i).cloned().unwrap_or_default());
            }
            rest(this, ordered)
        })
    }

    /// A top-level function called: with all its arguments, a call; with
    /// more, a call whose result is applied to the rest; with fewer, an
    /// object waiting for the rest.
    fn call(&mut self, env: &Env, whole: &Rc<Core>, s: Symbol, params: &Rc<Vec<Param>>, args: &[Rc<Core>], k: Consumer) -> Statement {
        if args.len() < params.len() {
            return self.partial(env, whole, Head2::Call(s), &params[..args.len()], args, k);
        }
        let (now, later) = args.split_at(params.len());
        let later = later.to_vec();
        let env2 = env.clone();
        let params = params.clone();
        self.arguments(env, &params, now, &mut |this, ps| {
            if later.is_empty() {
                Statement::Call(s.clone(), ps, vec![k.clone()])
            } else {
                let r = this.name("r");
                let then = this.apply_args(&env2, Producer::Var(r.clone()), &later, k.clone());
                Statement::Call(s.clone(), ps, vec![Consumer::MuTilde(Binder { name: r, rep: Rep::Ptr }, Box::new(then))])
            }
        })
    }

    /// A partial application: its arguments computed, then an object that
    /// takes the rest one `apply` at a time, and then does it.  What the rest
    /// are, and how each is represented, the partial application's type says.
    fn partial(&mut self, env: &Env, whole: &Rc<Core>, head: Head2, given: &[Param], args: &[Rc<Core>], k: Consumer) -> Statement {
        let remaining = self.typer.function_params(&env.scope, whole);
        let env2 = env.clone();
        self.arguments(env, given, args, &mut |this, given| {
            let obj = this.waiting(&env2, &head, given, &remaining);
            Statement::Cut(obj, k.clone())
        })
    }

    /// The object a partial application is: one `apply` for each parameter
    /// still to come (a descriptor for a type, nothing for one erased), the
    /// last doing what `head` does with all of them.
    fn waiting(&mut self, env: &Env, head: &Head2, given: Vec<Producer>, remaining: &[(typer::Rep, bool, bool, bool)]) -> Producer {
        if remaining.iter().any(|p| p.3) {
            self.errors.push("a partial application that waits for a record with fields that are types is not lowered yet".into());
        }
        let names: Vec<(Rep, String)> = remaining
            .iter()
            .filter(|(_, is_type, erased, _)| *is_type || !*erased)
            .map(|(r, is_type, _, _)| (if *is_type { Rep::Desc } else { self.rep(env, *r) }, self.name("a")))
            .collect();
        let k = self.name("k");
        let all: Vec<Producer> = given.into_iter().chain(names.iter().map(|(_, n)| Producer::Var(n.clone()))).collect();
        let done = match head {
            Head2::Call(s) => Statement::Call(s.clone(), all, vec![Consumer::Var(k.clone())]),
            Head2::Con(s) => Statement::Cut(Producer::Con(s.clone(), all), Consumer::Var(k.clone())),
            Head2::Perform(s) => Statement::Perform(s.clone(), all, Consumer::Var(k.clone())),
            Head2::Prim(op) => Statement::Prim(op.clone(), prim_args(op, all), vec![Consumer::Var(k.clone())]),
        };
        // innermost first: the last parameter's object does it
        let (mut body, mut cont, mut obj) = (done, k, None);
        for (rep, n) in names.iter().rev() {
            if let Some(inner) = obj.take() {
                let outer = self.name("k");
                body = Statement::Cut(inner, Consumer::Var(outer.clone()));
                cont = outer;
            }
            let binder = Binder { name: n.clone(), rep: rep.clone() };
            obj = Some(Producer::Cocase(vec![Method { name: "apply".into(), params: vec![binder], conts: vec![cont.clone()], body: body.clone() }]));
        }
        obj.unwrap_or(Producer::Mu(cont, Box::new(body)))
    }

    fn prim(&mut self, env: &Env, whole: &Rc<Core>, p: Prim, args: &[Rc<Core>], k: Consumer) -> Statement {
        let op = match p {
            Prim::IntAdd => "add",
            Prim::IntMul => "mul",
            Prim::IntSub => "sub",
            Prim::IntDiv => "div",
            Prim::IntMod => "mod",
            Prim::IntLt => "lt",
            Prim::IntEq => "eq",
            // the runtime joins an array of strings
            Prim::StrAppend => CONCAT,
        };
        let rep = if p == Prim::StrAppend { Rep::Str } else { Rep::I64 };
        let params = [Param::Value(rep.clone()), Param::Value(rep)];
        if args.len() < 2 {
            return self.partial(env, whole, Head2::Prim(op.into()), &params[..args.len()], args, k);
        }
        self.atoms(env, &args[..2], &mut |_, ps| Statement::Prim(op.into(), prim_args(op, ps), vec![k.clone()]))
    }

    /// A native: an array's operation is the runtime's primitive of the
    /// same name, and a literal, `#[a; b]`, an array.
    fn native(&mut self, env: &Env, whole: &Rc<Core>, info: NativeInfo, args: &[Rc<Core>], k: Consumer) -> Statement {
        let n = info.native;
        let all = info.params + n.arity();
        if n == Native::ArrayFromList && args.len() == all {
            if let Some(items) = self.list_literal(&args[info.params]) {
                return self.atoms(env, &items, &mut |_, ps| Statement::Cut(Producer::Array(ps), k.clone()));
            }
        }
        // one the runtime has no primitive for: a `def` of the program's
        // own (see [`natives`]), generic in the element as its type is
        if let Some((symbol, params)) = self.native_helper(n) {
            return self.call(env, whole, symbol, &params, args, k);
        }
        let reps: Vec<Rep> = match n {
            Native::Num(op, ty) => op.takes_each(ty).into_iter().map(Rep::number).collect(),
            _ => self.native_reps(n).to_vec(),
        };
        if reps.is_empty() && n.arity() > 0 {
            self.errors.push(format!("{} is not lowered yet", n.name()));
            return Statement::Error(n.name().to_string());
        }
        let params: Vec<Param> = (0..info.params).map(|_| Param::Skip).chain(reps.iter().cloned().map(Param::Value)).collect();
        let op = match n {
            Native::Num(op, ty) => op.prim(ty),
            Native::StringHash | Native::CharHash | Native::BoolHash => "hash".to_string(),
            _ => n.name().to_string(),
        };
        self.native_prim(env, whole, info, args, k, params, op)
    }

    /// What a native that is the runtime's primitive of its name takes.
    fn native_reps(&self, n: Native) -> &'static [Rep] {
        match n {
            Native::ArrayLen | Native::ArrayPop => &[Rep::Ptr],
            Native::ArrayGet => &[Rep::Ptr, Rep::I64],
            Native::ArrayGetOr => &[Rep::Any, Rep::Ptr, Rep::I64],
            Native::ArraySet => &[Rep::Ptr, Rep::I64, Rep::Any],
            Native::ArrayPush => &[Rep::Ptr, Rep::Any],
            Native::ArraySlice => &[Rep::Ptr, Rep::I64, Rep::I64],
            Native::ArrayConcat => &[Rep::Ptr, Rep::Ptr],
            Native::StringByteLength => &[Rep::Str],
            Native::StringSlice => &[Rep::Str, Rep::I64, Rep::I64],
            Native::StringCompare => &[Rep::Str, Rep::Str],
            Native::StringIndexOf => &[Rep::Str, Rep::Str, Rep::I64],
            Native::StringToChars => &[Rep::Str],
            Native::CharsToString => &[Rep::Ptr],
            Native::CharCode => &[Rep::Char],
            Native::CharFromCode => &[Rep::I64],
            Native::StringHash => &[Rep::Str],
            Native::CharHash => &[Rep::Char],
            Native::BoolHash => &[Rep::Bool],
            _ => &[],
        }
    }

    /// The rest of [`Self::native`]: a native that is the runtime's
    /// primitive `op`, taking values of `reps`.
    #[allow(clippy::too_many_arguments)]
    fn native_prim(&mut self, env: &Env, whole: &Rc<Core>, info: NativeInfo, args: &[Rc<Core>], k: Consumer, params: Vec<Param>, op: String) -> Statement {
        let n = info.native;
        let all = info.params + n.arity();
        if args.len() < all {
            return self.partial(env, whole, Head2::Prim(op), &params[..args.len()], args, k);
        }
        let values = args[info.params..all].to_vec();
        let later = args[all..].to_vec();
        let env2 = env.clone();
        // What an array holds comes out as whatever was stored, which only a
        // binder can say how it is represented: given straight to a
        // continuation, it would be taken for a pointer (`docs/CUT.md`).
        let stored = matches!(n, Native::ArrayGet | Native::ArrayGetOr) && later.is_empty() && !matches!(k, Consumer::MuTilde(..));
        let stored = if stored { Some(self.rep_of(env, whole)).filter(|r| *r != Rep::Ptr) } else { None };
        self.atoms(env, &values, &mut |this, ps| {
            if let Some(rep) = &stored {
                let x = this.name("x");
                let give = Statement::Cut(Producer::Var(x.clone()), k.clone());
                return Statement::Prim(op.clone(), ps, vec![Consumer::MuTilde(Binder { name: x, rep: rep.clone() }, Box::new(give))]);
            }
            if later.is_empty() {
                return Statement::Prim(op.clone(), ps, vec![k.clone()]);
            }
            let r = this.name("r");
            let then = this.apply_args(&env2, Producer::Var(r.clone()), &later, k.clone());
            Statement::Prim(op.clone(), ps, vec![Consumer::MuTilde(Binder { name: r, rep: Rep::Any }, Box::new(then))])
        })
    }

    /// The elements of a list written out, `x :: y :: Nil` (as an array
    /// literal's are), if `c` is one.
    fn list_literal(&self, c: &Rc<Core>) -> Option<Vec<Rc<Core>>> {
        let mut items = Vec::new();
        let mut at = c.clone();
        loop {
            let mut args = Vec::new();
            let mut head = at.clone();
            while let Core::App(f, a) = &*head {
                args.push(a.clone());
                head = f.clone();
            }
            args.reverse();
            let Core::Opaque(u, _) = &*head else { return None };
            let info = self.ctors.get(u)?;
            match (info.name.as_str(), args.len()) {
                ("Nil", 1) => return Some(items),
                ("::", 3) => {
                    items.push(args[1].clone());
                    at = args[2].clone();
                }
                _ => return None,
            }
        }
    }

    fn construct(&mut self, env: &Env, whole: &Rc<Core>, u: usize, info: &idyllic_typecheck::CtorInfo, args: &[Rc<Core>], k: Consumer) -> Statement {
        self.datas.insert(info.data);
        let s = self.symbol(u).unwrap_or_else(|| Symbol::idyll("", &info.name));
        let shape = self.typer.ctor_shape(u);
        let reps: Vec<Rep> = shape.map(|(_, f)| f.into_iter().map(|r| self.rep(env, r)).collect()).unwrap_or_default();
        let params: Vec<Param> = (0..info.params)
            .map(|_| Param::Skip)
            // a field that is a type holds its descriptor
            .chain((0..info.fields).map(|i| match reps.get(i) {
                Some(Rep::Desc) => Param::Desc,
                r => Param::Value(r.cloned().unwrap_or(Rep::Any)),
            }))
            .collect();
        if args.len() < params.len() {
            return self.partial(env, whole, Head2::Con(s), &params[..args.len()], args, k);
        }
        self.arguments(env, &params, &args[..params.len()], &mut |_, ps| Statement::Cut(Producer::Con(s.clone(), ps), k.clone()))
    }

    fn perform(&mut self, env: &Env, whole: &Rc<Core>, u: usize, info: &idyllic_typecheck::handlers::OpInfo, args: &[Rc<Core>], k: Consumer) -> Statement {
        self.effects.insert(info.effect);
        self.performed.insert(u);
        let s = self.symbol(u).unwrap_or_else(|| Symbol::idyll("", &info.name));
        let n = info.erased + info.arity;
        if args.len() < n {
            let reps = self.typer.op_shape(u).map(|(ps, _)| ps).unwrap_or_default();
            let params: Vec<Param> = (0..info.erased)
                .map(|_| Param::Skip)
                .chain(reps.into_iter().map(|r| Param::Value(self.rep(env, r))))
                .collect();
            return self.partial(env, whole, Head2::Perform(s), &params[..args.len()], args, k);
        }
        let values = args[info.erased..n].to_vec();
        let later = args[n..].to_vec();
        let env2 = env.clone();
        self.atoms(env, &values, &mut |this, ps| {
            if later.is_empty() {
                Statement::Perform(s.clone(), ps, k.clone())
            } else {
                let r = this.name("r");
                let then = this.apply_args(&env2, Producer::Var(r.clone()), &later, k.clone());
                Statement::Perform(s.clone(), ps, Consumer::MuTilde(Binder { name: r, rep: Rep::Ptr }, Box::new(then)))
            }
        })
    }

    /// A `case`: on data, a `case` consumer; on a `Bool`, `prim if`.  The
    /// consumer it goes to is named first if it is not a variable already,
    /// so that the arms share it.
    fn case(&mut self, env: &Env, scrutinee: &Rc<Core>, alts: &[idyllic_typecheck::core::CaseAlt], k: Consumer) -> Statement {
        if !matches!(k, Consumer::Var(_) | Consumer::Halt) {
            let j = self.name("j");
            let inner = self.case(env, scrutinee, alts, Consumer::Var(j.clone()));
            return Statement::Cut(Producer::Mu(j, Box::new(inner)), k);
        }
        if alts.iter().all(|a| matches!(a.head, Head::Bool(_))) {
            let arm = |this: &mut Self, b: bool| match alts.iter().find(|a| a.head == Head::Bool(b)) {
                Some(a) => this.term(env, &a.body, k.clone()),
                None => Statement::Error("no alternative for this".into()),
            };
            let (f, t) = (arm(self, false), arm(self, true));
            let (xf, xt) = (self.name("u"), self.name("u"));
            return self.atom(env, scrutinee, &mut |_, b| {
                Statement::Prim(
                    "if".into(),
                    vec![b],
                    vec![
                        Consumer::MuTilde(Binder { name: xf.clone(), rep: Rep::Unit }, Box::new(f.clone())),
                        Consumer::MuTilde(Binder { name: xt.clone(), rep: Rep::Unit }, Box::new(t.clone())),
                    ],
                )
            });
        }
        let mut arms = Vec::new();
        for alt in alts {
            let Head::Con(u) = alt.head else { continue };
            let info = self.ctors.get(&u).cloned();
            if let Some(i) = &info {
                self.datas.insert(i.data);
            }
            let params = info.as_ref().map_or(0, |i| i.params);
            let types = self.typer.alt_types(&env.scope, scrutinee, alt);
            let inner_scope = self.typer.alt_scope(&env.scope, scrutinee, alt);
            let mut inner = Env { slots: env.slots.clone(), scope: inner_scope, fields: env.fields.clone() };
            let mut fields = Vec::new();
            let held = self.typer.type_fields(u);
            for (i, x) in alt.vars.iter().enumerate() {
                if i < params {
                    inner.slots.push(Slot::Erased);
                } else {
                    let v = self.name(x);
                    // a field that is a type holds its descriptor
                    let r = if held.iter().any(|(f, _)| *f == i) {
                        typer::Rep::Desc
                    } else {
                        types.get(i).map_or(typer::Rep::Any, |t| self.typer.rep(&inner.scope, t))
                    };
                    fields.push(Binder { name: v.clone(), rep: self.rep(&inner, r) });
                    inner.slots.push(Slot::Var(v));
                }
            }
            let body = self.term(&inner, &alt.body, k.clone());
            arms.push(Arm { ctor: self.symbol(u), fields, body });
        }
        self.term(env, scrutinee, Consumer::Case(arms))
    }

    /// `handle body with clauses`.
    fn handle(&mut self, env: &Env, body: &Rc<Core>, ret: &Rc<Core>, clauses: &[idyllic_typecheck::core::OpClause], k: Consumer) -> Statement {
        let mut cut_clauses = Vec::new();
        for c in clauses {
            if let Some(info) = self.ops.get(&c.op) {
                self.effects.insert(info.effect);
            }
            let arity = self.ops.get(&c.op).map_or(1, |i| i.arity);
            let mut inner = env.clone();
            let mut lam = c.body.clone();
            // the operation's erased arguments, then its arguments, then the
            // resumption
            let (mut params, mut rep_vars) = (Vec::new(), Vec::new());
            for _ in 0..c.erased {
                let Some((x, _, ty, body)) = lambda(&lam) else { break };
                inner.scope = match &ty {
                    Some(t) => self.typer.bind(&inner.scope, &x, t),
                    None => self.typer.bind_unknown(&inner.scope, &x),
                };
                inner.slots.push(Slot::TypeVar(None, x.clone()));
                lam = body;
            }
            for _ in 0..arity {
                let Some((x, grade, ty, body)) = lambda(&lam) else { break };
                self.bind_method_param(&mut inner, &x, grade, ty.as_ref(), &mut params, &mut rep_vars);
                lam = body;
            }
            let Some((x, _, ty, body)) = lambda(&lam) else {
                self.errors.push(format!("{}'s clause has no resumption", c.name));
                continue;
            };
            let r = self.name("r");
            inner.scope = match &ty {
                Some(t) => self.typer.bind(&inner.scope, &x, t),
                None => self.typer.bind_unknown(&inner.scope, &x),
            };
            inner.slots.push(Slot::Resume(r.clone()));
            let kc = self.name("k");
            let s = self.term(&inner, &body, Consumer::Var(kc.clone()));
            let op = self.symbol(c.op).unwrap_or_else(|| Symbol::idyll("", &c.name));
            cut_clauses.push(Clause { op, params, resumption: r, cont: kc, body: s });
        }
        // `return x -> ..`
        let ret_clause = match lambda(ret) {
            Some((x, grade, ty, body)) => {
                let mut inner = env.clone();
                let (mut params, mut rep_vars) = (Vec::new(), Vec::new());
                self.bind_method_param(&mut inner, &x, grade, ty.as_ref(), &mut params, &mut rep_vars);
                let kc = self.name("k");
                let s = self.term(&inner, &body, Consumer::Var(kc.clone()));
                let x = params.pop().unwrap_or(Binder { name: self.name("x"), rep: Rep::Unit });
                (x, kc, s)
            }
            None => {
                let (x, kc) = (self.name("x"), self.name("k"));
                let s = Statement::Cut(Producer::Var(x.clone()), Consumer::Var(kc.clone()));
                (Binder { name: x, rep: Rep::Any }, kc, s)
            }
        };
        let b = self.name("b");
        let body = self.term(env, body, Consumer::Var(b.clone()));
        Statement::Handle(Box::new(Handle { clauses: cut_clauses, ret: ret_clause, body_cont: b, body, cont: k }))
    }

    /// The table: data types, roles, effects and native operations.
    fn declarations(&mut self, entry: Option<Symbol>, answer: Answer) -> Program {
        let mut datas = Vec::new();
        for d in self.datas.clone() {
            let Some(symbol) = self.symbol(d) else { continue };
            let mut ctors: Vec<(usize, idyllic_typecheck::CtorInfo)> =
                self.ctors.iter().filter(|(_, c)| c.data == d).map(|(u, c)| (*u, c.clone())).collect();
            ctors.sort_by_key(|(_, c)| c.tag);
            let mut rep_vars: Vec<String> = Vec::new();
            let mut out = Vec::new();
            for (u, c) in ctors {
                let (kinds, fields) = self.typer.ctor_shape(u).unwrap_or_default();
                // a field of a type parameter: that parameter's variable
                let fields = fields
                    .into_iter()
                    .map(|r| match r {
                        typer::Rep::Var(i) if kinds.get(i) == Some(&true) => {
                            let a = format!("a{i}");
                            if !rep_vars.contains(&a) {
                                rep_vars.push(a.clone());
                            }
                            Rep::Var(a)
                        }
                        typer::Rep::I64 => Rep::I64,
                        typer::Rep::Word(w) => Rep::Word(w),
                        typer::Rep::F64 => Rep::F64,
                        typer::Rep::F32 => Rep::F32,
                        typer::Rep::Bool => Rep::Bool,
                        typer::Rep::Unit | typer::Rep::Erased => Rep::Unit,
                        typer::Rep::Str => Rep::Str,
            typer::Rep::Char => Rep::Char,
                        typer::Rep::Ptr => Rep::Ptr,
                        typer::Rep::Desc => Rep::Desc,
                        _ => Rep::Any,
                    })
                    .collect();
                out.push((c.name.clone(), fields));
            }
            rep_vars.sort();
            datas.push(DataDecl { symbol, rep_vars, ctors: out });
        }
        // the roles, for the prelude's types the program uses
        let mut roles = Vec::new();
        let declared: BTreeSet<Symbol> = datas.iter().map(|d| d.symbol.clone()).collect();
        for (role, path) in ROLES {
            let s = Symbol::idyll("Prelude", path);
            let ty = Symbol { path: s.path[..s.path.len() - 1].to_vec(), ..s.clone() };
            if declared.contains(&ty) {
                roles.push((role.to_string(), s));
            }
        }
        // the effects, and the world's operations left to the runtime
        let mut effects = Vec::new();
        for e in self.effects.clone() {
            let Some(symbol) = self.symbol(e) else { continue };
            let mut ops: Vec<(usize, idyllic_typecheck::handlers::OpInfo)> =
                self.ops.iter().filter(|(_, o)| o.effect == e).map(|(u, o)| (*u, o.clone())).collect();
            ops.sort_by_key(|(_, o)| o.index);
            let mut out = Vec::new();
            for (u, o) in ops {
                let (params, result) = self.typer.op_shape(u).unwrap_or((Vec::new(), typer::Rep::Any));
                let conv = |r: typer::Rep| match r {
                    typer::Rep::I64 => Rep::I64,
                    typer::Rep::Word(w) => Rep::Word(w),
                    typer::Rep::F64 => Rep::F64,
                    typer::Rep::F32 => Rep::F32,
                    typer::Rep::Bool => Rep::Bool,
                    typer::Rep::Unit | typer::Rep::Erased => Rep::Unit,
                    typer::Rep::Str => Rep::Str,
            typer::Rep::Char => Rep::Char,
                    typer::Rep::Ptr => Rep::Ptr,
                    _ => Rep::Any,
                };
                out.push(OpDecl { name: o.name.clone(), many: o.many, params: params.into_iter().map(conv).collect(), result: conv(result) });
            }
            effects.push(EffectDecl { symbol, ops: out });
        }
        let mut natives = Vec::new();
        for u in self.performed.clone() {
            if self.handled_by_entry == Some(u) {
                continue;
            }
            let Some((package, _)) = self.symbols.get(&u) else { continue };
            if package != "Prelude" {
                continue;
            }
            let name = &self.ops[&u].name;
            match NATIVE.iter().find(|(n, _)| n == name) {
                Some((_, runtime)) => natives.push((self.symbol(u).expect("a symbol"), runtime.to_string())),
                None => self.errors.push(format!(
                    "{name} has no runtime operation of its shape yet: on the shared back end the prelude will define it over the runtime's"
                )),
            }
        }
        natives.sort();
        Program {
            entry,
            answer,
            datas,
            roles,
            natives,
            effects,
            vals: std::mem::take(&mut self.vals),
            defs: std::mem::take(&mut self.defs),
        }
    }
}

/// The runtime's primitive that joins strings: `concatStrings`, given an
/// array of them.
const CONCAT: &str = "concatStrings";

/// A primitive's arguments as the runtime takes them: `concatStrings` takes
/// its strings as one array.
fn prim_args(op: &str, args: Vec<Producer>) -> Vec<Producer> {
    if op == CONCAT {
        vec![Producer::Array(args)]
    } else {
        args
    }
}

/// What a partial application is of.
enum Head2 {
    Call(Symbol),
    Con(Symbol),
    Perform(Symbol),
    Prim(String),
}

/// A lambda's variable, the variable's grade if a constant, its type if
/// noted, and the lambda's body.
type Lambda = (String, Option<Grade>, Option<Rc<Core>>, Rc<Core>);

/// A lambda, through the note on it.
fn lambda(c: &Rc<Core>) -> Option<Lambda> {
    match &**c {
        Core::Lam(x, body) => Some((x.clone(), None, None, body.clone())),
        Core::Runtime(Note::Binds { grade, ty }, inner) => match &**inner {
            Core::Lam(x, body) => Some((x.clone(), *grade, ty.clone(), body.clone())),
            _ => None,
        },
        _ => None,
    }
}

/// Where each recursive definition inside another is: the level of the
/// top-level scope it is in, which is that of the top-level definition (or
/// the expression) that uses it, since nothing outside one can.  `program`
/// has `lets` top-level `let`s, and starts in a context of `base`
/// variables.
fn owners(program: &Rc<Core>, lets: usize, base: usize, recs: &HashMap<usize, RecDef>) -> HashMap<usize, usize> {
    let mut out: HashMap<usize, usize> = HashMap::new();
    let mut work: Vec<(usize, Rc<Core>)> = Vec::new();
    let mut c = program.clone();
    for i in 0..lets {
        let Core::Let(_, rhs, rest) = &*c else { break };
        work.push((base + i, rhs.clone()));
        c = rest.clone();
    }
    work.push((base + lets, c));
    // a top-level recursive definition's scope is its own level
    work.extend(recs.values().filter(|d| !d.local).map(|d| (d.lvl, d.core.clone())));
    while let Some((top, c)) = work.pop() {
        let (mut uses, mut opaques) = (Uses::default(), BTreeSet::new());
        used(&c, 0, 0, false, &mut uses, &mut opaques);
        for u in opaques {
            if let Some(d) = recs.get(&u).filter(|d| d.local) {
                if !out.contains_key(&u) {
                    out.insert(u, top);
                    // and what it uses inside it is in the same scope
                    work.push((top, d.core.clone()));
                }
            }
        }
    }
    out
}

/// The levels a term uses: as values, which have to be there when it runs,
/// and at all, which a type's descriptor may be wanted for.  An erased
/// argument is never computed, so what it mentions it uses only as a type;
/// so do types and a binder's note.
#[derive(Clone, Default)]
struct Uses {
    values: BTreeSet<usize>,
    all: BTreeSet<usize>,
}

/// For each recursive definition inside another, with the level of the
/// top-level scope it is in (see [`owners`]): that level, and the levels
/// between it and its own that it uses -- or that a recursive definition
/// it calls does, since a call passes them on.
fn local_free_levels(recs: &HashMap<usize, RecDef>, owners: &HashMap<usize, usize>) -> HashMap<usize, (usize, Uses)> {
    let mut out: HashMap<usize, (usize, Uses)> = HashMap::new();
    let mut calls: HashMap<usize, BTreeSet<usize>> = HashMap::new();
    for (u, d) in recs {
        let Some(&top) = owners.get(u) else { continue };
        let (mut uses, mut opaques) = (Uses::default(), BTreeSet::new());
        used(&d.core, d.lvl, 0, false, &mut uses, &mut opaques);
        uses.values.retain(|l| (top..d.lvl).contains(l));
        uses.all.retain(|l| (top..d.lvl).contains(l));
        out.insert(*u, (top, uses));
        calls.insert(*u, opaques);
    }
    loop {
        let mut changed = false;
        for (u, callees) in &calls {
            let (top, lvl) = (out[u].0, recs[u].lvl);
            let within = |l: &usize| (top..lvl).contains(l);
            let (mut values, mut all) = (Vec::new(), Vec::new());
            for (_, theirs) in callees.iter().filter_map(|e| out.get(e)) {
                values.extend(theirs.values.iter().copied().filter(|l| within(l) && !out[u].1.values.contains(l)));
                all.extend(theirs.all.iter().copied().filter(|l| within(l) && !out[u].1.all.contains(l)));
            }
            if !values.is_empty() || !all.is_empty() {
                let mine = &mut out.get_mut(u).expect("listed").1;
                mine.values.extend(values);
                mine.all.extend(all);
                changed = true;
            }
        }
        if !changed {
            return out;
        }
    }
}

/// What a term uses (in a context of `at` variables, `depth` binders in;
/// `erased` if it is never computed), and the constants it mentions.
fn used(c: &Core, at: usize, depth: usize, erased: bool, uses: &mut Uses, opaques: &mut BTreeSet<usize>) {
    let mut go = |c: &Core, d: usize, e: bool| used(c, at, d, erased || e, uses, opaques);
    match c {
        Core::Var(i, _) => {
            if *i >= depth {
                if let Some(l) = at.checked_sub(i - depth + 1) {
                    uses.all.insert(l);
                    if !erased {
                        uses.values.insert(l);
                    }
                }
            }
        }
        Core::Opaque(u, _) => {
            opaques.insert(*u);
        }
        Core::Pi { grade, in_type, dom, cod, .. } => {
            go(grade, depth, true);
            go(in_type, depth, true);
            go(dom, depth, true);
            go(cod, depth + 1, true);
        }
        Core::Lam(_, b) => go(b, depth + 1, false),
        Core::Let(_, e, b) => {
            go(e, depth, false);
            go(b, depth + 1, false);
        }
        Core::App(a, b) => {
            go(a, depth, false);
            go(b, depth, false);
        }
        Core::BoxTy(a, b) | Core::GradeAdd(a, b) | Core::GradeMul(a, b) | Core::GradeRange(a, b) | Core::Comp(a, b) => {
            go(a, depth, true);
            go(b, depth, true);
        }
        Core::Runtime(Note::Erased, a) => go(a, depth, true),
        Core::Runtime(note, a) => {
            if let Note::Binds { ty: Some(t), .. } = note {
                go(t, depth, true);
            }
            go(a, depth, false);
        }
        Core::BoxIntro(a) | Core::Unbox(a) => go(a, depth, false),
        Core::Case(scrutinee, alts) => {
            go(scrutinee, depth, false);
            for alt in alts.iter() {
                go(&alt.body, depth + alt.vars.len(), false);
            }
        }
        Core::Handle(b, r, ops) => {
            go(b, depth, false);
            go(r, depth, false);
            for op in ops.iter() {
                go(&op.body, depth, false);
            }
        }
        Core::Row(labels, tails) => {
            for x in labels.iter().chain(tails) {
                go(x, depth, true);
            }
        }
        Core::Type(_) | Core::LitTy(_) | Core::Lit(_) | Core::Prim(_) | Core::Meta(_) | Core::GradeLit(_) | Core::Error => {}
    }
}
