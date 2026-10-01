//! AxCut: a sequent-calculus intermediate language with only axioms and
//! cuts, after Schuster, Müller, Ostermann & Brachthäuser, "Compiling
//! Classical Sequent Calculus to Stock Hardware: The Duality of Compilation"
//! (OOPSLA 2025).  This is where Idyll stops being a type theory and starts
//! being a program: [`lower`] translates elaborated Core into it,
//! [`linearize`] makes the use of every variable explicit, and [`machine`]
//! runs the result.
//!
//! There are no expressions, only *statements*, and no implicit stack.  A
//! signature's symbols are at once the constructors of a data type and the
//! destructors of a codata type, and there are four ways to use one:
//!
//! ```text
//! let v = m(Γ); s          build a producer: a tagged record of fields Γ
//! switch v { m(Γ) ⇒ s }    take one apart, by cases
//! new v = (Γ){ m(Γ') ⇒ s } build a consumer: clauses closed over Γ
//! invoke v m               run one of a consumer's clauses
//! ```
//!
//! Functions are consumers with one clause, `apply(x, k)`; so are
//! continuations, `ret(r)`.  "Calling" and "returning" are both `invoke`, and
//! the continuation is a variable like any other — which is why the abstract
//! machine needs no stack.
//!
//! **The environment is ordered and linear.**  `let` and `new` consume the
//! *tail* of the environment, `switch` and `invoke` their variable from its
//! end, and `invoke` and `jump` need what remains to be exactly what the
//! clause or label expects.  The one statement that may reorder, duplicate or
//! drop variables is `substitute`.  On hardware the environment is the
//! register file and a substitution is a parallel move, so nothing is ever
//! looked up by name; here, names are kept and the machine checks them.
//!
//! [`lower`] produces the convenient form, with variables used freely and
//! arguments written out; [`linearize`] infers the substitutions.  Both are
//! [`Stmt`]s; only the second is AxCut proper.
//!
//! **No types.**  AxCut is simply typed (`prd T`, `cns T`, `ext T`), and Idyll
//! is dependently typed, so there is no typing the one in the other; types
//! are erased on the way down.  What typing would guarantee statically, the
//! machine checks as it goes.

pub mod grades;
pub mod inline;
pub mod linearize;
pub mod lower;
pub mod machine;

use idyllic_typecheck::grade::Grade;
use num_bigint::BigInt;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

pub type Var = String;
pub type Label = String;

/// The symbols of the signatures lowering uses: a fixed few, and the
/// constructors of the program's data types.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Symbol {
    /// A declared constructor: its name, and its position among its type's
    /// constructors, which is what a `switch` dispatches on.
    Con(Rc<str>, usize),
    /// `Fun { apply(x, k) }`: a function, given an argument and where to
    /// return.
    Apply,
    /// `Cont { ret(r) }`: a continuation, given a result.
    Ret,
    /// `Box { box(x) }`: the graded modality.  Grades are static, so at run
    /// time a box is a record of one field.
    Box,
    True,
    False,
    Unit,
    /// What is left of a type (or a grade) at run time.
    Erased,
    /// A handler's `return(x, k)`: what the handled computation's value
    /// becomes.
    Return,
    /// A handler's clause for an operation, `op(x, r, k)`: its name, and its
    /// position among its effect's operations.
    Op(Rc<str>, usize),
}

impl Symbol {
    /// What a `switch` dispatches on.  `False` and `True` are `0` and `1`,
    /// which is also what a comparison produces.
    pub fn tag(&self) -> usize {
        match self {
            Symbol::Con(_, tag) => *tag,
            Symbol::True => 1,
            // after a handler's `return`
            Symbol::Op(_, index) => 1 + index,
            _ => 0,
        }
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Symbol::Con(name, _) => name,
            Symbol::Apply => "apply",
            Symbol::Ret => "ret",
            Symbol::Box => "box",
            Symbol::True => "true",
            Symbol::False => "false",
            Symbol::Unit => "unit",
            Symbol::Erased => "erased",
            Symbol::Return => "return",
            Symbol::Op(name, _) => name,
        })
    }
}

/// Primitive operations on external types: the machine's integers (here,
/// big ones) and strings.  An extern does not consume its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Extern {
    /// `extern lit n { (v) ⇒ s }`
    LitInt(BigInt),
    LitStr(String),
    /// `extern add(a, b) { (v) ⇒ s }`
    Add,
    Mul,
    Sub,
    /// division and remainder, rounding toward zero
    Div,
    Mod,
    /// `extern lt(a, b) { (v) ⇒ s }`: `v` is `true()` or `false()`, which
    /// have no fields and so are not allocated
    Lt,
    Eq,
    /// `extern halt(v) {}`: no clauses, so nothing follows.  The program's
    /// answer is `v`.
    Halt,
}

