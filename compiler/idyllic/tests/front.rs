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
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("the repository")
}

fn meadow() -> PathBuf {
    std::env::var_os("IDYLL_FRONT_MEADOW").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("meadow"))
}

/// `front/`, built natively, once: its executable, or why there is none.
fn front() -> Result<PathBuf, String> {
    static BUILT: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let dir = repo().join("front");
            let out = Command::new(meadow())
                .arg("build")
                .arg("--release")
                .arg(&dir)
                .output()
                .map_err(|e| format!("no meadow to build front/ with ({e})"))?;
            if !out.status.success() {
                return Err(format!("front/ did not build:\n{}", String::from_utf8_lossy(&out.stderr)));
            }
            Ok(dir.join("target/release/native/silo").join(format!("Idyll{}", std::env::consts::EXE_SUFFIX)))
        })
        .clone()
}

/// Every program of the corpora, with a name for it; the prelude; and every
/// `.idl` file of the repository (the standard library, the examples, the
/// benchmarks).
fn corpus() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, text) in [
        ("handwritten", include_str!("corpus/handwritten.txt")),
        ("exhaustive", include_str!("corpus/exhaustive.txt")),
        ("tutorial", include_str!("corpus/tutorial.txt")),
    ] {
        for (i, src) in text.replace("\r\n", "\n").split("\n%%%\n").enumerate() {
            out.push((format!("{name}-{i:04}"), src.to_string()));
        }
    }
    out.push(("prelude".to_string(), idyllic::pipeline::PRELUDE.to_string()));
    let repo = repo();
    let mut files = Vec::new();
    for dir in ["lib", "examples", "bench"] {
        idl_files(&repo.join(dir), &mut files);
    }
    files.sort();
    for f in files {
        let name = f.strip_prefix(&repo).unwrap_or(&f).to_string_lossy().replace(['/', '\\'], "-");
        let text = std::fs::read_to_string(&f).expect("a source file").replace("\r\n", "\n");
        out.push((name.trim_end_matches(".idl").to_string(), text));
    }
    out
}

fn idl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "target" && n != "build") {
                idl_files(&path, out);
            }
        } else if path.extension().is_some_and(|x| x == "idl") {
            out.push(path);
        }
    }
}

/// What `front/`'s `pass` writes for each program: its programs written to
/// files of a directory of the test's own, and `Idyll <pass> <files>` run on
/// them once, its output cut at each `== <file>` line.
fn dumps(pass: &str, programs: &[(String, String)]) -> Result<Vec<String>, String> {
    let exe = front()?;
    let dir = std::env::temp_dir().join(format!("idyll-front-{pass}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let files: Vec<String> = programs
        .iter()
        .map(|(name, src)| {
            let f = format!("{name}.idl");
            std::fs::write(dir.join(&f), src).map(|()| f)
        })
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    // run where the files are, a few hundred at a time: a command line has a
    // length it may not pass
    let mut text = String::new();
    for batch in files.chunks(200) {
        let out = Command::new(&exe).arg(pass).args(batch).current_dir(&dir).output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(format!("front/ failed:\n{}", String::from_utf8_lossy(&out.stderr)));
        }
        text.push_str(&String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"));
    }
    let _ = std::fs::remove_dir_all(&dir);
    // A program's own text can have `== ` in it, so a dump starts only at the
    // line that names the next program's file.
    let mut parts: Vec<String> = Vec::new();
    for line in text.lines() {
        let next = programs.get(parts.len()).map(|(name, _)| format!("{name}.idl"));
        if line.starts_with("== ") && next.is_some_and(|f| line.ends_with(&f)) {
            parts.push(String::new());
        } else if let Some(part) = parts.last_mut() {
            if !part.is_empty() {
                part.push('\n');
            }
            part.push_str(line);
        }
    }
    if parts.len() != programs.len() {
        return Err(format!("front/ wrote {} dumps for {} programs", parts.len(), programs.len()));
    }
    Ok(parts)
}

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

/// Where two dumps first differ, for a report: the line, for a dump of
/// several, or the token.
fn first_difference(ours: &str, theirs: &str) -> String {
    if ours.contains('\n') || theirs.contains('\n') {
        let (a, b): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), theirs.lines().collect());
        let at = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
        let line = |v: &[&str]| v.get(at).copied().unwrap_or("(nothing)").to_string();
        return format!("at line {at}:\n    here    {}\n    front/  {}", line(&a), line(&b));
    }
    let (a, b): (Vec<&str>, Vec<&str>) = (ours.split(' ').collect(), theirs.split(' ').collect());
    let at = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let around = |v: &[&str]| v[at.saturating_sub(4)..(at + 4).min(v.len())].join(" ");
    format!("at token {at}: here `{}`, front/ `{}`", around(&a), around(&b))
}

/// Compare `front/`'s dump of each program with this compiler's.
fn agree(pass: &str, ours: impl Fn(&str) -> String) {
    let programs = corpus();
    let theirs = match dumps(pass, &programs) {
        Ok(d) => d,
        Err(why) if why.starts_with("no meadow") => {
            eprintln!("{why}: front/ was not checked");
            return;
        }
        Err(why) => panic!("{why}"),
    };
    let differ: Vec<String> = programs
        .iter()
        .zip(&theirs)
        .filter_map(|((name, src), theirs)| {
            let ours = ours(src);
            (ours != *theirs).then(|| format!("{name}: {}", first_difference(&ours, theirs)))
        })
        .collect();
    let errors = theirs.iter().filter(|d| *d == "(error)").count();
    eprintln!("{pass}: {} programs, {errors} of them an error to front/", programs.len());
    assert!(
        differ.is_empty(),
        "{} of {} programs differ:\n{}",
        differ.len(),
        programs.len(),
        differ.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn the_tokens_and_their_layout_agree() {
    agree("tokens", tokens);
}

#[test]
fn the_parses_agree() {
    agree("parse", parsed);
}

// --- the parse, written out ---------------------------------------------------
//
// As `front/src/Ast.mw` writes it: an S-expression a line for each part of the
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

    fn literal(l: &Lit) -> String {
        match l {
            Lit::Unit => "(lit ())".into(),
            Lit::Int(n) => format!("(lit {n})"),
            Lit::Bool(b) => format!("(lit {})", if *b { "True" } else { "False" }),
            Lit::Str(s) => format!("(lit {})", quoted(s)),
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
                if *icit == Icit::Explicit { "e" } else { "i" }.into(),
                opt(g, grade),
                term(dom),
                term(cod),
            ]),
            Term::Lam(p, body) => sx(["lam".into(), param(&p.node), term(body)]),
            Term::App(f, a) => sx(["app".into(), term(f), term(a)]),
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
        lines.extend(p.body.iter().map(|e| sx(["body".into(), term(e)])));
        lines.join("\n")
    }
}
