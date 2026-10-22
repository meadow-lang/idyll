//! Cut as text, in the syntax of `docs/CUT.md` (draft v0).  The spec's
//! grammar is still for reading, and the back end's parser will settle the
//! exact rules; until then this follows the spec's examples.  It is the only
//! place that knows the concrete syntax.

use super::*;
use idyllic_builtins::Width;
use std::fmt::Write;

impl fmt::Display for Rep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rep::I64 => f.write_str("i64"),
            Rep::Bool => f.write_str("bool"),
            Rep::Unit => f.write_str("unit"),
            Rep::Str => f.write_str("str"),
            Rep::Word(w) => f.write_str(match w {
                Width::I8 => "i8",
                Width::I16 => "i16",
                Width::I32 => "i32",
                Width::U8 => "u8",
                Width::U16 => "u16",
                Width::U32 => "u32",
                Width::U64 => "u64",
            }),
            Rep::F64 => f.write_str("f64"),
            Rep::F32 => f.write_str("f32"),
            Rep::Char => f.write_str("char"),
            Rep::Ptr => f.write_str("ptr"),
            Rep::Desc => f.write_str("desc"),
            Rep::Var(a) => write!(f, "'{a}"),
            Rep::Any => f.write_str("any"),
        }
    }
}

impl fmt::Display for Binder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name, self.rep)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Kind::Effect => "Effect",
            Kind::ManyEffect => "ManyEffect",
        })
    }
}

fn commas<T: fmt::Display>(xs: &[T]) -> String {
    xs.iter().map(T::to_string).collect::<Vec<_>>().join(", ")
}

/// `(values; continuations)`, with the continuations' representation
/// written: they are `ptr`, as every continuation is.
fn params(values: &[Binder], conts: &[String]) -> String {
    let conts: Vec<String> = conts.iter().map(|k| format!("{k}: ptr")).collect();
    format!("({}; {})", commas(values), conts.join(", "))
}

/// A character as Cut's text reads it back, and as Meadow's Cut package
/// writes one: its code point, `'\u{61}'`.
fn character(c: char) -> String {
    format!("'\\u{{{:x}}}'", c as u32)
}

fn string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

struct Printer {
    out: String,
}

