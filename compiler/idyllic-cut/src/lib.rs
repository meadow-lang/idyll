//! Cut, the upper IR of the back end Idyll shares with Meadow, and Idyll's
//! lowering to it.
//!
//! The contract is `docs/CUT.md` in the Meadow repository: a program as text,
//! typed by representations, which the back end's reference interpreter runs
//! and Glade and Silo compile.  This crate holds Cut as data ([`Program`] and
//! what it is made of), prints it as that text ([`print`]), and lowers a
//! checked Idyll program to it ([`lower`]).  The printer is the only place
//! that knows the concrete syntax, so that following the spec as it settles
//! is a change to one file.

pub mod lower;
pub mod print;

use std::fmt;

/// How a value is represented at run time: the only types Cut has.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Rep {
    I64,
    /// a byte: what `stringByteAt` gives, which `toInt` makes an `i64`
    U8,
    Bool,
    /// a Unicode scalar value: what `stringToChars` gives and
    /// `charsToString` takes, which `charCode` and `charFromCode` go between
    /// it and an `i64`
    Char,
    Unit,
    Str,
    Ptr,
    /// a descriptor: what a representation variable is, at run time
    Desc,
    /// a representation variable, `'a`
    Var(String),
    /// boxed, and self-describing at run time
    Any,
}

/// A flat, globally unique name: `idyll:App/Math.double`.  Kept as its parts;
/// [`fmt::Display`] writes it as the spec spells it, quoting a segment that
/// is not a plain identifier.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Symbol {
    pub lang: String,
    /// the package, with its version if it has one: `Json@1.2.0`
    pub package: String,
    /// the module path, then the name
    pub path: Vec<String>,
}

impl Symbol {
    /// An Idyll name: its package, and its path there (`Math.double`, with
    /// the segments split at `.` unless a segment is an operator).
    pub fn idyll(package: &str, path: &str) -> Symbol {
        Symbol { lang: "idyll".into(), package: package.into(), path: split_path(path) }
    }

    /// The symbol of something declared under this one: a constructor under
    /// its type, an operation under its effect.
    pub fn child(&self, name: &str) -> Symbol {
        let mut path = self.path.clone();
        path.push(name.into());
        Symbol { path, ..self.clone() }
    }
}

/// `Math.double` as its segments, and `List.::` as `List` and `::`: the
/// leading segments are identifiers (modules, a type), and whatever is left
/// is the name, which may be an operator with dots of its own.
fn split_path(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = path;
    while let Some((head, tail)) = rest.split_once('.') {
        if !is_ident(head) || tail.is_empty() {
            break;
        }
        out.push(head.to_string());
        rest = tail;
    }
    out.push(rest.to_string());
    out
}

fn is_ident(s: &str) -> bool {
    let mut cs = s.chars();
    cs.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A segment of a symbol as it is written: as it is if it is a plain
/// identifier, quoted if not.
pub fn segment(seg: &str) -> String {
    if is_ident(seg) {
        seg.to_string()
    } else {
        format!("\"{}\"", seg.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}/", self.lang, self.package)?;
        for (i, seg) in self.path.iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            f.write_str(&segment(seg))?;
        }
        Ok(())
    }
}

/// A binder: a name, and how its value is represented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binder {
    pub name: String,
    pub rep: Rep,
}

/// What makes a value.
#[derive(Clone, Debug, PartialEq)]
pub enum Producer {
    Var(String),
    /// a top-level value, by its symbol
    Val(Symbol),
    Int(i64),
    Str(String),
    /// a character, `'a'`
    Char(char),
    Bool(bool),
    Unit,
    /// a descriptor for a representation known when compiled
    Desc(Rep),
    /// a declared constructor, applied
    Con(Symbol, Vec<Producer>),
    /// the IR's own tuple
    Tuple(Vec<Producer>),
    /// `μ k. s`
    Mu(String, Box<Statement>),
    /// codata: an object with methods
    Cocase(Vec<Method>),
    Array(Vec<Producer>),
}

/// A method of an object: `apply(x: i64; k) => s`.
#[derive(Clone, Debug, PartialEq)]
pub struct Method {
    pub name: String,
    pub params: Vec<Binder>,
    pub conts: Vec<String>,
    pub body: Statement,
}