/// The parameters a clause binds, and what it does with them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    pub params: Vec<Var>,
    pub body: Stmt,
}

pub type Clauses = Vec<(Symbol, Branch)>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stmt {
    /// `substitute [new ↦ old, ...]; s`: the environment of `s`, in order,
    /// in terms of the current one.
    Substitute(Vec<(Var, Var)>, Box<Stmt>),
    /// `let v = m(args); s`
    Let(Var, Symbol, Vec<Var>, Box<Stmt>),
    /// `new v = (captured){ clauses }; s`.  Lowering leaves `captured` empty;
    /// it is the clauses' free variables, which [`linearize`] works out.
    New(Var, Vec<Var>, Clauses, Box<Stmt>),
    /// `switch v { clauses }`
    Switch(Var, Clauses),
    /// `invoke v m`, with the arguments the clause will receive.  Once
    /// linearized they are exactly the rest of the environment, which is why
    /// AxCut does not write them.
    Invoke(Var, Symbol, Vec<Var>),
    /// `jump l`, likewise.
    Jump(Label, Vec<Var>),
    /// `extern m(args){ (Γ) ⇒ s, ... }`
    Extern(Extern, Vec<Var>, Vec<Branch>),
    /// `handle h k as ret; s`: run `s` under the handler `h`, a consumer
    /// with a `return(x, k)` clause and an `op(x, r, k)` clause for each
    /// operation of its effect.  `s` sends its value to `ret`, which hands it
    /// to `h`'s `return`; and what the handler gives goes to `k`.  Consumes
    /// `h` and `k`, as `let` consumes its fields.
    Handle(Var, Var, Var, Box<Stmt>),
    /// `perform op(x, ..) k`: the nearest handler with a clause for `op`
    /// takes over, given the arguments, a resumption that continues with
    /// `k`, and where its own `handle` goes.  Like `invoke`, it leaves
    /// nothing behind.
    Perform(Symbol, Vec<Var>, Var),
}

/// `define l : (Γ) = s`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition {
    pub label: Label,
    pub params: Vec<Var>,
    pub body: Stmt,
}

/// A list of labelled statements.  Execution starts at the first, which
/// takes no parameters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Program {
    pub definitions: Vec<Definition>,
    /// What the type checker promised about how often a variable is used,
    /// for the variables it said anything about.  Variables are named apart,
    /// so one table serves the whole program.  Nothing at run time depends
    /// on it; [`grades::check`] holds the code to it.
    pub grades: HashMap<Var, Grade>,
    /// The variables that stand for a continuation: where a call returns to.
    /// The machine treats them like any other consumer; native code keeps
    /// them on the stack (`idyllic_llvm`), and has to know which they are.
    pub continuations: HashSet<Var>,
}

impl Program {
    pub fn new(definitions: Vec<Definition>) -> Program {
        Program { definitions, grades: HashMap::new(), continuations: HashSet::new() }
    }

    pub fn definition(&self, label: &str) -> Option<&Definition> {
        self.definitions.iter().find(|d| d.label == label)
    }
}

// ---------------------------------------------------------------------------
// Free variables
// ---------------------------------------------------------------------------

/// The variables a statement needs from its environment, each once, in order
/// of first use.
pub fn free_vars(s: &Stmt) -> Vec<Var> {
    free_vars_where(s, &|_, args| args.to_vec())
}

/// As [`free_vars`], given which of a jump's arguments its label really
/// takes (while that is still being worked out).
pub fn free_vars_where(s: &Stmt, jump: &dyn Fn(&Label, &[Var]) -> Vec<Var>) -> Vec<Var> {
    fn add(out: &mut Vec<Var>, v: &Var) {
        if !out.contains(v) {
            out.push(v.clone());
        }
    }
    let under = |out: &mut Vec<Var>, bound: &[Var], s: &Stmt| {
        for v in free_vars_where(s, jump) {
            if !bound.contains(&v) {
                add(out, &v);
            }
        }
    };
    let mut out = Vec::new();
    match s {
        Stmt::Substitute(pairs, _) => pairs.iter().for_each(|(_, old)| add(&mut out, old)),
        Stmt::Let(v, _, args, rest) => {
            args.iter().for_each(|a| add(&mut out, a));
            under(&mut out, std::slice::from_ref(v), rest);
        }
        Stmt::New(v, _, clauses, rest) => {
            clauses.iter().for_each(|(_, b)| under(&mut out, &b.params, &b.body));
            under(&mut out, std::slice::from_ref(v), rest);
        }
        Stmt::Switch(v, clauses) => {
            add(&mut out, v);
            clauses.iter().for_each(|(_, b)| under(&mut out, &b.params, &b.body));
        }
        Stmt::Invoke(v, _, args) => {
            args.iter().for_each(|a| add(&mut out, a));
            add(&mut out, v);
        }
        Stmt::Jump(l, args) => jump(l, args).iter().for_each(|a| add(&mut out, a)),
        Stmt::Extern(_, args, branches) => {
            args.iter().for_each(|a| add(&mut out, a));
            branches.iter().for_each(|b| under(&mut out, &b.params, &b.body));
        }
        Stmt::Handle(h, k, ret, rest) => {
            add(&mut out, h);
            add(&mut out, k);
            under(&mut out, std::slice::from_ref(ret), rest);
        }
        Stmt::Perform(_, xs, k) => {
            xs.iter().for_each(|x| add(&mut out, x));
            add(&mut out, k);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Printing, in the paper's notation
// ---------------------------------------------------------------------------

fn commas(vars: &[Var]) -> String {
    vars.join(", ")
}

impl fmt::Display for Extern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Extern::LitInt(n) => write!(f, "lit {n}"),
            Extern::LitStr(s) => write!(f, "lit {s:?}"),
            Extern::Add => write!(f, "add"),
            Extern::Mul => write!(f, "mul"),
            Extern::Sub => write!(f, "sub"),
            Extern::Div => write!(f, "div"),
            Extern::Mod => write!(f, "mod"),
            Extern::Lt => write!(f, "lt"),
            Extern::Eq => write!(f, "eq"),
            Extern::Halt => write!(f, "halt"),
        }
    }
}