impl Printer {
    fn producer(&mut self, p: &Producer, indent: usize) {
        match p {
            Producer::Var(x) => self.out.push_str(x),
            Producer::Val(s) => write!(self.out, "{s}").unwrap(),
            Producer::Int(n) => write!(self.out, "{n}").unwrap(),
            Producer::Float(x) => self.out.push_str(&idyllic_builtins::num::fmt_float(*x)),
            Producer::Str(s) => self.out.push_str(&string(s)),
            Producer::Char(c) => self.out.push_str(&character(*c)),
            Producer::Bool(b) => write!(self.out, "{b}").unwrap(),
            Producer::Unit => self.out.push_str("unit"),
            Producer::Desc(r) => write!(self.out, "desc({r})").unwrap(),
            Producer::Con(k, args) => {
                write!(self.out, "{k}").unwrap();
                if !args.is_empty() {
                    self.args(args, indent);
                }
            }
            Producer::Tuple(args) => {
                self.out.push_str("#tuple");
                self.args(args, indent);
            }
            Producer::Array(args) => {
                self.out.push('[');
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(", ");
                    }
                    self.producer(a, indent);
                }
                self.out.push(']');
            }
            Producer::Mu(k, s) => {
                write!(self.out, "μ {k}.").unwrap();
                self.nested(s, indent + 1);
            }
            Producer::Cocase(methods) => {
                self.out.push_str("cocase {");
                for (i, m) in methods.iter().enumerate() {
                    if i > 0 {
                        self.out.push(';');
                    }
                    self.line(indent + 1);
                    write!(self.out, "{}{} =>", m.name, params(&m.params, &m.conts)).unwrap();
                    self.nested(&m.body, indent + 2);
                }
                self.line(indent);
                self.out.push('}');
            }
        }
    }

    fn args(&mut self, args: &[Producer], indent: usize) {
        self.out.push('(');
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            self.producer(a, indent);
        }
        self.out.push(')');
    }

    /// `(p, ..; c, ..)`
    fn call(&mut self, args: &[Producer], conts: &[Consumer], indent: usize) {
        self.out.push('(');
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            self.producer(a, indent);
        }
        self.out.push_str("; ");
        for (i, c) in conts.iter().enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            self.consumer(c, indent);
        }
        self.out.push(')');
    }

    fn consumer(&mut self, c: &Consumer, indent: usize) {
        match c {
            Consumer::Var(k) => self.out.push_str(k),
            Consumer::Halt => self.out.push_str("halt"),
            Consumer::MuTilde(x, s) => {
                write!(self.out, "μ̃ {x}.").unwrap();
                self.nested(s, indent + 1);
            }
            Consumer::Case(arms) => {
                self.out.push_str("case {");
                for (i, arm) in arms.iter().enumerate() {
                    if i > 0 {
                        self.out.push(';');
                    }
                    self.line(indent + 1);
                    match &arm.ctor {
                        Some(k) => write!(self.out, "{k}").unwrap(),
                        None => self.out.push_str("#tuple"),
                    }
                    if !arm.fields.is_empty() || arm.ctor.is_none() {
                        write!(self.out, "({})", commas(&arm.fields)).unwrap();
                    }
                    self.out.push_str(" =>");
                    self.nested(&arm.body, indent + 2);
                }
                self.line(indent);
                self.out.push('}');
            }
            Consumer::Method(m, args, conts) => {
                self.out.push_str(m);
                self.call(args, conts, indent);
            }
        }
    }

    /// A statement on a line of its own, one level in.
    fn nested(&mut self, s: &Statement, indent: usize) {
        self.line(indent);
        self.statement(s, indent);
    }

    fn line(&mut self, indent: usize) {
        self.out.push('\n');
        self.out.push_str(&"  ".repeat(indent));
    }

    fn statement(&mut self, s: &Statement, indent: usize) {
        match s {
            Statement::Cut(p, c) => {
                self.out.push('<');
                self.producer(p, indent);
                self.out.push_str(" | ");
                self.consumer(c, indent);
                self.out.push('>');
            }
            Statement::Call(f, args, conts) => {
                write!(self.out, "{f}").unwrap();
                self.call(args, conts, indent);
            }
            Statement::Prim(op, args, conts) => {
                write!(self.out, "prim {op}").unwrap();
                self.call(args, conts, indent);
            }
            Statement::Let(x, p, body) => {
                write!(self.out, "let {x} = ").unwrap();
                self.producer(p, indent);
                self.out.push_str(" in");
                self.nested(body, indent);
            }
            Statement::Perform(op, args, c) => {
                write!(self.out, "perform {op}").unwrap();
                self.call(args, std::slice::from_ref(c), indent);
            }
            Statement::Error(msg) => write!(self.out, "error {}", string(msg)).unwrap(),
            Statement::Handle(h) => {
                self.out.push_str("handle {");
                for c in &h.clauses {
                    self.line(indent + 1);
                    write!(self.out, "{}{} =>", c.op, params(&c.params, &[c.resumption.clone(), c.cont.clone()])).unwrap();
                    self.nested(&c.body, indent + 2);
                    self.out.push(';');
                }
                let (x, k, body) = &h.ret;
                self.line(indent + 1);
                write!(self.out, "return{} =>", params(std::slice::from_ref(x), std::slice::from_ref(k))).unwrap();
                self.nested(body, indent + 2);
                self.line(indent);
                write!(self.out, "}} in μ {}.", h.body_cont).unwrap();
                self.nested(&h.body, indent + 1);
                self.line(indent);
                self.out.push_str("; ");
                self.consumer(&h.cont, indent);
            }
        }
    }
}

