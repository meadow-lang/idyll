//! The entry's answer, written out as the REPL writes it.
//!
//! The runtimes print only strings (`docs/CUT.md`: `answer` is `none` or
//! `str`), so a program whose value is anything else is given code that
//! turns it into one, in the form Idyll's REPL, AxCut machine and reference
//! interpreter show it: `42`, `True`, `()`, `"text"`, `[v]`, `#[1; 2]`,
//! `(1, True)`, `x :: xs`, `Just (Ok 3)`. Then every implementation's answer
//! can be compared as text.
//!
//! The answer's type is known when the program is lowered, so what shows it
//! is monomorphic: a `def` for each data type it meets, named for the type
//! as it is written (`"#show Maybe (Int, Bool)"`), one for each such type
//! that is a field and may need parentheses (`"#atom .."`), and one for
//! each array type. An integer is shown by
//! the prelude's `showInt`, which is kept for the purpose. A string is
//! quoted and escaped as the REPL does it, which is Rust's `{:?}`: `\"`,
//! `\\`, `\n`, `\t`, `\r`, `\0`, and `\u{1b}` for any other control
//! character (`"#escape"`).
//!
//! A value with a function or a type in it has nothing to show, as at the
//! REPL: its program answers `none`.

use super::*;
use idyllic_typecheck::typer::{Scope, Shown, ShownCtor};
use idyllic_typecheck::value::Value;

/// What shows the values of a data type: its `def`, and the one for when
/// it is a field (`None` if it never needs parentheses).
#[derive(Clone)]
pub(super) struct ShowDefs {
    show: Symbol,
    atom: Option<Symbol>,
    /// the prelude's pair, written as a tuple: and what writes its items
    items: Option<Symbol>,
}

/// The name of the prelude's pair, shown as a tuple.
const PAIR: &str = "Pair";

