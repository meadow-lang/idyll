//! What a block of code is at each stage of the compiler: the REPL's `:ir`.
//!
//! The block is compiled as an entry of a session would be, with the
//! session's declarations in scope, and each stage's form of it is rendered
//! as text: the tokens as layout leaves them, the syntax tree, the HIR, the
//! checker's Core, AxCut as lowered and as inlined, LLVM IR, and Cut, the
//! shared back end's upper IR.  The early stages show the block alone; Core
//! shows the block's definitions and expression; the IRs after it show the
//! whole program the block makes, which is what is lowered: the block, and
//! whatever of the session and the prelude it uses.
//!
//! A block of declarations alone has nothing to run, and a back end leaves
//! out what nothing uses; so it is lowered as a program whose expression is
//! `()`, with its definitions kept whether or not anything calls them (see
//! [`idyllic_typecheck::TypeCheckResult::roots`]).

use crate::pipeline::{self, Deps, Front, Options, PackageSource, Stage};
use idyllic_diagnostics::{self as diagnostics, Diagnostic};
use idyllic_lexer::token::Token;
use idyllic_span::LineIndex;
use idyllic_typecheck::{core::Core, DeclKind};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::rc::Rc;

/// Every stage, by the name `:ir` knows it by, with what it is.
pub const STAGES: &[(&str, &str)] = &[
    ("tokens", "the lexer's tokens, laid out: «{» «;» «}» are what layout inserts"),
    ("ast", "the parser's syntax tree"),
    ("hir", "names resolved to uniques, definitions grouped by what they use"),
    ("core", "the checker's elaborated Core: implicit arguments, boxes and grades explicit"),
    ("axcut", "AxCut, the sequent-calculus IR the machine runs, substitutions inferred"),
    ("inlined", "the same, inlined: what the native compiler is given"),
    ("llvm", "LLVM IR, from the inlined AxCut"),
    ("cut", "Cut, the shared back end's upper IR (docs/CUT.md in Meadow)"),
];