impl Stmt {
    /// Statements run on, one per line, at one indentation; clauses nest.
    fn render(&self, indent: usize, out: &mut String) {
        let pad = "  ".repeat(indent);
        let clauses = |clauses: &Clauses, out: &mut String| {
            for (i, (m, b)) in clauses.iter().enumerate() {
                out.push_str(&format!("{pad}  {m}({}) ⇒\n", commas(&b.params)));
                b.body.render(indent + 2, out);
                out.push_str(if i + 1 < clauses.len() { ",\n" } else { " }" });
            }
        };
        match self {
            Stmt::Substitute(pairs, rest) => {
                let pairs: Vec<String> = pairs.iter().map(|(new, old)| format!("{new} ↦ {old}")).collect();
                out.push_str(&format!("{pad}substitute [{}];\n", pairs.join(", ")));
                rest.render(indent, out);
            }
            Stmt::Let(v, m, args, rest) => {
                out.push_str(&format!("{pad}let {v} = {m}({});\n", commas(args)));
                rest.render(indent, out);
            }
            Stmt::New(v, captured, cs, rest) => {
                out.push_str(&format!("{pad}new {v} = ({}){{\n", commas(captured)));
                clauses(cs, out);
                out.push_str(";\n");
                rest.render(indent, out);
            }
            Stmt::Switch(v, cs) => {
                out.push_str(&format!("{pad}switch {v} {{\n"));
                clauses(cs, out);
            }
            Stmt::Invoke(v, m, _) => out.push_str(&format!("{pad}invoke {v} {m}")),
            Stmt::Jump(l, _) => out.push_str(&format!("{pad}jump {l}")),
            Stmt::Handle(h, k, ret, rest) => {
                out.push_str(&format!("{pad}handle {h} {k} as {ret};\n"));
                rest.render(indent, out);
            }
            Stmt::Perform(op, xs, k) => out.push_str(&format!("{pad}perform {op}({}) {k}", commas(xs))),
            Stmt::Extern(op, args, branches) => {
                let args = if args.is_empty() { String::new() } else { format!("({})", commas(args)) };
                match branches.as_slice() {
                    [] => out.push_str(&format!("{pad}extern {op}{args} {{}}")),
                    // the usual case reads as a binding: what follows stays
                    // at this indentation, and the brace closes at the end
                    [b] => {
                        out.push_str(&format!("{pad}extern {op}{args} {{ ({}) ⇒\n", commas(&b.params)));
                        b.body.render(indent, out);
                        out.push_str(" }");
                    }
                    bs => {
                        out.push_str(&format!("{pad}extern {op}{args} {{\n"));
                        for (i, b) in bs.iter().enumerate() {
                            out.push_str(&format!("{pad}  ({}) ⇒\n", commas(&b.params)));
                            b.body.render(indent + 2, out);
                            out.push_str(if i + 1 < bs.len() { ",\n" } else { " }" });
                        }
                    }
                }
            }
        }
    }
}

impl fmt::Display for Stmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        self.render(0, &mut out);
        f.write_str(&out)
    }
}

impl fmt::Display for Program {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, d) in self.definitions.iter().enumerate() {
            if i > 0 {
                f.write_str("\n\n")?;
            }
            writeln!(f, "define {} : ({}) =", d.label, commas(&d.params))?;
            let mut body = String::new();
            d.body.render(1, &mut body);
            f.write_str(&body)?;
        }
        Ok(())
    }
}