impl Lower<'_> {
    /// The entry's statement, given what computes its value: the value
    /// shown, and handed to `k` as a string; `None` if it has nothing to
    /// show.
    pub(super) fn show_answer(&mut self, s: &Scope, ty: &Rc<Value>, k: Consumer) -> Option<(Rep, Consumer)> {
        let v = self.name("answer");
        let body = self.show(s, ty, Producer::Var(v.clone()), k, false)?;
        let rep = self.flat_rep(s, ty);
        Some((rep.clone(), Consumer::MuTilde(Binder { name: v, rep }, Box::new(body))))
    }

    /// A representation for a closed type: there are no type variables
    /// where the answer is shown.
    fn flat_rep(&mut self, s: &Scope, ty: &Rc<Value>) -> Rep {
        match self.typer.rep(s, ty) {
            typer::Rep::I64 => Rep::I64,
            typer::Rep::Bool => Rep::Bool,
            typer::Rep::Unit | typer::Rep::Erased => Rep::Unit,
            typer::Rep::Str => Rep::Str,
            typer::Rep::Ptr => Rep::Ptr,
            typer::Rep::Var(_) | typer::Rep::Any => Rep::Any,
        }
    }

    /// `x`, of type `ty`, as a string handed to `k`; parenthesised if
    /// `atom` and it is a constructor with fields.
    fn show(&mut self, s: &Scope, ty: &Rc<Value>, x: Producer, k: Consumer, atom: bool) -> Option<Statement> {
        let text = |t: &str| Producer::Str(t.to_string());
        Some(match self.typer.shown(s, ty) {
            Shown::Int => Statement::Call(self.show_int.clone()?, vec![x], vec![k]),
            Shown::Bool => {
                let (f, t) = (self.name("u"), self.name("u"));
                let branch = |name: String, word: &str, k: Consumer| {
                    Consumer::MuTilde(Binder { name, rep: Rep::Unit }, Box::new(Statement::Cut(text(word), k)))
                };
                Statement::Prim("if".into(), vec![x], vec![branch(f, "False", k.clone()), branch(t, "True", k)])
            }
            Shown::Unit => Statement::Cut(text("()"), k),
            Shown::Str => {
                let escape = self.escape_def();
                let e = self.name("e");
                let quoted = concat(vec![text("\""), Producer::Var(e.clone()), text("\"")], k);
                Statement::Call(escape, vec![x], vec![str_binder(e, quoted)])
            }
            // a box is the IR's one-element tuple
            Shown::Box(inner) => {
                let v = self.name("x");
                let rep = self.flat_rep(s, &inner);
                let t = self.name("s");
                let wrap = concat(vec![text("["), Producer::Var(t.clone()), text("]")], k);
                let body = self.show(s, &inner, Producer::Var(v.clone()), str_binder(t, wrap), false)?;
                Statement::Cut(x, Consumer::Case(vec![Arm { ctor: None, fields: vec![Binder { name: v, rep }], body }]))
            }
            Shown::Array(elem) => Statement::Call(self.array_show(s, ty, &elem)?, vec![x], vec![k]),
            Shown::Data { key, ctors } => {
                let defs = self.data_show(s, &key, ctors.clone())?;
                let which = match (atom, &defs.atom) {
                    (true, Some(a)) => {
                        // made the first time a field needs it
                        if self.atoms.insert(key) {
                            self.atom_def(s, &defs, a, &ctors)?;
                        }
                        a.clone()
                    }
                    _ => defs.show,
                };
                Statement::Call(which, vec![x], vec![k])
            }
            Shown::Nothing => return None,
        })
    }

    /// Several values shown in turn, then `done` with their strings.
    fn show_all(
        &mut self,
        s: &Scope,
        mut items: VecDeque<(Rc<Value>, Producer, bool)>,
        mut got: Vec<Producer>,
        done: &mut dyn FnMut(&mut Self, Vec<Producer>) -> Statement,
    ) -> Option<Statement> {
        let Some((ty, x, atom)) = items.pop_front() else { return Some(done(self, got)) };
        let t = self.name("s");
        got.push(Producer::Var(t.clone()));
        let rest = self.show_all(s, items, got, done)?;
        self.show(s, &ty, x, str_binder(t, rest), atom)
    }

    /// What shows a data type's values, made the first time it is asked
    /// for.
    fn data_show(&mut self, s: &Scope, key: &str, ctors: Vec<ShownCtor>) -> Option<ShowDefs> {
        if let Some(known) = self.shows.get(key) {
            return known.clone();
        }
        let tuple = ctors.len() == 1 && ctors[0].name == PAIR && ctors[0].fields.len() == 2 && self.is_prelude(ctors[0].unique);
        let compound = !tuple && ctors.iter().any(|c| !c.fields.is_empty());
        let named = |what: &str| named(&self.package, what, key);
        let defs = ShowDefs {
            show: named("show"),
            atom: compound.then(|| named("atom")),
            items: tuple.then(|| named("items")),
        };
        // known before its body is made: a type may contain itself
        self.shows.insert(key.to_string(), Some(defs.clone()));
        let made = if tuple { self.tuple_defs(s, &defs, &ctors[0]) } else { self.data_defs(s, &defs, &ctors) };
        if made.is_none() {
            self.shows.insert(key.to_string(), None);
        }
        made.map(|_| defs)
    }

    fn is_prelude(&self, u: usize) -> bool {
        self.symbols.get(&u).is_some_and(|(p, _)| p == "Prelude")
    }

    /// `Just (Ok 3)`, `x :: xs`: a case on the constructor.
    fn data_defs(&mut self, s: &Scope, defs: &ShowDefs, ctors: &[ShownCtor]) -> Option<()> {
        let (x, k) = (self.name("x"), self.name("k"));
        let mut arms = Vec::new();
        for c in ctors {
            if let Some(info) = self.ctors.get(&c.unique) {
                self.datas.insert(info.data);
            }
            let symbol = self.symbol(c.unique)?;
            let fields: Vec<Binder> =
                c.fields.iter().map(|ty| Binder { name: self.name("f"), rep: self.flat_rep(s, ty) }).collect();
            let vars: Vec<Producer> = fields.iter().map(|b| Producer::Var(b.name.clone())).collect();
            let kv = Consumer::Var(k.clone());
            let body = match (c.name.as_str(), c.fields.as_slice()) {
                (name, []) => Statement::Cut(Producer::Str(name.to_string()), kv),
                // `x :: xs`, written as it is read: the head parenthesised
                // if it is compound, the tail as it is
                ("::", [head, tail]) => {
                    let items = VecDeque::from([(head.clone(), vars[0].clone(), true), (tail.clone(), vars[1].clone(), false)]);
                    self.show_all(s, items, Vec::new(), &mut |_, ss| {
                        concat(vec![ss[0].clone(), Producer::Str(" :: ".into()), ss[1].clone()], kv.clone())
                    })?
                }
                (name, tys) => {
                    let items: VecDeque<_> = tys.iter().cloned().zip(vars.iter().cloned()).map(|(t, v)| (t, v, true)).collect();
                    let name = name.to_string();
                    self.show_all(s, items, Vec::new(), &mut |_, ss| {
                        let mut parts = vec![Producer::Str(name.clone())];
                        for p in ss {
                            parts.push(Producer::Str(" ".into()));
                            parts.push(p);
                        }
                        concat(parts, kv.clone())
                    })?
                }
            };
            arms.push(Arm { ctor: Some(symbol), fields, body });
        }
        let body = Statement::Cut(Producer::Var(x.clone()), Consumer::Case(arms));
        self.push_show(&defs.show, x, k, body);
        Some(())
    }

    /// A data type's values as a field: in parentheses, if its constructor
    /// has fields of its own.
    fn atom_def(&mut self, s: &Scope, defs: &ShowDefs, atom: &Symbol, ctors: &[ShownCtor]) -> Option<()> {
        let (x, k) = (self.name("x"), self.name("k"));
        let mut arms = Vec::new();
        for c in ctors {
            let symbol = self.symbol(c.unique)?;
            let fields: Vec<Binder> =
                c.fields.iter().map(|ty| Binder { name: self.name("f"), rep: self.flat_rep(s, ty) }).collect();
            let whole = vec![Producer::Var(x.clone())];
            let body = if c.fields.is_empty() {
                Statement::Call(defs.show.clone(), whole, vec![Consumer::Var(k.clone())])
            } else {
                let t = self.name("s");
                let wrap = concat(vec![Producer::Str("(".into()), Producer::Var(t.clone()), Producer::Str(")".into())], Consumer::Var(k.clone()));
                Statement::Call(defs.show.clone(), whole, vec![str_binder(t, wrap)])
            };
            arms.push(Arm { ctor: Some(symbol), fields, body });
        }
        let body = Statement::Cut(Producer::Var(x.clone()), Consumer::Case(arms));
        self.push_show(atom, x, k, body);
        Some(())
    }

    /// `(1, True)`, and a pair nested to the right as a longer tuple,
    /// `(1, 2, 3)`: its items, then the parentheses.
    fn tuple_defs(&mut self, s: &Scope, defs: &ShowDefs, pair: &ShownCtor) -> Option<()> {
        let items_def = defs.items.clone().expect("a tuple's");
        if let Some(info) = self.ctors.get(&pair.unique) {
            self.datas.insert(info.data);
        }
        let symbol = self.symbol(pair.unique)?;
        let (first, second) = (pair.fields[0].clone(), pair.fields[1].clone());
        // the items: the first, then the rest's items if it is a pair too
        let (x, k) = (self.name("x"), self.name("k"));
        let fields = vec![
            Binder { name: self.name("f"), rep: self.flat_rep(s, &first) },
            Binder { name: self.name("f"), rep: self.flat_rep(s, &second) },
        ];
        let (a, b) = (Producer::Var(fields[0].name.clone()), Producer::Var(fields[1].name.clone()));
        let rest_items = match self.typer.shown(s, &second) {
            Shown::Data { key, ctors } if ctors.len() == 1 && ctors[0].name == PAIR && self.is_prelude(ctors[0].unique) => {
                self.data_show(s, &key, ctors)?.items
            }
            _ => None,
        };
        let kv = Consumer::Var(k.clone());
        let ta = self.name("s");
        let tb = self.name("s");
        let joined = concat(vec![Producer::Var(ta.clone()), Producer::Str(", ".into()), Producer::Var(tb.clone())], kv);
        let rest = match rest_items {
            Some(items) => Statement::Call(items, vec![b], vec![str_binder(tb, joined)]),
            None => self.show(s, &second, b, str_binder(tb, joined), false)?,
        };
        let body = self.show(s, &first, a, str_binder(ta, rest), false)?;
        let arm = Arm { ctor: Some(symbol), fields, body };
        self.push_show(&items_def, x.clone(), k, Statement::Cut(Producer::Var(x), Consumer::Case(vec![arm])));
        // the whole: in parentheses
        let (x, k) = (self.name("x"), self.name("k"));
        let t = self.name("s");
        let wrap = concat(vec![Producer::Str("(".into()), Producer::Var(t.clone()), Producer::Str(")".into())], Consumer::Var(k.clone()));
        let body = Statement::Call(items_def, vec![Producer::Var(x.clone())], vec![str_binder(t, wrap)]);
        self.push_show(&defs.show, x, k, body);
        Some(())
    }

    /// `#[1; 2]`, `#[1;]`, `#[;]`: the elements in turn, then the brackets.
    fn array_show(&mut self, s: &Scope, array: &Rc<Value>, elem: &Rc<Value>) -> Option<Symbol> {
        let key = self.typer.type_key(s, array)?;
        if let Some(known) = self.shows.get(&key) {
            return known.clone().map(|d| d.show);
        }
        let (show, items) = (named(&self.package, "show", &key), named(&self.package, "items", &key));
        self.shows.insert(key.clone(), Some(ShowDefs { show: show.clone(), atom: None, items: Some(items.clone()) }));
        // the items from `i` up to `n`, separated by "; "
        let (xs, i, n, k) = (self.name("xs"), self.name("i"), self.name("n"), self.name("k"));
        let (e, se, j, rest) = (self.name("e"), self.name("s"), self.name("j"), self.name("s"));
        let rep = self.flat_rep(s, elem);
        let kv = || Consumer::Var(k.clone());
        let more = Statement::Call(
            items.clone(),
            vec![Producer::Var(xs.clone()), Producer::Var(j.clone()), Producer::Var(n.clone())],
            vec![str_binder(rest.clone(), concat(vec![Producer::Var(se.clone()), Producer::Str("; ".into()), Producer::Var(rest)], kv()))],
        );
        let last = Statement::Cut(Producer::Var(se.clone()), kv());
        let next = Statement::Prim(
            "add".into(),
            vec![Producer::Var(i.clone()), Producer::Int(1)],
            vec![Consumer::MuTilde(
                Binder { name: j.clone(), rep: Rep::I64 },
                Box::new(Statement::Prim(
                    "eq".into(),
                    vec![Producer::Var(j), Producer::Var(n.clone())],
                    vec![unit_binder(self.name("u"), more), unit_binder(self.name("u"), last)],
                )),
            )],
        );
        let shown = self.show(s, elem, Producer::Var(e.clone()), str_binder(se, next), false);
        let Some(shown) = shown else {
            self.shows.insert(key, None);
            return None;
        };
        let body = Statement::Prim(
            "arrayGet".into(),
            vec![Producer::Var(xs.clone()), Producer::Var(i.clone())],
            vec![Consumer::MuTilde(Binder { name: e, rep }, Box::new(shown))],
        );
        self.show_defs.push(Def {
            symbol: items.clone(),
            rep_vars: Vec::new(),
            effect_vars: Vec::new(),
            params: vec![
                Binder { name: xs, rep: Rep::Ptr },
                Binder { name: i, rep: Rep::I64 },
                Binder { name: n.clone(), rep: Rep::I64 },
            ],
            conts: vec![k],
            body,
        });
        // the whole: brackets, and a `;` after one element alone
        let (xs, n, k, t) = (self.name("xs"), self.name("n"), self.name("k"), self.name("s"));
        let kv = || Consumer::Var(k.clone());
        let one = concat(vec![Producer::Str("#[".into()), Producer::Var(t.clone()), Producer::Str(";]".into())], kv());
        let many = concat(vec![Producer::Str("#[".into()), Producer::Var(t.clone()), Producer::Str("]".into())], kv());
        let bracket = Statement::Prim(
            "eq".into(),
            vec![Producer::Var(n.clone()), Producer::Int(1)],
            vec![unit_binder(self.name("u"), many), unit_binder(self.name("u"), one)],
        );
        let nonempty = Statement::Call(
            items,
            vec![Producer::Var(xs.clone()), Producer::Int(0), Producer::Var(n.clone())],
            vec![str_binder(t, bracket)],
        );
        let empty = Statement::Cut(Producer::Str("#[;]".into()), kv());
        let body = Statement::Prim(
            "arrayLen".into(),
            vec![Producer::Var(xs.clone())],
            vec![Consumer::MuTilde(
                Binder { name: n.clone(), rep: Rep::I64 },
                Box::new(Statement::Prim(
                    "eq".into(),
                    vec![Producer::Var(n), Producer::Int(0)],
                    vec![unit_binder(self.name("u"), nonempty), unit_binder(self.name("u"), empty)],
                )),
            )],
        );
        self.push_show(&show, xs, k, body);
        Some(show)
    }

    /// What escapes a string as the REPL shows one, made the first time it
    /// is wanted. It goes byte by byte, copying the runs between what needs
    /// escaping as they are: each run ends at an ASCII byte, so no character
    /// is cut in two.
    fn escape_def(&mut self) -> Symbol {
        let escape = Symbol { lang: "idyll".into(), package: self.package.clone(), path: vec!["#escape".into()] };
        if self.atoms.insert("#escape".into()) {
            self.make_escape(&escape);
        }
        escape
    }

    fn make_escape(&mut self, escape: &Symbol) {
        let (go, byte) = (escape.child("go"), escape.child("byte"));
        let var = |x: &String| Producer::Var(x.clone());
        let text = |t: &str| Producer::Str(t.to_string());
        let int = Producer::Int;
        // escape(s) = go(s, 0, 0, the length, "")
        let (st, n, k) = (self.name("s"), self.name("n"), self.name("k"));
        let body = prim_then("stringByteLength", vec![var(&st)], &n, Rep::I64,
            Statement::Call(go.clone(), vec![var(&st), int(0), int(0), var(&n), text("")], vec![Consumer::Var(k.clone())]));
        self.push_def(escape, vec![(st, Rep::Str)], k, body);
        // go(s, start, i, n, acc): what is escaped of `s` before `start` is
        // `acc`; the bytes from `start` up to `i` need no escaping
        let (st, start, i, n, acc, k) = (self.name("s"), self.name("start"), self.name("i"), self.name("n"), self.name("acc"), self.name("k"));
        let (b, e, len, j, run, acc2, last) = (self.name("b"), self.name("e"), self.name("len"), self.name("j"), self.name("run"), self.name("acc"), self.name("run"));
        let kv = || Consumer::Var(k.clone());
        let plain = Statement::Call(go.clone(), vec![var(&st), var(&start), var(&j), var(&n), var(&acc)], vec![kv()]);
        let escaped = prim_then("stringSlice", vec![var(&st), var(&start), var(&i)], &run, Rep::Str,
            concat(vec![var(&acc), var(&run), var(&e)], str_binder(acc2.clone(),
                Statement::Call(go.clone(), vec![var(&st), var(&j), var(&j), var(&n), var(&acc2)], vec![kv()]))));
        let which = prim_then("stringByteLength", vec![var(&e)], &len, Rep::I64, test_eq(self, var(&len), int(0), escaped, plain));
        let after = prim_then("add", vec![var(&i), int(1)], &j, Rep::I64, which);
        let step = prim_then("stringByteAt", vec![var(&st), var(&i)], &b, Rep::I64,
            Statement::Call(byte.clone(), vec![var(&b)], vec![str_binder(e, after)]));
        let end = prim_then("stringSlice", vec![var(&st), var(&start), var(&n)], &last, Rep::Str, concat(vec![var(&acc), var(&last)], kv()));
        let body = test_eq(self, var(&i), var(&n), step, end);
        let params = vec![(st, Rep::Str), (start, Rep::I64), (i, Rep::I64), (n, Rep::I64), (acc, Rep::Str)];
        self.push_def(&go, params, k, body);
        // byte(b): how it is escaped, or "" if it needs none
        let (b, k) = (self.name("b"), self.name("k"));
        let kv = || Consumer::Var(k.clone());
        let digits = text("0123456789abcdef");
        let (hi, lo, h1, l1, dh, dl) = (self.name("hi"), self.name("lo"), self.name("hi"), self.name("lo"), self.name("d"), self.name("d"));
        // `\u{1b}`: lower-case hex digits, no leading zero
        let written = test_eq(
            self,
            var(&hi),
            int(0),
            concat(vec![text("\\u{"), var(&dh), var(&dl), text("}")], kv()),
            concat(vec![text("\\u{"), var(&dl), text("}")], kv()),
        );
        let unicode = prim_then("div", vec![var(&b), int(16)], &hi, Rep::I64,
            prim_then("mod", vec![var(&b), int(16)], &lo, Rep::I64,
                prim_then("add", vec![var(&hi), int(1)], &h1, Rep::I64,
                    prim_then("add", vec![var(&lo), int(1)], &l1, Rep::I64,
                        prim_then("stringSlice", vec![digits.clone(), var(&hi), var(&h1)], &dh, Rep::Str,
                            prim_then("stringSlice", vec![digits, var(&lo), var(&l1)], &dl, Rep::Str, written))))));
        // another byte below a space, or DEL; anything else is itself
        let control = test_lt(self, var(&b), int(32), Statement::Cut(text(""), kv()), unicode.clone());
        let mut body = test_eq(self, var(&b), int(127), control, unicode);
        for (code, escaped) in [(0, "\\0"), (13, "\\r"), (9, "\\t"), (10, "\\n"), (92, "\\\\"), (34, "\\\"")] {
            body = test_eq(self, var(&b), int(code), body, Statement::Cut(text(escaped), kv()));
        }
        self.push_def(&byte, vec![(b, Rep::I64)], k, body);
    }

    fn push_def(&mut self, symbol: &Symbol, params: Vec<(String, Rep)>, k: String, body: Statement) {
        self.show_defs.push(Def {
            symbol: symbol.clone(),
            rep_vars: Vec::new(),
            effect_vars: Vec::new(),
            params: params.into_iter().map(|(name, rep)| Binder { name, rep }).collect(),
            conts: vec![k],
            body,
        });
    }

    fn push_show(&mut self, symbol: &Symbol, x: String, k: String, body: Statement) {
        self.show_defs.push(Def {
            symbol: symbol.clone(),
            rep_vars: Vec::new(),
            effect_vars: Vec::new(),
            params: vec![Binder { name: x, rep: Rep::Ptr }],
            conts: vec![k],
            body,
        });
    }
}

