//! **Parts**: a package's modules, compiled a strongly connected component
//! of their imports at a time.
//!
//! Modules may import each other in a circle, so the unit a module can be
//! checked in is not the module but the SCC of the import graph it is in:
//! the modules that reach each other through `use`s (or qualified names). A
//! package is checked a part at a time, in an order where each comes after
//! the parts it imports, each part against the compiled packages it depends
//! on and the compiled parts it imports, as a package is checked against its
//! dependencies. Inside a part the modules are renamed and checked together,
//! in one batch, as a whole package was.
//!
//! The graph is of files: a module written inside another's file (`module M
//! where ..`) is in that file's part. An import is a `use` whose path starts
//! with a module's, or a qualified name that does (`Collections.List.map`),
//! read from the text: more edges than there are uses at worst, which only
//! puts more modules in one part.
//!
//! The root module's part is last: every part that imports it is in it, so
//! that its body, which runs after every definition, is the last thing the
//! package defines.
//!
//! An instance is a hint to the parts that import (perhaps through others)
//! the module it is in, and to its own: as in Haskell, it is seen where its
//! module is.

use std::collections::BTreeSet;

/// One part: its modules, by their index in the package, the root's first
/// if it is here; the earlier parts it imports, directly or not; and whether
/// it holds the root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub modules: Vec<usize>,
    pub imports: Vec<usize>,
    pub root: bool,
}

/// The parts of the package `package` whose modules are `paths` (the
/// root's empty, first) with these texts. A module is named by its path, or
/// by its path under the package's name (`Std.Bool`).
pub fn parts(package: &str, paths: &[Vec<String>], texts: &[&str]) -> Vec<Part> {
    let n = paths.len();
    let names = |j: usize| [paths[j].join("."), format!("{package}.{}", paths[j].join("."))];
    let modules: Vec<String> = paths.iter().filter(|p| !p.is_empty()).map(|p| p.join(".")).collect();
    let edges: Vec<Vec<usize>> = (0..n)
        .map(|i| {
            (0..n)
                .filter(|&j| j != i)
                .filter(|&j| match paths[j].is_empty() {
                    // the root, by the package's name alone
                    true => names_root(texts[i], package, &modules),
                    false => names(j).iter().any(|p| imports(texts[i], p)),
                })
                .collect()
        })
        .collect();
    let mut comps = tarjan(&edges);
    // the root's part takes in every part that reaches it
    let reaches_root = reaching(&edges, 0);
    let root = comps.iter().position(|c| c.contains(&0)).expect("the root is somewhere");
    let mut merged: BTreeSet<usize> = comps[root].iter().copied().collect();
    comps.retain(|c| {
        if c.iter().any(|m| reaches_root.contains(m)) {
            merged.extend(c.iter().copied());
            false
        } else {
            true
        }
    });
    comps.push(merged.into_iter().collect());
    // which part each module is in, and what each part imports
    let mut part_of = vec![0; n];
    for (k, c) in comps.iter().enumerate() {
        for &m in c {
            part_of[m] = k;
        }
    }
    let mut out: Vec<Part> = Vec::new();
    for (k, c) in comps.iter().enumerate() {
        let mut direct: BTreeSet<usize> = BTreeSet::new();
        for &m in c {
            direct.extend(edges[m].iter().map(|&j| part_of[j]).filter(|&p| p != k));
        }
        let mut all = direct.clone();
        for &d in &direct {
            all.extend(out[d].imports.iter().copied());
        }
        let mut modules = c.clone();
        modules.sort_unstable();
        out.push(Part { modules, imports: all.into_iter().collect(), root: k == comps.len() - 1 });
    }
    out
}

/// Whether a module written inside a file has the path of another module of
/// the package: the files are then one part, so that the renamer, which
/// reports it, sees both.
pub fn clash(paths: &[Vec<String>], programs: &[idyllic_ast::Program]) -> bool {
    fn inner(at: &[String], p: &idyllic_ast::Program, out: &mut Vec<Vec<String>>) {
        for (name, q) in &p.modules {
            let mut path = at.to_vec();
            path.push(name.node.clone());
            inner(&path, q, out);
            out.push(path);
        }
    }
    let mut all: Vec<Vec<String>> = paths.to_vec();
    for (path, p) in paths.iter().zip(programs) {
        inner(path, p, &mut all);
    }
    let n = all.len();
    all.sort();
    all.dedup();
    all.len() != n
}

