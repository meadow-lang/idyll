//! The compiler written in Meadow, `front/`, checked against this one, pass
//! by pass, as MeadowBoot is checked against the Rust Meadow compiler.
//!
//! Each pass of `front/` writes what it made as text, and the same pass here
//! is written the same way; a program whose two texts differ is a difference
//! between the compilers, shown at its first token. The inputs are every
//! program of the compiler's corpora (`tests/corpus/`).
//!
//! `front/` is built once a run, natively (`meadow build --release`), with the
//! `meadow` on the path; with none, the tests say so and pass, so that a
//! machine without Meadow can still run the rest. `IDYLL_FRONT_MEADOW` names
//! another `meadow` to build it with.

use idyllic_lexer::token::Token;
use idyllic_span::LineIndex;

#[path = "support/tutorial_examples.rs"]
#[allow(dead_code)]
mod tutorial_examples;

#[path = "support/front.rs"]
#[allow(dead_code)]
mod harness;
use harness::*;

/// The tokens this compiler's lexer and layout make of `src`, as `front/`
/// writes them: each as the text it covers, the virtual ones as «{» «;» «}»,
/// a space between each two.
fn tokens(src: &str) -> String {
    let lexed = idyllic_lexer::tokenize(src);
    let laid = idyllic_layout::layout(&LineIndex::new(src), &lexed.tokens);
    laid.tokens
        .iter()
        .map(|t| match t.node {
            Token::VLBrace => "«{»".to_string(),
            Token::VSemi => "«;»".to_string(),
            Token::VRBrace => "«}»".to_string(),
            _ => src[t.span.start..t.span.end].to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn the_tokens_and_their_layout_agree() {
    agree("tokens", tokens);
}

#[test]
fn the_parses_agree() {
    agree("parse", parsed);
}

#[test]
fn the_renames_agree() {
    let source = idyllic::pipeline::PRELUDE;
    let prelude = program(source).expect("the prelude parses");
    agree_with(
        &["rename", "--prelude", "_prelude.txt"],
        &[("_prelude.txt", source)],
        |src| renamed(&prelude, src),
        renumbered,
    );
}

#[test]
fn the_types_agree() {
    let source = idyllic::pipeline::PRELUDE;
    let prelude = program(source).expect("the prelude parses");
    let std_dir = repo().join("lib/Std/src");
    let std_dir = std_dir.to_string_lossy();
    agree_with(
        &["types", "--prelude", "_prelude.txt", "--std", &std_dir],
        &[("_prelude.txt", source)],
        |src| typed(&prelude, src),
        |d| renumbered_by(d, '?'),
    );
}

/// The standard library, checked as the package it is, by both: each of
/// its declarations' types, and its errors (none).
#[test]
fn the_std_package_agrees() {
    let Ok(exe) = front() else {
        eprintln!("no front/ to check the Std package with");
        return;
    };
    let source = idyllic::pipeline::PRELUDE;
    let prelude = program(source).expect("the prelude parses");
    let dir = std::env::temp_dir().join(format!("idyll-front-std-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory");
    std::fs::write(dir.join("_prelude.txt"), source).expect("the prelude can be written");
    let std_dir = repo().join("lib/Std/src");
    let out = std::process::Command::new(&exe)
        .args(["package", "--prelude", "_prelude.txt", "Std"])
        .arg(&std_dir)
        .current_dir(&dir)
        .output()
        .expect("front/ runs");
    let _ = std::fs::remove_dir_all(&dir);
    let text = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    let theirs = text.strip_prefix("== Std\n").unwrap_or(&text).trim_end().to_string();
    // here: the package's modules, checked as `Std` is
    let programs: Vec<(Vec<String>, idyllic_ast::Program)> = idyllic::stdlib::MODULES
        .iter()
        .map(|(path, text)| {
            let path = if path.is_empty() { Vec::new() } else { path.split('.').map(String::from).collect() };
            (path, program(text).expect("a Std module parses"))
        })
        .collect();
    let modules: Vec<idyllic_rename::ModuleSource<'_>> =
        programs.iter().map(|(path, program)| idyllic_rename::ModuleSource { path: path.clone(), program }).collect();
    let result = idyllic_rename::rename_package("Std", &modules, &prelude, &[], idyllic_builtins::FIRST_USER_UNIQUE, false);
    assert!(result.errors.is_empty(), "Std renames");
    let module = idyllic_scc::scc_module(result.module);
    let checked = idyllic_typecheck::check_module(&module, result.spans.len(), false, &[]);
    let ours = renumbered_by(&checked_dump(&checked), '?');
    let theirs = renumbered_by(&theirs, '?');
    eprintln!("std: {} declarations", ours.lines().count());
    assert!(ours == theirs, "the Std package differs: {}", first_difference(&ours, &theirs));
}

#[test]
fn the_cut_agrees() {
    let source = idyllic::pipeline::PRELUDE;
    let prelude = program(source).expect("the prelude parses");
    agree_with(&["cut", "--prelude", "_prelude.txt"], &[("_prelude.txt", source)], |src| cut(&prelude, src), cut_canonical);
}

// --- the Cut, written out ------------------------------------------------------
//
// Both compilers' Cut as its text (`front/` builds it with the Cut package,
// this compiler with its own printer), compared once canonical: with no
// whitespace, which the two printers lay out differently; with the names
// the lowering made (`k_23`, `d_a_4`) numbered afresh in each declaration in
// the order they appear, since this compiler lowers the recursive
// definitions in a hash map's order, and so numbers them differently from
// run to run; and with the declarations sorted, for the same reason.

/// What this compiler lowers `src` to, after the prelude, as Cut's text.
fn cut(prelude: &idyllic_ast::Program, src: &str) -> String {
    let Some(p) = program(src) else { return "(error)".to_string() };
    let result = idyllic_rename::rename_module(&p, prelude);
    if !result.errors.is_empty() {
        return "(error)".to_string();
    }
    let module = idyllic_scc::scc_module(result.module);
    let mut checked = idyllic_typecheck::check_module(&module, result.spans.len(), false, &[]);
    if !checked.errors.is_empty() {
        return "(error)".to_string();
    }
    match idyllic_cut::lower::lower(&mut checked, "Main") {
        Ok(program) => program.to_string(),
        Err(why) => format!("(not lowered: {})", why.join("; ")),
    }
}

/// A Cut text in the canonical form described above.
fn cut_canonical(text: &str) -> String {
    if text.starts_with('(') {
        return text.to_string();
    }
    let starts = ["data ", "roles {", "native {", "effect ", "val ", "def "];
    let (mut header, mut decls): (Vec<&str>, Vec<Vec<&str>>) = (Vec::new(), Vec::new());
    for line in text.lines() {
        if starts.iter().any(|s| line.starts_with(s)) {
            decls.push(vec![line]);
        } else if let Some(d) = decls.last_mut() {
            d.push(line);
        } else {
            header.push(line);
        }
    }
    let squeeze = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let mut out: Vec<String> = decls.iter().map(|d| renamed_fresh(&squeeze(&d.join("\n")))).collect();
    out.sort();
    let mut all = vec![squeeze(&header.join("\n"))];
    all.extend(out);
    all.join("\n")
}

/// Each name of the form `hint_123` numbered by where it first appears:
/// `#0`, `#1`, ...
fn renamed_fresh(decl: &str) -> String {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let chars: Vec<char> = decl.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    while i < chars.len() {
        if word(chars[i]) && (i == 0 || !word(chars[i - 1])) {
            let start = i;
            while i < chars.len() && word(chars[i]) {
                i += 1;
            }
            let w: String = chars[start..i].iter().collect();
            let fresh = w.rsplit_once('_').is_some_and(|(h, n)| !h.is_empty() && !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
            if fresh {
                let n = seen.len();
                out.push_str(&format!("#{}", seen.entry(w).or_insert(n)));
            } else {
                out.push_str(&w);
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

// --- the types, written out ---------------------------------------------------
//
// As `front/src/Check.mw` writes them: a line each, sorted, for each of the
// program's declarations with its type (`name : type`), the type of its
// expression (`body : type`), and each error, by the name of its kind
// (`error: Mismatch`). Metavariables are numbered by first appearance on
// both sides.

/// What this compiler's checker makes of `src`, after the prelude, written
/// out; a program that names `Std` is checked against it, compiled, as a
/// package depends on it.
fn typed(prelude: &idyllic_ast::Program, src: &str) -> String {
    let Some(p) = program(src) else { return "(error)".to_string() };
    let std = idyllic::stdlib::compiled();
    let uses_std = src.contains("Std");
    let result = if uses_std {
        let modules = [idyllic_rename::ModuleSource { path: Vec::new(), program: &p }];
        let first = std.uniques.end.max(idyllic_builtins::FIRST_USER_UNIQUE);
        idyllic_rename::rename_package("Main", &modules, prelude, &[("Std", &std.names)], first, false)
    } else {
        idyllic_rename::rename_module(&p, prelude)
    };
    if !result.errors.is_empty() {
        return "(error)".to_string();
    }
    let module = idyllic_scc::scc_module(result.module);
    let deps: Vec<&idyllic_typecheck::packages::CompiledPackage> = if uses_std { vec![&*std] } else { Vec::new() };
    let checked = idyllic_typecheck::check_module(&module, result.spans.len(), false, &deps);
    checked_dump(&checked)
}

/// A checked module written out: its own declarations with their types, its
/// expression's type, and its errors by kind, a line each, sorted.
fn checked_dump(checked: &idyllic_typecheck::TypeCheckResult) -> String {
    let mut lines: Vec<String> =
        checked.catalog.iter().filter(|d| !d.prelude).map(|d| format!("{} : {}", d.name, d.ty)).collect();
    lines.extend(checked.body_type.iter().map(|ty| format!("body : {ty}")));
    for e in &checked.errors {
        let kind = format!("{:?}", e.kind);
        let name = kind.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("").to_string();
        lines.push(format!("error: {name}"));
    }
    lines.sort();
    lines.join("\n")
}

// --- the rename, written out --------------------------------------------------
//
// As `front/src/Hir.mw` writes it: the program's own part of the module, a line
// for each declaration, every name `text@unique`. `renumbered` then numbers
// the uniques in the order they first appear, on both sides, so that what is
// compared is which use is of which binder, not how each renamer counts.

/// What this compiler's renamer makes of `src`, after the prelude, written
/// out.
fn renamed(prelude: &idyllic_ast::Program, src: &str) -> String {
    let Some(p) = program(src) else { return "(error)".to_string() };
    let result = idyllic_rename::rename_module(&p, prelude);
    if !result.errors.is_empty() {
        return "(error)".to_string();
    }
    h::module(&result.module)
}

mod h {
    use idyllic_hir::*;

    std::thread_local! {
        /// the module's overloaded names, by node, while it is written out:
        /// each as front/'s HIR has it, `(overload x@u x@v)`
        static OVERLOADS: std::cell::RefCell<std::collections::HashMap<NodeId, Vec<usize>>> = Default::default();
    }

    /// The name at `node`, with every meaning it has there.
    fn ident_at(node: NodeId, x: &Ident) -> String {
        match OVERLOADS.with(|o| o.borrow().get(&node).cloned()) {
            Some(us) => sx(std::iter::once("overload".to_string()).chain(us.iter().map(|u| format!("{}@{u}", x.text)))),
            None => ident(x),
        }
    }

    fn sx<S: AsRef<str>>(parts: impl IntoIterator<Item = S>) -> String {
        let parts: Vec<String> = parts.into_iter().map(|s| s.as_ref().to_string()).collect();
        format!("({})", parts.join(" "))
    }

    fn opt<T>(m: &Option<T>, f: impl Fn(&T) -> String) -> String {
        m.as_ref().map_or("-".to_string(), f)
    }

    fn flag(yes: &str, b: bool) -> String {
        if b { yes } else { "-" }.to_string()
    }

    fn with<S: Into<String>>(head: impl IntoIterator<Item = S>, rest: Vec<String>) -> String {
        sx(head.into_iter().map(Into::into).chain(rest))
    }

    fn ident(x: &Ident) -> String {
        match x.unique {
            Some(u) => format!("{}@{u}", x.text),
            None => format!("{}@?", x.text),
        }
    }

    fn grade(g: &GradeAnn) -> String {
        sx(["grade".into(), term(&g.subject), opt(&g.in_type, |t| term(t))])
    }

    fn term(t: &Term) -> String {
        match &t.kind {
            TermKind::Type => "Type".into(),
            TermKind::Var(x) => ident_at(t.id, x),
            TermKind::Lit(l) => super::w::literal(l),
            TermKind::Pi(x, icit, g, dom, cod) => sx([
                "pi".into(),
                opt(x, ident),
                match icit {
                    Icit::Explicit => "e",
                    Icit::Implicit => "i",
                    Icit::Auto => "a",
                }
                .into(),
                opt(g, grade),
                term(dom),
                term(cod),
            ]),
            TermKind::Lam(x, g, ty, body) => sx(["lam".into(), ident(x), opt(g, grade), opt(ty, |t| term(t)), term(body)]),
            TermKind::App(f, a) => sx(["app".into(), term(f), term(a)]),
            TermKind::ImplicitApp(f, a) => sx(["iapp".into(), term(f), term(a)]),
            TermKind::AutoApp(f, a) => sx(["aapp".into(), term(f), term(a)]),
            TermKind::Search => "search".into(),
            TermKind::Ann(e, ty) => sx(["ann".into(), term(e), term(ty)]),
            TermKind::Hole => "_".into(),
            TermKind::BoxIntro(e) => sx(["box".into(), term(e)]),
            TermKind::Inf => "inf".into(),
            TermKind::Range(a, b) => sx(["range".into(), term(a), term(b)]),
            TermKind::LetBox(x, e, body) => sx(["letbox".into(), ident(x), term(e), term(body)]),
            TermKind::Let(gs, body) => sx(["let".into(), sx(gs.iter().map(group)), term(body)]),
            TermKind::Case(e, cs) => with(["case".to_string(), term(e)], cs.iter().map(clause).collect()),
            TermKind::Match(cs) => with(["match"], cs.iter().map(clause).collect()),
            TermKind::Omitted => "?".into(),
            TermKind::Eff(b, e) => sx(["eff".into(), term(b), term(e)]),
            TermKind::Row(ls, tail) => sx(["row".into(), sx(ls.iter().map(term)), opt(tail, |t| term(t))]),
            TermKind::Handle(e, cs) => with(["handle".to_string(), term(e)], cs.iter().map(handle_clause).collect()),
            TermKind::SeqNil => "nil".into(),
            TermKind::SeqCons(h, tl) => sx(["cons".into(), term(h), term(tl)]),
            TermKind::ArrayLit(es) => with(["array"], es.iter().map(term).collect()),
        }
    }

    fn handle_clause(c: &HandleClause) -> String {
        sx(["clause".into(), opt(&c.op, ident), sx(c.params.iter().map(ident)), term(&c.body)])
    }

    fn clause(c: &Clause) -> String {
        sx(["clause".into(), sx(c.pats.iter().map(pattern)), rhs(&c.rhs)])
    }

    fn rhs(r: &Rhs) -> String {
        match r {
            Rhs::Plain(e) => sx(["=".into(), term(e)]),
            Rhs::Guarded(gs) => with(["guards"], gs.iter().map(|(c, e)| sx([term(c), term(e)])).collect()),
            Rhs::Impossible => "impossible".into(),
        }
    }

    fn pattern(p: &Pattern) -> String {
        match &p.kind {
            PatKind::Var(x) => ident(x),
            PatKind::Wild => "_".into(),
            PatKind::Lit(l) => super::w::literal(l),
            PatKind::Con(c, ps) => with(["con".to_string(), ident_at(p.id, c)], ps.iter().map(pattern).collect()),
            PatKind::As(x, q) => sx(["as".into(), ident(x), pattern(q)]),
            PatKind::Dot(e) => sx(["dot".into(), term(e)]),
            PatKind::SeqNil => "pnil".into(),
            PatKind::SeqCons(ps) => with(["pcons"], ps.iter().map(pattern).collect()),
        }
    }

    fn bind(b: &Bind, terminating: &std::collections::HashSet<usize>, hints: &std::collections::HashSet<usize>) -> String {
        let vouched = b.name.unique.is_some_and(|u| terminating.contains(&u));
        let hinted = b.name.unique.is_some_and(|u| hints.contains(&u));
        sx([
            "bind".into(),
            ident(&b.name),
            opt(&b.ty, term),
            term(&b.rhs),
            flag("linear", b.linear),
            flag("terminating", vouched),
            flag("hint", hinted),
        ])
    }

    fn group(g: &BindGroup) -> String {
        let none = Default::default();
        match g {
            BindGroup::NonRec(b) => sx(["nonrec".into(), bind(b, &none, &none)]),
            BindGroup::Rec(bs) => with(["rec"], bs.iter().map(|b| bind(b, &none, &none)).collect()),
        }
    }

    fn type_param((x, k): &(Ident, Option<Term>)) -> String {
        sx([ident(x), opt(k, term)])
    }

    fn data(d: &DataDecl) -> String {
        let ctor = |c: &CtorDecl| match &c.shape {
            CtorShape::Fields(ts) => sx([ident(&c.name), with(["fields"], ts.iter().map(term).collect())]),
            CtorShape::Sig(ty) => sx([ident(&c.name), sx(["sig".into(), term(ty)])]),
        };
        sx([
            "data".into(),
            ident(&d.name),
            sx(d.params.iter().map(type_param)),
            opt(&d.kind, term),
            sx(d.ctors.iter().map(ctor)),
            opt(&d.seq, |(n, c)| sx(["seq".into(), ident(n), ident(c)])),
            flag("linear", d.linear),
            flag("trait", d.is_trait),
        ])
    }

    /// A trait's superclass projections and its methods, in their fields'
    /// order: each method with its field and how many parameters the trait
    /// has.
    fn trait_line(m: &Module, d: &DataDecl) -> String {
        let u = d.name.unique.unwrap_or(usize::MAX);
        let supers = m.supers.get(&u).cloned().unwrap_or_default();
        let mut methods: Vec<(usize, (usize, usize, usize))> = m.methods.iter().filter(|(_, (t, ..))| *t == u).map(|(m, i)| (*m, *i)).collect();
        methods.sort_by_key(|(_, (_, i, _))| *i);
        sx([
            "trait".into(),
            ident(&d.name),
            sx(supers.iter().map(|p| format!("super@{p}"))),
            sx(methods.iter().map(|(mu, (_, i, n))| sx([format!("method@{mu}"), i.to_string(), n.to_string()]))),
        ])
    }

    fn effect(e: &EffectDecl) -> String {
        let op = |o: &OpDecl| sx(["op".into(), ident(&o.name), flag("many", o.many), term(&o.ty)]);
        sx(["effect".into(), ident(&e.name), sx(e.params.iter().map(type_param)), sx(e.ops.iter().map(op))])
    }

    /// The program's own part of a module, a line each: data types, effects,
    /// natives, bindings, then the body, `main` and whether it is linear.
    pub fn module(m: &Module) -> String {
        OVERLOADS.with(|o| *o.borrow_mut() = m.overloads.clone());
        let own = |x: &Ident| x.unique.is_some_and(|u| u >= m.uniques.0);
        let mut lines: Vec<String> = Vec::new();
        lines.extend(m.datas.iter().filter(|d| own(&d.name)).map(data));
        lines.extend(m.datas.iter().filter(|d| own(&d.name) && d.is_trait).map(|d| trait_line(m, d)));
        lines.extend(m.effects.iter().filter(|e| own(&e.name)).map(effect));
        lines.extend(m.natives.iter().filter(|n| own(&n.name)).map(|n| sx(["native".into(), ident(&n.name), term(&n.ty)])));
        let binds = m.groups.iter().flat_map(|g| g.binds());
        lines.extend(binds.filter(|b| own(&b.name)).map(|b| bind(b, &m.terminating, &m.hints)));
        lines.extend(m.body.iter().map(|e| sx(["body".into(), term(e)])));
        lines.extend(m.main.iter().map(|u| sx(["main".to_string(), format!("main@{u}")])));
        lines.push(sx(["linear", if m.linear_body { "true" } else { "false" }]));
        lines.join("\n")
    }
}

// --- the parse, written out ---------------------------------------------------
//
// As `front/src/View.mw` writes it: an S-expression a line for each part of the
// program, nothing about where anything was written; `(error)` for a program
// the lexer, the layout or the parser found anything wrong with.

/// What this compiler's parser makes of `src`, written out.
fn parsed(src: &str) -> String {
    let lexed = idyllic_lexer::tokenize(src);
    let laid = idyllic_layout::layout(&LineIndex::new(src), &lexed.tokens);
    if !lexed.diagnostics.is_empty() || !laid.errors.is_empty() {
        return "(error)".to_string();
    }
    match idyllic_parser::parse(src.len(), &laid.tokens) {
        Ok(p) => w::program(&p),
        Err(_) => "(error)".to_string(),
    }
}

mod w {
    use idyllic_ast::*;

    fn sx<S: AsRef<str>>(parts: impl IntoIterator<Item = S>) -> String {
        let parts: Vec<String> = parts.into_iter().map(|s| s.as_ref().to_string()).collect();
        format!("({})", parts.join(" "))
    }

    fn opt<T>(m: &Option<T>, f: impl Fn(&T) -> String) -> String {
        m.as_ref().map_or("-".to_string(), f)
    }

    fn flag(yes: &str, b: bool) -> String {
        if b { yes } else { "-" }.to_string()
    }

    /// In quotes, with `\`, `"` and the control characters escaped; any other
    /// byte is itself.
    fn quoted(s: &str) -> String {
        let mut out = vec![b'"'];
        for &b in s.as_bytes() {
            match b {
                b'\\' => out.extend_from_slice(b"\\\\"),
                b'"' => out.extend_from_slice(b"\\\""),
                b'\n' => out.extend_from_slice(b"\\n"),
                b'\t' => out.extend_from_slice(b"\\t"),
                b'\r' => out.extend_from_slice(b"\\r"),
                b if b < 32 || b == 127 => out.extend_from_slice(format!("\\u{{{b:x}}}").as_bytes()),
                b => out.push(b),
            }
        }
        out.push(b'"');
        String::from_utf8(out).expect("escaping keeps UTF-8 whole")
    }

    pub fn literal(l: &Lit) -> String {
        match l {
            Lit::Unit => "(lit ())".into(),
            Lit::Int(n) => format!("(lit {n})"),
            Lit::Bool(b) => format!("(lit {})", if *b { "True" } else { "False" }),
            Lit::Str(s) => format!("(lit {})", quoted(s)),
            Lit::Char(c) => format!("(lit {})", idyllic_ast::show_char(*c)),
        }
    }

    fn grade(g: &GradeAnn) -> String {
        sx(["grade".into(), term(&g.subject), opt(&g.in_type, |t| term(t))])
    }

    fn param(p: &Param) -> String {
        sx(["param".into(), p.name.node.clone(), flag("unbox", p.unbox), opt(&p.grade, grade), opt(&p.ty, term)])
    }

    fn fields<T>(fs: &[(Ident, T)], f: impl Fn(&T) -> String) -> Vec<String> {
        fs.iter().map(|(x, e)| sx([x.node.clone(), f(e)])).collect()
    }

    fn with<S: Into<String>>(head: impl IntoIterator<Item = S>, rest: Vec<String>) -> String {
        sx(head.into_iter().map(Into::into).chain(rest))
    }

    pub fn term(t: &LTerm) -> String {
        match &t.node {
            Term::Type => "Type".into(),
            Term::Var(x) => x.node.clone(),
            Term::Lit(l) => literal(l),
            Term::Pi(x, icit, g, dom, cod) => sx([
                "pi".into(),
                opt(x, |x| x.node.clone()),
                match icit {
                    Icit::Explicit => "e",
                    Icit::Implicit => "i",
                    Icit::Auto => "a",
                }
                .into(),
                opt(g, grade),
                term(dom),
                term(cod),
            ]),
            Term::Lam(p, body) => sx(["lam".into(), param(&p.node), term(body)]),
            Term::App(f, a) => sx(["app".into(), term(f), term(a)]),
            Term::ImplicitApp(f, a) => sx(["iapp".into(), term(f), term(a)]),
            Term::AutoApp(f, a) => sx(["aapp".into(), term(f), term(a)]),
            Term::Resolved(u, x) => format!("{x}@{u}"),
            Term::RecordOf(r, args, fields, me) => with(
                ["recordof".to_string(), r.node.clone(), sx(args.iter().map(term)), me.node.clone()],
                fields.iter().map(|(f, e)| sx([f.node.clone(), term(e)])).collect(),
            ),
            Term::Search => "search".into(),
            Term::Ann(e, ty) => sx(["ann".into(), term(e), term(ty)]),
            Term::Hole => "_".into(),
            Term::BoxIntro(e) => sx(["box".into(), term(e)]),
            Term::Inf => "inf".into(),
            Term::Range(a, b) => sx(["range".into(), term(a), term(b)]),
            Term::LetBox(x, e, body) => sx(["letbox".into(), x.node.clone(), term(e), term(body)]),
            Term::Let(ds, body) => sx(["let".into(), sx(ds.iter().map(decl)), term(body)]),
            Term::Case(e, alts) => with(["case".to_string(), term(e)], alts.iter().map(alt).collect()),
            Term::Eff(b, e) => sx(["eff".into(), term(b), term(e)]),
            Term::Row(ls, tail) => sx(["row".into(), sx(ls.iter().map(term)), opt(tail, |t| term(t))]),
            Term::Handle(e, cs) => with(["handle".to_string(), term(e)], cs.iter().map(clause).collect()),
            Term::Seq(es) => with(["seq"], es.iter().map(term).collect()),
            Term::ArrayLit(es) => with(["array"], es.iter().map(term).collect()),
            Term::RecordLit(r, fs) => with(["record".to_string(), r.node.clone()], fields(fs, term)),
            Term::Proj(e, f) => sx(["proj".into(), term(e), f.node.clone()]),
            Term::Update(e, fs) => with(["update".to_string(), term(e)], fields(fs, term)),
        }
    }

    fn pat(p: &LPat) -> String {
        match &p.node {
            Pat::Bind(x) => param(x),
            Pat::Wild => "_".into(),
            Pat::Lit(l) => literal(l),
            Pat::Con(c, ps) => with(["con".to_string(), c.node.clone()], ps.iter().map(pat).collect()),
            Pat::As(x, q) => sx(["as".into(), x.node.clone(), pat(q)]),
            Pat::Dot(e) => sx(["dot".into(), term(e)]),
            Pat::Seq(ps) => with(["pseq"], ps.iter().map(pat).collect()),
            Pat::Record(r, fs) => with(["precord".to_string(), r.node.clone()], fields(fs, pat)),
        }
    }

    fn rhs(r: &Rhs) -> String {
        match r {
            Rhs::Plain(e) => sx(["=".into(), term(e)]),
            Rhs::Guarded(gs) => with(["guards"], gs.iter().map(|(c, e)| sx([term(c), term(e)])).collect()),
            Rhs::Impossible => "impossible".into(),
        }
    }

    fn alt(a: &Alt) -> String {
        sx(["alt".into(), pat(&a.pat), rhs(&a.rhs)])
    }

    fn clause(c: &HandleClause) -> String {
        sx(["clause".into(), c.op.node.clone(), sx(c.params.iter().map(|x| x.node.clone())), term(&c.body)])
    }

    fn decl(d: &LDecl) -> String {
        match &d.node {
            Decl::Sig(f, ty) => sx(["sig".into(), f.node.clone(), term(ty)]),
            Decl::Def(f, ps, ty, r) => {
                sx(["def".into(), f.node.clone(), sx(ps.iter().map(pat)), opt(ty, term), rhs(r)])
            }
            Decl::Native(f, ty) => sx(["native".into(), f.node.clone(), term(ty)]),
        }
    }

    fn visibility(v: Visibility) -> String {
        match v {
            Visibility::Private => "private",
            Visibility::Super => "super",
            Visibility::Package => "package",
            Visibility::Public => "public",
        }
        .into()
    }

    fn type_param((x, k): &(Ident, Option<LTerm>)) -> String {
        sx([x.node.clone(), opt(k, term)])
    }

    fn data(d: &Data) -> String {
        let ctor = |(name, c): &(Ident, Ctor)| match c {
            Ctor::Fields(ts) => sx([name.node.clone(), with(["fields"], ts.iter().map(term).collect())]),
            Ctor::Sig(ty) => sx([name.node.clone(), sx(["sig".into(), term(ty)])]),
        };
        sx([
            "data".into(),
            d.name.node.clone(),
            sx(d.params.iter().map(type_param)),
            opt(&d.kind, term),
            sx(d.ctors.iter().map(ctor)),
            opt(&d.seq, |s| sx(["seq".into(), s.nil.node.clone(), s.cons.node.clone()])),
            visibility(d.visibility),
            opt(&d.fields, |fs| sx(fs.iter().map(|f| f.node.clone()))),
            flag("trait", d.is_trait),
            sx(d.defaults.iter().map(|(m, name, function)| sx([m.node.clone(), name.clone(), flag("function", *function)]))),
        ])
    }

    fn effect(e: &EffectDecl) -> String {
        let op = |o: &OpSig| sx(["op".into(), o.name.node.clone(), flag("many", o.many), term(&o.ty)]);
        sx([
            "effect".into(),
            e.name.node.clone(),
            sx(e.params.iter().map(type_param)),
            sx(e.ops.iter().map(op)),
            visibility(e.visibility),
        ])
    }

    fn use_decl(u: &Use) -> String {
        sx([
            "use".into(),
            flag("pub", u.public),
            sx(u.path.iter().map(|x| x.node.clone())),
            flag("glob", u.glob),
            opt(&u.alias, |a| a.node.clone()),
            opt(&u.names, |ns| sx(ns.iter().map(|x| x.node.clone()))),
        ])
    }

    /// A line for each part: whether it is linear, then its uses, data types,
    /// effects, declarations, visibilities, `@terminating`s and body.
    pub fn program(p: &Program) -> String {
        let mut lines = vec![sx(["linear", if p.linear { "true" } else { "false" }])];
        lines.extend(p.uses.iter().map(|u| use_decl(&u.node)));
        lines.extend(p.datas.iter().map(|d| data(&d.node)));
        lines.extend(p.effects.iter().map(|e| effect(&e.node)));
        lines.extend(p.decls.iter().map(decl));
        lines.extend(p.visibility.iter().map(|(x, v)| sx(["vis".into(), x.node.clone(), visibility(*v)])));
        lines.extend(p.terminating.iter().map(|x| sx(["terminating".into(), x.node.clone()])));
        lines.extend(p.hints.iter().map(|x| sx(["hint".into(), x.node.clone()])));
        lines.extend(p.body.iter().map(|e| sx(["body".into(), term(e)])));
        // a module inside the file, its own program indented under it
        for (name, inner) in &p.modules {
            lines.push(sx(["module".into(), name.node.clone()]));
            lines.extend(program(inner).lines().map(|l| format!("  {l}")));
        }
        lines.join("\n")
    }
}