/// What receives a value.
#[derive(Clone, Debug, PartialEq)]
pub enum Consumer {
    Var(String),
    Halt,
    /// `μ̃ x: rep. s`
    MuTilde(Binder, Box<Statement>),
    /// take data apart: an arm for each constructor
    Case(Vec<Arm>),
    /// call a method of the object received
    Method(String, Vec<Producer>, Vec<Consumer>),
}

/// An arm of a `case`: a constructor (or the IR's tuple), its fields bound.
#[derive(Clone, Debug, PartialEq)]
pub struct Arm {
    /// `None` for `#tuple`
    pub ctor: Option<Symbol>,
    pub fields: Vec<Binder>,
    pub body: Statement,
}

/// A producer meeting a consumer, or something that ends the same way.
#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    Cut(Producer, Consumer),
    /// a top-level definition, called
    Call(Symbol, Vec<Producer>, Vec<Consumer>),
    Prim(String, Vec<Producer>, Vec<Consumer>),
    Let(Binder, Producer, Box<Statement>),
    Handle(Box<Handle>),
    /// `perform E.op(p, ..; c)`
    Perform(Symbol, Vec<Producer>, Consumer),
    Error(String),
}

/// `handle { clauses; return(..) } in μ b. body ; cont`
#[derive(Clone, Debug, PartialEq)]
pub struct Handle {
    pub clauses: Vec<Clause>,
    /// `return(x: rep; k) => s`
    pub ret: (Binder, String, Statement),
    pub body_cont: String,
    pub body: Statement,
    pub cont: Consumer,
}

/// `E.op(x: rep, ..; r, k) => s`
#[derive(Clone, Debug, PartialEq)]
pub struct Clause {
    pub op: Symbol,
    pub params: Vec<Binder>,
    pub resumption: String,
    pub cont: String,
    pub body: Statement,
}

/// What becomes of the entry's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    None,
    Str,
}

/// `data S <'a> { K(rep, ..); .. }`
#[derive(Clone, Debug, PartialEq)]
pub struct DataDecl {
    pub symbol: Symbol,
    pub rep_vars: Vec<String>,
    pub ctors: Vec<(String, Vec<Rep>)>,
}

/// `effect S { @many op(rep, ..) -> rep; .. }`
#[derive(Clone, Debug, PartialEq)]
pub struct EffectDecl {
    pub symbol: Symbol,
    pub ops: Vec<OpDecl>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpDecl {
    pub name: String,
    pub many: bool,
    pub params: Vec<Rep>,
    pub result: Rep,
}

/// An effect variable's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Effect,
    ManyEffect,
}

/// `def S <'a = d; e: Effect> (params; conts) = body`
#[derive(Clone, Debug, PartialEq)]
pub struct Def {
    pub symbol: Symbol,
    /// each representation variable, with the descriptor parameter that
    /// carries it
    pub rep_vars: Vec<(String, String)>,
    pub effect_vars: Vec<(String, Kind)>,
    pub params: Vec<Binder>,
    pub conts: Vec<String>,
    pub body: Statement,
}

/// `val S : rep = s`
#[derive(Clone, Debug, PartialEq)]
pub struct Val {
    pub symbol: Symbol,
    pub rep: Rep,
    pub body: Statement,
}

/// A whole program, as one text.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub entry: Option<Symbol>,
    pub answer: Answer,
    pub datas: Vec<DataDecl>,
    pub roles: Vec<(String, Symbol)>,
    /// each operation the program may leave unhandled, with the runtime's
    /// operation that does it
    pub natives: Vec<(Symbol, String)>,
    pub effects: Vec<EffectDecl>,
    /// in the order they are computed
    pub vals: Vec<Val>,
    pub defs: Vec<Def>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_are_written_as_the_spec_spells_them() {
        assert_eq!(Symbol::idyll("App", "double").to_string(), "idyll:App/double");
        assert_eq!(Symbol::idyll("App", "Math.double").to_string(), "idyll:App/Math.double");
        assert_eq!(Symbol::idyll("Prelude", "List").child("::").to_string(), "idyll:Prelude/List.\"::\"");
        assert_eq!(Symbol::idyll("Prelude", "Nat.x'").to_string(), "idyll:Prelude/Nat.\"x'\"");
        assert_eq!(Symbol::idyll("Json@1.2.0-alpha.1", "parse").to_string(), "idyll:Json@1.2.0-alpha.1/parse");
        // an operator with dots of its own stays one segment
        assert_eq!(Symbol::idyll("App", "Ops....").path, ["Ops", "..."]);
    }
}
