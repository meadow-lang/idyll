//! `@test` definitions, and the program `idyll test` runs: each test, of
//! type `() -> ()` with at most the effects the runtime handles (as `main`
//! is), called in the order they were written by `Std.Test`'s runner, one
//! failing not stopping the rest:
//!
//! ```text
//! let s = testsStart n in
//! let r0 = testCase "Module.first" first in
//! ..
//! testsDone (r0 + ..) n
//! ```
//!
//! `testCase` handles the test's `Test` effect, prints how it went and
//! gives back `1` or `0`; `testsDone` prints the total and exits with status
//! 1 if any failed.

use super::core::Core;
use super::packages::CompiledPackage;
use super::value::Value;
use super::{Checker, Ctx, TypeErrorKind};
use idyllic_ast::Lit;
use idyllic_builtins::Prim;
use idyllic_hir as hir;
use std::rc::Rc;

impl Checker {
    /// The program that runs `m`'s tests, each checked to be one; `None`
    /// (reported) if `Std.Test`'s runner is not there to run them with.
    pub(super) fn tests_program(&mut self, ctx: &Ctx, m: &hir::Module, deps: &[&CompiledPackage], world: Rc<Value>) -> Option<Rc<Core>> {
        let runner = |name: &str| -> Option<Rc<Core>> {
            let u = deps.iter().filter(|p| p.name == "Std").find_map(|p| {
                p.names.exports.iter().find(|(path, x, _, _)| path.len() == 1 && path[0] == "Test" && x == name).map(|e| e.2)
            })?;
            ctx.lookup(&hir::Ident { unique: Some(u), text: name.into() }).map(|(c, _, _)| c)
        };
        let (Some(start), Some(case), Some(done)) = (runner("testsStart"), runner("testCase"), runner("testsDone")) else {
            self.error(hir::NodeId(0), TypeErrorKind::NoTestRunner);
            return None;
        };
        // each test, by its bind, checked as `main` is
        let binds: Vec<&hir::Bind> = m.groups.iter().flat_map(hir::BindGroup::binds).collect();
        let mut tests = Vec::new();
        for &u in &m.tests {
            let Some(b) = binds.iter().find(|b| b.name.unique == Some(u)) else { continue };
            if !self.check_test(ctx, b, world.clone()) {
                continue;
            }
            let Some((core, _, _)) = ctx.lookup(&b.name) else { continue };
            let name = match m.qualifiers.get(&u) {
                Some(module) => format!("{module}.{}", b.name.text),
                None => b.name.text.clone(),
            };
            tests.push((name, core));
        }
        let n = tests.len();
        let app = |f: Rc<Core>, a: Rc<Core>| Rc::new(Core::App(f, a));
        let int = |k: usize| Rc::new(Core::Lit(Lit::Int(k as i64)));
        // under `d` lets, a variable of the context is `d` further out
        let shifted = |c: &Rc<Core>, d: usize| match &**c {
            Core::Var(i, x) => Rc::new(Core::Var(i + d, x.clone())),
            _ => c.clone(),
        };
        // the sum of the results, the innermost let the last test's
        let total = (0..n).map(|i| Rc::new(Core::Var(n - 1 - i, format!("r{i}")))).reduce(|a, b| {
            app(app(Rc::new(Core::Prim(Prim::IntAdd)), a), b)
        });
        let total = total.unwrap_or_else(|| int(0));
        let mut body = app(app(shifted(&done, n + 1), total), int(n));
        for (i, (name, test)) in tests.iter().enumerate().rev() {
            let d = i + 1;
            let call = app(app(shifted(&case, d), Rc::new(Core::Lit(Lit::Str(name.clone())))), shifted(test, d));
            body = Rc::new(Core::Let(format!("r{i}"), call, body));
        }
        Some(Rc::new(Core::Let("s".into(), app(start, int(n)), body)))
    }

    /// A test is `() -> ()`, with at most the effects the runtime handles,
    /// as `main` is.
    fn check_test(&mut self, ctx: &Ctx, b: &hir::Bind, world: Rc<Value>) -> bool {
        let Some((_, ty, _)) = ctx.lookup(&b.name) else { return false };
        let unit = || Rc::new(Value::LitTy(idyllic_builtins::Builtin::Unit));
        let fits = match &*self.force(ty.clone()) {
            Value::Pi { name, icit: idyllic_ast::Icit::Explicit, dom, cod, .. } => {
                let (result, effects) = self.split_comp(self.apply(cod, super::value::vvar(ctx.lvl, name.clone())));
                self.unify(ctx.lvl, dom.clone(), unit()) && self.unify(ctx.lvl + 1, result, unit()) && self.effects_fit(ctx.lvl + 1, effects, world)
            }
            Value::Error => true,
            _ => false,
        };
        if !fits {
            let ty = self.quote(ctx.lvl, ty);
            self.error(b.id, TypeErrorKind::TestType { name: b.name.text.clone(), ty });
        }
        fits
    }
}