/// What [`inspect`] makes of a block.
pub struct Inspection {
    /// each stage asked for, in the order of [`STAGES`], with its text, or
    /// why there is none
    pub stages: Vec<(&'static str, Result<String, String>)>,
    /// the block's diagnostics, rendered, if it has any errors; the stages
    /// after the HIR need a block that checks
    pub errors: Option<String>,
}

/// The block `entry`, after the declarations `earlier` (a session's), at
/// each of the `wanted` stages (every stage, if none).
pub fn inspect(earlier: &str, entry: &str, opts: &Options, wanted: &[&str]) -> Inspection {
    let wants = |stage: &str| wanted.is_empty() || wanted.contains(&stage);
    let entry = entry.trim_end();
    let mut stages: Vec<(&'static str, Result<String, String>)> = Vec::new();
    let mut put = |name: &str, text: Result<String, String>| {
        let name = STAGES.iter().find(|(n, _)| *n == name).map(|(n, _)| *n).expect("a stage");
        stages.push((name, text));
    };

    // the block alone
    let lexed = idyllic_lexer::tokenize(entry);
    let laid_out = idyllic_layout::layout(&LineIndex::new(entry), &lexed.tokens);
    if wants("tokens") {
        put("tokens", Ok(tokens(entry, &laid_out.tokens)));
    }
    if wants("ast") {
        put(
            "ast",
            idyllic_parser::parse(entry.len(), &laid_out.tokens)
                .map(|program| tree(&format!("{program:?}")))
                .map_err(|_| "it does not parse".to_string()),
        );
    }

    // the block among the session's declarations
    let source = format!("{earlier}{entry}\n");
    let front = match front(&source, opts) {
        Ok(front) => front,
        Err(unread) => {
            let errors = render(&unread.diagnostics, &unread.sources, earlier.len(), opts);
            return Inspection { stages, errors: Some(errors) };
        }
    };
    let ours = |span: Option<idyllic_span::Span>| span.is_some_and(|s| s.start >= earlier.len() && s.start < source.len());
    if wants("hir") {
        let m = &front.module;
        let mut out: Vec<String> = Vec::new();
        for d in m.datas.iter().filter(|d| ours(front.spans.get(d.id))) {
            out.push(tree(&format!("{d:?}")));
        }
        for e in m.effects.iter().filter(|e| ours(front.spans.get(e.id))) {
            out.push(tree(&format!("{e:?}")));
        }
        let mine = |b: &idyllic_hir::Bind| ours(front.spans.get(b.id));
        for g in m.groups.iter().filter(|g| g.binds().iter().any(|b| mine(b))) {
            out.push(tree(&format!("{g:?}")));
        }
        if let Some(body) = m.body.as_ref().filter(|b| ours(front.spans.get(b.id))) {
            out.push(tree(&format!("{body:?}")));
        }
        put("hir", Ok(out.join("\n\n")));
    }
    if front.diagnostics.iter().any(Diagnostic::is_error) {
        let errors = render(&front.diagnostics, &front.sources, earlier.len(), opts);
        return Inspection { stages, errors: Some(errors) };
    }

    // a block with nothing to run runs `()`, keeping its definitions
    let declares_only = front.checked.body_type.is_none();
    let front = if declares_only {
        match self::front(&format!("{source}()\n"), opts) {
            Ok(front) => front,
            Err(unread) => {
                let errors = render(&unread.diagnostics, &unread.sources, earlier.len(), opts);
                return Inspection { stages, errors: Some(errors) };
            }
        }
    } else {
        front
    };
    let Front { diagnostics, sources, spans, mut checked, decls, .. } = front;
    let entry_defs: Vec<_> = checked
        .catalog
        .iter()
        .filter(|d| !d.prelude && d.kind == DeclKind::Value && ours(spans.get(d.node)))
        .cloned()
        .collect();
    checked.roots = entry_defs.iter().filter_map(|d| d.unique).collect::<HashSet<usize>>();
    if wants("core") {
        put("core", Ok(core(&checked, &entry_defs, !declares_only)));
    }
    if !(wants("axcut") || wants("inlined") || wants("llvm") || wants("cut")) {
        return Inspection { stages, errors: None };
    }
    // what it uses of Std is lowered with it
    let std_package = crate::stdlib::compiled();
    let linking = pipeline::to_link(&checked, &[&*std_package]);
    if !linking.is_empty() {
        idyllic_typecheck::packages::link(&mut checked, &linking);
    }
    let mut lowered = pipeline::finish(checked, diagnostics, sources, decls, None, Stage::Lower);
    let failed = match &lowered.evaluation {
        Some(pipeline::Evaluation::Failed(why)) => Some(format!("it could not be lowered: {why}")),
        _ => None,
    };
    let text = |p: Option<&idyllic_seq::Program>| match (&failed, p) {
        (Some(why), _) => Err(why.clone()),
        (None, Some(p)) => Ok(p.to_string()),
        (None, None) => Err("it was not lowered".to_string()),
    };
    if wants("axcut") {
        put("axcut", text(lowered.axcut.as_ref()));
    }
    if wants("inlined") {
        put("inlined", text(lowered.native.as_ref()));
    }
    if wants("llvm") {
        put("llvm", text(lowered.native.as_ref()).and_then(|_| {
            let native = lowered.native.as_ref().expect("lowered");
            idyllic_llvm::emit(native, lowered.result_kind).map_err(|why| format!("not compiled to native code: {why}"))
        }));
    }
    if wants("cut") {
        let checked = lowered.checked.as_mut().expect("checked");
        put("cut", idyllic_cut::lower::lower(checked, "Main").map(|p| p.to_string()).map_err(|why| why.join("\n")));
    }
    Inspection { stages, errors: None }
}

fn front(source: &str, opts: &Options) -> Result<Front, pipeline::Output> {
    pipeline::with_std("Main", Deps::default(), |deps| {
        pipeline::front(&PackageSource::single("Main", "<interactive>", source), deps, opts, false)
    })
}

/// The diagnostics of the block, which starts at `from`, rendered.
fn render(all: &[Diagnostic], sources: &idyllic_span::SourceMap, from: usize, opts: &Options) -> String {
    let ours: Vec<Diagnostic> =
        all.iter().filter(|d| d.labels.is_empty() || d.labels.iter().any(|l| l.span.start >= from)).cloned().collect();
    diagnostics::render_map(&ours, sources, opts.style)
}

/// The tokens, a line of them for each line of the source they came from.
fn tokens(src: &str, tokens: &[idyllic_span::Spanned<Token>]) -> String {
    let lines = LineIndex::new(src);
    let mut out = String::new();
    let mut line = None;
    for t in tokens {
        // what layout closes a line with goes at its end, whatever its span
        let closes = matches!(t.node, Token::VSemi | Token::VRBrace);
        let here = if closes { line.unwrap_or(0) } else { lines.line_col(t.span.start.min(src.len())).0 };
        match line {
            Some(l) if l == here => out.push(' '),
            Some(_) => out.push('\n'),
            None => {}
        }
        line = Some(here);
        match &t.node {
            Token::VLBrace => out.push_str("«{»"),
            Token::VSemi => out.push_str("«;»"),
            Token::VRBrace => out.push_str("«}»"),
            other => {
                let _ = write!(out, "{other}");
            }
        }
    }
    out
}

/// The block's definitions, each with its type, and its expression (if it
/// has one), as the checker elaborated them.
fn core(checked: &idyllic_typecheck::TypeCheckResult, defs: &[idyllic_typecheck::Declared], has_body: bool) -> String {
    let mut out = String::new();
    let mut lets: Vec<(usize, Rc<Core>)> = Vec::new();
    let mut body = checked.program.clone();
    for u in &checked.toplevel {
        match body.as_deref() {
            Some(Core::Let(_, rhs, rest)) => {
                lets.push((*u, rhs.clone()));
                body = Some(rest.clone());
            }
            _ => break,
        }
    }
    for d in defs {
        let Some(u) = d.unique else { continue };
        let value = lets
            .iter()
            .find(|(v, _)| *v == u)
            .map(|(_, c)| c.clone())
            .or_else(|| checked.rec_defs.iter().find(|r| r.unique == u).map(|r| r.core.clone()));
        let _ = writeln!(out, "{} : {}", d.name, d.ty);
        match value {
            Some(v) => {
                let _ = writeln!(out, "{} = {v}\n", d.name);
            }
            // a type former, say: erased, and kept nowhere
            None => out.push('\n'),
        }
    }
    if let (true, Some(body)) = (has_body, body) {
        let _ = writeln!(out, "{body}");
    }
    out.trim_end().to_string()
}

/// A `{:?}` dump, made readable: spans and node ids left out, a `Spanned`
/// shown as what it holds, an identifier as `name#unique`, and what fits on
/// a line put on one.
fn tree(debug: &str) -> String {
    let mut reader = Reader { chars: debug.chars().collect(), at: 0 };
    let mut out = String::new();
    while let Some(node) = reader.value() {
        if let Some(node) = tidy(node) {
            print(&node, 0, &mut out);
            out.push('\n');
        }
        reader.skip(',');
    }
    out.trim_end().to_string()
}

/// A value, as `{:?}` writes it.
#[derive(Debug, Clone)]
enum Node {
    /// a number, a string, a unit variant, ...
    Atom(String),
    /// `Name(a, b)`, and `(a, b)` with no name
    Call(String, Vec<Node>),
    /// `Name { k: v }`, and `{k: v}` (a map) with no name
    Struct(String, Vec<(String, Node)>),
    List(Vec<Node>),
}

struct Reader {
    chars: Vec<char>,
    at: usize,
}

impl Reader {
    fn space(&mut self) {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.space();
        self.chars.get(self.at).copied()
    }

    fn skip(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Option<Node> {
        let c = self.peek()?;
        match c {
            '"' | '\'' => Some(Node::Atom(self.quoted(c))),
            '[' => {
                self.at += 1;
                Some(Node::List(self.items(']')))
            }
            '(' => {
                self.at += 1;
                Some(Node::Call(String::new(), self.items(')')))
            }
            '{' => {
                self.at += 1;
                Some(Node::Struct(String::new(), self.fields()))
            }
            ')' | ']' | '}' | ',' | ':' => None,
            _ => {
                let start = self.at;
                while self.chars.get(self.at).is_some_and(|c| !"()[]{},: \n\t\"'".contains(*c)) {
                    self.at += 1;
                }
                let word: String = self.chars[start..self.at].iter().collect();
                // `a::b` paths are one word
                if self.chars.get(self.at) == Some(&':') && self.chars.get(self.at + 1) == Some(&':') {
                    self.at += 2;
                    let rest = self.value().map(|n| match n {
                        Node::Atom(a) => a,
                        _ => String::new(),
                    });
                    return Some(Node::Atom(format!("{word}::{}", rest.unwrap_or_default())));
                }
                if self.skip('(') {
                    Some(Node::Call(word, self.items(')')))
                } else if self.peek() == Some('{') {
                    self.at += 1;
                    Some(Node::Struct(word, self.fields()))
                } else {
                    Some(Node::Atom(word))
                }
            }
        }
    }

    fn quoted(&mut self, q: char) -> String {
        let start = self.at;
        self.at += 1;
        while let Some(&c) = self.chars.get(self.at) {
            self.at += 1;
            if c == '\\' {
                self.at += 1;
            } else if c == q {
                break;
            }
        }
        self.chars[start..self.at.min(self.chars.len())].iter().collect()
    }

    fn items(&mut self, close: char) -> Vec<Node> {
        let mut items = Vec::new();
        while !self.skip(close) {
            match self.value() {
                Some(v) => items.push(v),
                None if self.peek().is_none() => break,
                None => self.at += 1,
            }
            self.skip(',');
        }
        items
    }

    fn fields(&mut self) -> Vec<(String, Node)> {
        let mut fields = Vec::new();
        while !self.skip('}') {
            let Some(key) = self.value() else {
                if self.peek().is_none() {
                    break;
                }
                self.at += 1;
                continue;
            };
            let key = match key {
                Node::Atom(a) => a,
                other => {
                    let mut s = String::new();
                    print(&other, 0, &mut s);
                    s
                }
            };
            if self.skip(':') {
                if let Some(v) = self.value() {
                    fields.push((key, v));
                }
            } else {
                // a set's element
                fields.push((String::new(), Node::Atom(key)));
            }
            self.skip(',');
        }
        fields
    }
}

/// Leave out what says where rather than what: spans and node ids.
fn tidy(node: Node) -> Option<Node> {
    match node {
        Node::Struct(name, _) if name == "Span" => None,
        Node::Call(name, _) if name == "NodeId" => None,
        Node::Struct(name, fields) if name == "Spanned" => {
            fields.into_iter().find(|(k, _)| k == "node").and_then(|(_, v)| tidy(v))
        }
        Node::Struct(name, fields) if name == "Ident" => {
            let get = |k: &str| fields.iter().find(|(f, _)| f == k).map(|(_, v)| v.clone());
            let text = match get("text") {
                Some(Node::Atom(t)) => t.trim_matches('"').to_string(),
                _ => "?".to_string(),
            };
            Some(Node::Atom(match get("unique") {
                Some(Node::Call(_, args)) => match args.first() {
                    Some(Node::Atom(u)) => format!("{text}#{u}"),
                    _ => text,
                },
                _ => text,
            }))
        }
        Node::Struct(name, fields) => {
            let fields: Vec<(String, Node)> = fields
                .into_iter()
                .filter(|(k, _)| k != "span" && k != "id")
                .filter_map(|(k, v)| Some((k, tidy(v)?)))
                .collect();
            Some(match fields.as_slice() {
                [] if !name.is_empty() => Node::Atom(name),
                // a wrapper: `Term { kind: .. }` is its kind
                [(k, _)] if k == "kind" => fields.into_iter().next().expect("one").1,
                _ => Node::Struct(name, fields),
            })
        }
        Node::Call(name, args) => Some(Node::Call(name, args.into_iter().filter_map(tidy).collect())),
        Node::List(items) => Some(Node::List(items.into_iter().filter_map(tidy).collect())),
        atom => Some(atom),
    }
}

/// How wide a line of a tree may be before it is broken.
const WIDTH: usize = 90;

fn flat(node: &Node) -> String {
    let join = |items: &[Node]| items.iter().map(flat).collect::<Vec<_>>().join(", ");
    match node {
        Node::Atom(a) => a.clone(),
        Node::Call(name, args) => format!("{name}({})", join(args)),
        Node::List(items) => format!("[{}]", join(items)),
        Node::Struct(name, fields) => {
            let fields: Vec<String> =
                fields.iter().map(|(k, v)| if k.is_empty() { flat(v) } else { format!("{k}: {}", flat(v)) }).collect();
            let gap = if name.is_empty() { "" } else { " " };
            format!("{name}{gap}{{ {} }}", fields.join(", "))
        }
    }
}

fn print(node: &Node, indent: usize, out: &mut String) {
    let one = flat(node);
    if indent + one.chars().count() <= WIDTH {
        out.push_str(&one);
        return;
    }
    let pad = " ".repeat(indent + 2);
    let close = " ".repeat(indent);
    let items = |open: &str, items: Vec<(Option<&str>, &Node)>, end: &str, out: &mut String| {
        out.push_str(open);
        out.push('\n');
        for (key, item) in items {
            out.push_str(&pad);
            if let Some(k) = key.filter(|k| !k.is_empty()) {
                out.push_str(k);
                out.push_str(": ");
            }
            print(item, indent + 2, out);
            out.push_str(",\n");
        }
        out.push_str(&close);
        out.push_str(end);
    };
    match node {
        Node::Atom(a) => out.push_str(a),
        Node::Call(name, args) => items(&format!("{name}("), args.iter().map(|a| (None, a)).collect(), ")", out),
        Node::List(list) => items("[", list.iter().map(|a| (None, a)).collect(), "]", out),
        Node::Struct(name, fields) => {
            let gap = if name.is_empty() { "" } else { " " };
            items(&format!("{name}{gap}{{"), fields.iter().map(|(k, v)| (Some(k.as_str()), v)).collect(), "}", out)
        }
    }
}
