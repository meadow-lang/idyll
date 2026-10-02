//! The natives the runtime has no primitive for, as code of the program's
//! own: `arrayToList`, `arrayFromList` of a list that is not written out,
//! and the mutable arrays. Each is a `def`, generic in the element type the
//! way any Cut function is (a representation variable and its descriptor),
//! made the first time a program uses it, out of the array primitives the
//! runtime does have.
//!
//! A mutable array is the runtime's array, used linearly: Cut's arrays are
//! persistent, so a write copies where Idyll's own back ends write in
//! place, which changes how fast it is and nothing else.

use super::*;

impl Lower<'_> {
    /// What a native with no primitive is called as, and its parameters
    /// (its type parameter's descriptor, then what it takes); `None` for a
    /// native that has a primitive, or a type.
    pub(super) fn native_helper(&mut self, n: Native) -> Option<(Symbol, Rc<Vec<Param>>)> {
        let values: &[Rep] = match n {
            Native::ArrayToList | Native::ArrayFromList | Native::MArrayLen | Native::MArrayFreeze | Native::MArrayThaw => {
                &[Rep::Ptr]
            }
            Native::MArrayNew => &[Rep::I64, Rep::Any],
            Native::MArrayGet => &[Rep::Ptr, Rep::I64],
            _ => return None,
        };
        let params: Vec<Param> = std::iter::once(Param::Desc).chain(values.iter().cloned().map(Param::Value)).collect();
        let symbol = Symbol::idyll(&self.package, &format!("#{}", n.name()));
        if self.natives_made.insert(n) {
            self.make_native(n, &symbol)?;
        }
        Some((symbol, Rc::new(params)))
    }

    /// A constructor of the prelude's, by name: its symbol, its data type
    /// declared for the program.
    fn prelude_ctor(&mut self, name: &str) -> Option<Symbol> {
        let (u, data) = self
            .ctors
            .iter()
            .find(|(u, c)| c.name == name && self.symbols.get(u).is_some_and(|(p, _)| p == "Prelude"))
            .map(|(u, c)| (*u, c.data))?;
        self.datas.insert(data);
        self.symbol(u)
    }

    fn make_native(&mut self, n: Native, symbol: &Symbol) -> Option<()> {
        let (d, a) = (self.name("d"), self.name("a"));
        let elem = Rep::Var(a.clone());
        let k = self.name("k");
        let kv = || Consumer::Var(k.clone());
        let var = |x: &String| Producer::Var(x.clone());
        let bind = |name: &String, rep: Rep, body: Statement| Consumer::MuTilde(Binder { name: name.clone(), rep }, Box::new(body));
        let def = |symbol: &Symbol, params: Vec<Binder>, k: &String, body: Statement| Def {
            symbol: symbol.clone(),
            rep_vars: vec![(a.clone(), d.clone())],
            effect_vars: Vec::new(),
            params: std::iter::once(Binder { name: d.clone(), rep: Rep::Desc }).chain(params).collect(),
            conts: vec![k.clone()],
            body,
        };
        match n {
            // from the last element back, consing each onto what follows it
            Native::ArrayToList => {
                let (nil, cons) = (self.prelude_ctor("Nil")?, self.prelude_ctor("::")?);
                let go = symbol.child("go");
                let (xs, len, i) = (self.name("xs"), self.name("n"), self.name("i"));
                let body = Statement::Prim(
                    "arrayLen".into(),
                    vec![var(&xs)],
                    vec![bind(&len, Rep::I64, Statement::Prim(
                        "sub".into(),
                        vec![var(&len), Producer::Int(1)],
                        vec![bind(&i, Rep::I64, Statement::Call(go.clone(), vec![var(&d), var(&xs), var(&i), Producer::Con(nil, Vec::new())], vec![kv()]))],
                    ))],
                );
                self.defs.push(def(symbol, vec![Binder { name: xs, rep: Rep::Ptr }], &k, body));
                let (k2, xs, i, acc, e, c, j, u, w) = (
                    self.name("k"), self.name("xs"), self.name("i"), self.name("acc"), self.name("e"), self.name("c"), self.name("j"), self.name("u"), self.name("u"),
                );
                let step = Statement::Prim(
                    "arrayGet".into(),
                    vec![var(&xs), var(&i)],
                    vec![bind(&e, elem.clone(), Statement::Cut(
                        Producer::Con(cons, vec![var(&e), var(&acc)]),
                        bind(&c, Rep::Ptr, Statement::Prim(
                            "sub".into(),
                            vec![var(&i), Producer::Int(1)],
                            vec![bind(&j, Rep::I64, Statement::Call(go.clone(), vec![var(&d), var(&xs), var(&j), var(&c)], vec![Consumer::Var(k2.clone())]))],
                        )),
                    ))],
                );
                let body = Statement::Prim(
                    "lt".into(),
                    vec![var(&i), Producer::Int(0)],
                    vec![bind(&u, Rep::Unit, step), bind(&w, Rep::Unit, Statement::Cut(var(&acc), Consumer::Var(k2.clone())))],
                );
                let params = vec![Binder { name: xs, rep: Rep::Ptr }, Binder { name: i, rep: Rep::I64 }, Binder { name: acc, rep: Rep::Ptr }];
                self.defs.push(def(&go, params, &k2, body));
            }
            // each element pushed in turn onto what has been made so far
            Native::ArrayFromList => {
                let (nil, cons) = (self.prelude_ctor("Nil")?, self.prelude_ctor("::")?);
                let go = symbol.child("go");
                let list = self.name("list");
                let body = Statement::Call(go.clone(), vec![var(&d), var(&list), Producer::Array(Vec::new())], vec![kv()]);
                self.defs.push(def(symbol, vec![Binder { name: list, rep: Rep::Ptr }], &k, body));
                let (k2, l, acc, h, t, pushed) = (self.name("k"), self.name("l"), self.name("acc"), self.name("h"), self.name("t"), self.name("a"));
                let arms = vec![
                    Arm { ctor: Some(nil), fields: Vec::new(), body: Statement::Cut(var(&acc), Consumer::Var(k2.clone())) },
                    Arm {
                        ctor: Some(cons),
                        fields: vec![Binder { name: h.clone(), rep: elem.clone() }, Binder { name: t.clone(), rep: Rep::Ptr }],
                        body: Statement::Prim(
                            "arrayPush".into(),
                            vec![var(&acc), var(&h)],
                            vec![bind(&pushed, Rep::Ptr, Statement::Call(go.clone(), vec![var(&d), var(&t), var(&pushed)], vec![Consumer::Var(k2.clone())]))],
                        ),
                    },
                ];
                let body = Statement::Cut(var(&l), Consumer::Case(arms));
                let params = vec![Binder { name: l, rep: Rep::Ptr }, Binder { name: acc, rep: Rep::Ptr }];
                self.defs.push(def(&go, params, &k2, body));
            }
            // `x`, pushed `n` times
            Native::MArrayNew => {
                let go = symbol.child("go");
                let (count, x) = (self.name("n"), self.name("x"));
                let body = Statement::Call(go.clone(), vec![var(&d), var(&count), var(&x), Producer::Array(Vec::new())], vec![kv()]);
                self.defs.push(def(symbol, vec![Binder { name: count, rep: Rep::I64 }, Binder { name: x, rep: elem.clone() }], &k, body));
                let (k2, left, x, acc, pushed, fewer, u, w) =
                    (self.name("k"), self.name("n"), self.name("x"), self.name("acc"), self.name("a"), self.name("m"), self.name("u"), self.name("u"));
                let more = Statement::Prim(
                    "arrayPush".into(),
                    vec![var(&acc), var(&x)],
                    vec![bind(&pushed, Rep::Ptr, Statement::Prim(
                        "sub".into(),
                        vec![var(&left), Producer::Int(1)],
                        vec![bind(&fewer, Rep::I64, Statement::Call(go.clone(), vec![var(&d), var(&fewer), var(&x), var(&pushed)], vec![Consumer::Var(k2.clone())]))],
                    ))],
                );
                let body = Statement::Prim(
                    "lt".into(),
                    vec![Producer::Int(0), var(&left)],
                    vec![bind(&u, Rep::Unit, Statement::Cut(var(&acc), Consumer::Var(k2.clone()))), bind(&w, Rep::Unit, more)],
                );
                let params = vec![
                    Binder { name: left, rep: Rep::I64 },
                    Binder { name: x, rep: elem.clone() },
                    Binder { name: acc, rep: Rep::Ptr },
                ];
                self.defs.push(def(&go, params, &k2, body));
            }
            // a read gives back the array, with what it read boxed: the
            // prelude's `Pair`, the box the IR's one-element tuple
            Native::MArrayLen => {
                let pair = self.prelude_ctor("Pair")?;
                let (m, len) = (self.name("m"), self.name("n"));
                let answer = Producer::Con(pair, vec![Producer::Tuple(vec![var(&len)]), var(&m)]);
                let body = Statement::Prim("arrayLen".into(), vec![var(&m)], vec![bind(&len, Rep::I64, Statement::Cut(answer, kv()))]);
                self.defs.push(def(symbol, vec![Binder { name: m, rep: Rep::Ptr }], &k, body));
            }
            Native::MArrayGet => {
                let pair = self.prelude_ctor("Pair")?;
                let (m, i, e) = (self.name("m"), self.name("i"), self.name("e"));
                let answer = Producer::Con(pair, vec![Producer::Tuple(vec![var(&e)]), var(&m)]);
                let body = Statement::Prim("arrayGet".into(), vec![var(&m), var(&i)], vec![bind(&e, elem.clone(), Statement::Cut(answer, kv()))]);
                self.defs.push(def(symbol, vec![Binder { name: m, rep: Rep::Ptr }, Binder { name: i, rep: Rep::I64 }], &k, body));
            }
            // the same array, under the other type
            Native::MArrayFreeze | Native::MArrayThaw => {
                let m = self.name("m");
                let body = Statement::Cut(var(&m), kv());
                self.defs.push(def(symbol, vec![Binder { name: m, rep: Rep::Ptr }], &k, body));
            }
            _ => return None,
        }
        Some(())
    }
}