/// The strings, joined, to `k`.
fn concat(parts: Vec<Producer>, k: Consumer) -> Statement {
    Statement::Prim(CONCAT.into(), prim_args(CONCAT, parts), vec![k])
}

fn str_binder(name: String, body: Statement) -> Consumer {
    Consumer::MuTilde(Binder { name, rep: Rep::Str }, Box::new(body))
}

/// `prim op(args; μ̃ x: rep. body)`
fn prim_then(op: &str, args: Vec<Producer>, x: &str, rep: Rep, body: Statement) -> Statement {
    Statement::Prim(op.into(), args, vec![Consumer::MuTilde(Binder { name: x.to_string(), rep }, Box::new(body))])
}

/// `a == b`: `no` if not, `yes` if so.
fn test_eq(l: &mut Lower<'_>, a: Producer, b: Producer, no: Statement, yes: Statement) -> Statement {
    Statement::Prim("eq".into(), vec![a, b], vec![unit_binder(l.name("u"), no), unit_binder(l.name("u"), yes)])
}

/// `a < b`: `no` if not, `yes` if so.
fn test_lt(l: &mut Lower<'_>, a: Producer, b: Producer, no: Statement, yes: Statement) -> Statement {
    Statement::Prim("lt".into(), vec![a, b], vec![unit_binder(l.name("u"), no), unit_binder(l.name("u"), yes)])
}

/// A test's continuation, which receives `unit`.
fn unit_binder(name: String, body: Statement) -> Consumer {
    Consumer::MuTilde(Binder { name, rep: Rep::Unit }, Box::new(body))
}

/// What shows a type: `idyll:Main/"#show Maybe (Int, Bool)"`, the type as
/// it is written, in a quoted segment of its own.
fn named(package: &str, what: &str, key: &str) -> Symbol {
    Symbol { lang: "idyll".into(), package: package.into(), path: vec![format!("#{what} {key}")] }
}