/// A program as the text the back end reads.
pub fn program(p: &Program) -> String {
    let mut out = String::from("cut 0\n");
    if let Some(entry) = &p.entry {
        writeln!(out, "entry {entry}").unwrap();
        writeln!(out, "answer {}", if p.answer == Answer::Str { "str" } else { "none" }).unwrap();
    }
    for d in &p.datas {
        let vars = if d.rep_vars.is_empty() {
            String::new()
        } else {
            format!(" <{}>", d.rep_vars.iter().map(|a| format!("'{a}")).collect::<Vec<_>>().join(", "))
        };
        let ctors: Vec<String> = d
            .ctors
            .iter()
            .map(|(k, fields)| {
                let k = segment(k);
                if fields.is_empty() {
                    k
                } else {
                    format!("{k}({})", commas(fields))
                }
            })
            .collect();
        writeln!(out, "\ndata {}{vars} {{ {} }}", d.symbol, ctors.join("; ")).unwrap();
    }
    if !p.roles.is_empty() {
        out.push_str("\nroles {\n");
        for (role, k) in &p.roles {
            writeln!(out, "  {role} = {k}").unwrap();
        }
        out.push_str("}\n");
    }
    if !p.natives.is_empty() {
        out.push_str("\nnative {\n");
        for (op, runtime) in &p.natives {
            writeln!(out, "  {op} = {runtime}").unwrap();
        }
        out.push_str("}\n");
    }
    for e in &p.effects {
        let ops: Vec<String> = e
            .ops
            .iter()
            .map(|op| {
                let name = segment(&op.name);
                format!("{}{name}({}) -> {}", if op.many { "@many " } else { "" }, commas(&op.params), op.result)
            })
            .collect();
        writeln!(out, "\neffect {} {{ {} }}", e.symbol, ops.join("; ")).unwrap();
    }
    for v in &p.vals {
        let mut pr = Printer { out: String::new() };
        pr.nested(&v.body, 1);
        writeln!(out, "\nval {} : {} ={}", v.symbol, v.rep, pr.out).unwrap();
    }
    for d in &p.defs {
        let mut head = Vec::new();
        if !d.rep_vars.is_empty() {
            head.push(d.rep_vars.iter().map(|(a, desc)| format!("'{a} = {desc}")).collect::<Vec<_>>().join(", "));
        }
        if !d.effect_vars.is_empty() {
            head.push(d.effect_vars.iter().map(|(e, k)| format!("{e}: {k}")).collect::<Vec<_>>().join(", "));
        }
        let generics = if head.is_empty() { String::new() } else { format!(" <{}>", head.join("; ")) };
        let mut pr = Printer { out: String::new() };
        pr.nested(&d.body, 1);
        writeln!(out, "\ndef {}{generics} {} ={}", d.symbol, params(&d.params, &d.conts), pr.out).unwrap();
    }
    out
}

impl fmt::Display for Program {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&program(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_reads_as_the_spec_writes_one() {
        let nat = Symbol::idyll("App", "Nat");
        let double = Symbol::idyll("App", "double");
        let n = |x: &str| Binder { name: x.into(), rep: Rep::I64 };
        let p = Program {
            entry: Some(Symbol::idyll("App", "main")),
            answer: Answer::None,
            datas: vec![DataDecl { symbol: nat.clone(), rep_vars: vec![], ctors: vec![("Z".into(), vec![]), ("S".into(), vec![Rep::Ptr])] }],
            roles: vec![("nil".into(), Symbol::idyll("Prelude", "List").child("Nil"))],
            natives: vec![(Symbol::idyll("Prelude", "Console.putStr"), "Console.writeOutput".into())],
            effects: vec![],
            vals: vec![Val {
                symbol: Symbol::idyll("App", "greeting"),
                rep: Rep::Str,
                body: Statement::Cut(Producer::Str("hello".into()), Consumer::Halt),
            }],
            defs: vec![Def {
                symbol: double.clone(),
                rep_vars: vec![],
                effect_vars: vec![],
                params: vec![n("n")],
                conts: vec!["k".into()],
                body: Statement::Prim(
                    "add".into(),
                    vec![Producer::Var("n".into()), Producer::Var("n".into())],
                    vec![Consumer::Var("k".into())],
                ),
            }],
        };
        let text = program(&p);
        assert!(text.starts_with("cut 0\nentry idyll:App/main\nanswer none\n"), "{text}");
        assert!(text.contains("data idyll:App/Nat { Z; S(ptr) }"), "{text}");
        assert!(text.contains("  nil = idyll:Prelude/List.Nil"), "{text}");
        assert!(text.contains("  idyll:Prelude/Console.putStr = Console.writeOutput"), "{text}");
        assert!(text.contains("val idyll:App/greeting : str =\n  <\"hello\" | halt>"), "{text}");
        assert!(text.contains("def idyll:App/double (n: i64; k: ptr) =\n  prim add(n, n; k)"), "{text}");
    }
}