/// Whether `text` names the root module, by the package's name alone: not
/// as the start of another module's path, `App.Syntax`.
fn names_root(text: &str, package: &str, modules: &[String]) -> bool {
    let mut from = 0;
    while let Some(at) = text[from..].find(package) {
        let start = from + at;
        let rest = &text[start + package.len()..];
        let after = rest.bytes().next();
        let follows = matches!(after, None | Some(b'.' | b' ' | b'\n' | b'\r' | b'(' | b'\t'));
        let named = after == Some(b'.') || text[..start].trim_end().ends_with("use");
        if follows && named && (start == 0 || !is_word(text.as_bytes()[start - 1])) {
            let under = rest.strip_prefix('.').map(|r| r.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.')).next().unwrap_or(""));
            let another = under.is_some_and(|u| modules.iter().any(|m| u == m || u.starts_with(&format!("{m}."))));
            if !another {
                return true;
            }
        }
        from = start + package.len();
    }
    false
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'\'' || b == b'.'
}

/// Whether `text` names the module at `path`: a `use` of it or of something
/// in it, or a name qualified with it.
fn imports(text: &str, path: &str) -> bool {
    let bytes = text.as_bytes();
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'\'' || b == b'.';
    let mut from = 0;
    while let Some(at) = text[from..].find(path) {
        let start = from + at;
        let end = start + path.len();
        let before = start == 0 || !word(bytes[start - 1]);
        let after = bytes.get(end).copied();
        let follows = matches!(after, None | Some(b'.' | b' ' | b'\n' | b'\r' | b'(' | b'\t'));
        // `use Odd`, `use Odd (odd)`, `Odd.odd`, `use Odd.*`
        let named = after == Some(b'.') || text[..start].trim_end().ends_with("use");
        if before && follows && named {
            return true;
        }
        from = end;
    }
    false
}

/// The SCCs of a graph, each after those it reaches (Tarjan's order).
fn tarjan(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    struct T<'a> {
        edges: &'a [Vec<usize>],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        out: Vec<Vec<usize>>,
    }
    fn visit(t: &mut T<'_>, v: usize) {
        t.index[v] = Some(t.next);
        t.low[v] = t.next;
        t.next += 1;
        t.stack.push(v);
        t.on[v] = true;
        for &w in &t.edges[v] {
            match t.index[w] {
                None => {
                    visit(t, w);
                    t.low[v] = t.low[v].min(t.low[w]);
                }
                Some(i) if t.on[w] => t.low[v] = t.low[v].min(i),
                Some(_) => {}
            }
        }
        if Some(t.low[v]) == t.index[v] {
            let mut comp = Vec::new();
            while let Some(w) = t.stack.pop() {
                t.on[w] = false;
                comp.push(w);
                if w == v {
                    break;
                }
            }
            t.out.push(comp);
        }
    }
    let n = edges.len();
    let mut t = T { edges, index: vec![None; n], low: vec![0; n], on: vec![false; n], stack: Vec::new(), next: 0, out: Vec::new() };
    for v in 0..n {
        if t.index[v].is_none() {
            visit(&mut t, v);
        }
    }
    t.out
}

/// The nodes from which `to` can be reached.
fn reaching(edges: &[Vec<usize>], to: usize) -> BTreeSet<usize> {
    let mut seen = BTreeSet::from([to]);
    loop {
        let more: Vec<usize> = (0..edges.len()).filter(|v| !seen.contains(v) && edges[*v].iter().any(|w| seen.contains(w))).collect();
        if more.is_empty() {
            return seen;
        }
        seen.extend(more);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Vec<String> {
        if s.is_empty() { Vec::new() } else { s.split('.').map(String::from).collect() }
    }

    #[test]
    fn modules_that_import_each_other_are_one_part() {
        let paths = [p(""), p("Even"), p("Odd"), p("Util")];
        let texts = ["use Even (even)\n", "use Odd (odd)\nuse Util\n", "use Even (even)\n", "x = 1\n"];
        let ps = parts("Main", &paths, &texts);
        assert_eq!(ps.len(), 3, "{ps:?}");
        assert_eq!(ps[0].modules, [3]);
        assert_eq!(ps[1].modules, [1, 2]);
        assert_eq!(ps[1].imports, [0]);
        assert_eq!(ps[2], Part { modules: vec![0], imports: vec![0, 1], root: true });
    }

    #[test]
    fn a_module_that_names_the_root_is_in_its_part() {
        let paths = [p(""), p("Syntax"), p("Child")];
        let texts = ["use Child\n", "x = 1\n", "use App.Syntax\nuse App (shared)\n"];
        let ps = parts("App", &paths, &texts);
        assert_eq!(ps.iter().map(|p| p.modules.clone()).collect::<Vec<_>>(), [vec![1], vec![0, 2]]);
        // `App.Syntax` alone is the module, not the root
        let texts = ["use Child\n", "x = 1\n", "use App.Syntax\n"];
        assert_eq!(parts("App", &paths, &texts).len(), 3);
    }

    #[test]
    fn the_parts_come_before_the_root() {
        let paths = [p(""), p("A"), p("B")];
        let texts = ["use A\n", "x = Main.y\n", "Collections.List.map\n"];
        let ps = parts("Main", &paths, &texts);
        // `A` names no module `Main` here: the root has no path to name
        // and `A` names the root, `Main.y`, so it is in the root's part
        assert_eq!(ps.iter().map(|p| p.modules.clone()).collect::<Vec<_>>(), [vec![2], vec![0, 1]]);
    }

    #[test]
    fn a_qualified_name_is_an_import_and_a_longer_name_is_not() {
        assert!(imports("f = Odd.odd 3\n", "Odd"));
        assert!(imports("use Odd.*\n", "Odd"));
        assert!(!imports("f = Oddity.x\n", "Odd"));
        assert!(!imports("-- Odd numbers\n", "Odd"));
    }
}
