//! Packages compiled apart, as Meadow compiles them: a package is checked
//! against the *compiled* packages it depends on, never their source, and
//! what checking it makes is a [`CompiledPackage`] of its own, which is what
//! its dependents are checked against, what a build saves to reuse, and what
//! is linked into a program.
//!
//! A compiled package has two halves.
//!
//! **Its interface**, for checking a dependent: what it calls everything it
//! gives (for `use`), the types of its constants, and, unlike Meadow's, the
//! *bodies* of its transparent definitions, since a dependent may compute
//! with them in its types (`plus n Z` must be `n` in someone else's package
//! too).  Everything there is closed: where the package's own definitions
//! were variables of its context, they are constants, `Core::Opaque`, whose
//! values a dependent keeps among its globals.
//!
//! **Its code**, for linking: the top-level definitions in the order they
//! are computed, and the recursive ones.  Within a package a definition
//! refers to the ones before it, and to the prelude's, by index, and to every
//! other package's as a constant.  Linking nests every package's definitions
//! in dependency order, so each is computed once, before what depends on it,
//! as a program's modules are initialised in OCaml: and it renumbers each
//! package's variables, and turns its constants for other packages'
//! definitions into the variables they now are.
//!
//! Every package of a build names its own things with uniques from a range
//! of its own (see `idyllic-rename`), so packages linked together never share
//! one.  The prelude is named the same in every package, so its types are
//! the same types everywhere.

use super::core::{Core, Lvl};
use super::handlers::{EffectInfo, OpInfo};
use super::termination::Unfold;
use super::value::Env;
use super::{BinderInfo, Checker, Ctx, CtorInfo, RecDef, TypeCheckResult};
use idyllic_hir as hir;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// A constant a dependent knows of: its type, what it is if it is a
/// transparent definition, and how it unfolds if it is a recursive one that
/// terminates.  All closed.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ImportedConstant {
    pub unique: usize,
    pub name: String,
    pub ty: Rc<Core>,
    pub value: Option<Rc<Core>>,
    /// how many arguments it takes before it can compute, and its case tree
    /// under them
    pub unfold: Option<(usize, Rc<Core>)>,
}

/// What compiling a package makes: see the module docs.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CompiledPackage {
    pub name: String,
    /// the uniques it named its own things with
    pub uniques: std::ops::Range<usize>,
    /// what its modules give, for its dependents' `use`s
    pub names: hir::PackageNames,
    /// the interface proper
    pub constants: Vec<ImportedConstant>,
    pub ctors: Vec<(usize, CtorInfo)>,
    pub datas: Vec<(usize, Vec<usize>)>,
    pub positive_params: Vec<(usize, Vec<bool>)>,
    pub seqs: Vec<(usize, (usize, usize))>,
    pub effects: Vec<(usize, EffectInfo)>,
    pub ops: Vec<(usize, OpInfo)>,
    /// its natives, which its code may use
    pub natives: Vec<(usize, super::natives::NativeInfo)>,
    /// its code: where its own definitions start in its context (after the
    /// builtins and the prelude's), then those definitions in order, its
    /// recursive ones, and its trailing expression with its type
    pub base: Lvl,
    pub lets: Vec<(usize, String, Rc<Core>)>,
    pub rec_defs: Vec<RecDef>,
    pub body: Option<(Rc<Core>, Rc<Core>)>,
    /// whether that expression is `main ()`, the package having none of
    /// its own
    pub main: bool,
    /// its names, by unique, with its package's name and their paths in it,
    /// for the symbols a back end names things by
    pub symbols: Vec<(usize, String, String)>,
    /// what its metavariables were solved with, which its code may mention
    pub meta_solutions: Vec<Option<Rc<Core>>>,
    /// its definitions offered to the search for an auto argument
    #[serde(default)]
    pub hints: Vec<usize>,
    /// its traits' records
    #[serde(default)]
    pub traits: Vec<usize>,
    /// its instances with no parameters, and those of their fields that are
    /// a constant or a primitive (see [`Checker::specialize`])
    #[serde(default)]
    pub instance_fields: Vec<(usize, Vec<Option<Rc<Core>>>)>,
}

impl Checker {
    /// Declare what the packages depended on give: each constant with its
    /// type, a transparent one with its value, and everything known about
    /// their data types and effects.
    pub(super) fn import(&mut self, ctx: &Ctx, deps: &[&CompiledPackage]) -> Ctx {
        let mut ctx = ctx.clone();
        let empty = Env::empty();
        for p in deps {
            for c in &p.constants {
                let ty = self.eval(&empty, &c.ty);
                let x = hir::Ident { unique: Some(c.unique), text: c.name.clone() };
                ctx = self.declare_constant(&ctx, &x, ty);
                if let Some(v) = &c.value {
                    let v = self.eval(&empty, v);
                    self.globals.insert(c.unique, v);
                }
                if let Some((arity, body)) = &c.unfold {
                    self.transparent.insert(c.unique, Unfold::closed(*arity, body.clone()));
                }
            }
            self.ctors.extend(p.ctors.iter().cloned());
            self.datas.extend(p.datas.iter().cloned());
            self.positive_params.extend(p.positive_params.iter().cloned());
            self.seqs.extend(p.seqs.iter().cloned());
            self.effects.extend(p.effects.iter().cloned());
            self.ops.extend(p.ops.iter().cloned());
            self.natives.extend(p.natives.iter().cloned());
        }
        ctx
    }

    /// Close a term quoted in the top-level context `ctx`: a variable for a
    /// top-level definition becomes the constant it is to everyone else.
    fn close(&self, ctx: &Ctx, core: &Rc<Core>) -> Rc<Core> {
        self.close_at(ctx, ctx.lvl, ctx.lvl, core)
    }

    /// The same, for a term quoted at level `at`, where only the levels
    /// below `limit` are top-level definitions.
    pub(super) fn close_at(&self, ctx: &Ctx, at: Lvl, limit: Lvl, core: &Rc<Core>) -> Rc<Core> {
        core.rewrite(0, &|c, d| match c {
            Core::Var(i, n) if *i >= d && i - d < at => {
                let level = at - 1 - (i - d);
                if level >= limit {
                    return None;
                }
                let unique = ctx.binders.get(level)?.key?;
                Some(Rc::new(Core::Opaque(unique, n.clone())))
            }
            _ => None,
        })
    }

    /// The type of a constant, closed.
    fn constant_type(&self, ctx: &Ctx, unique: usize) -> Option<Rc<Core>> {
        let ty = self.constants.get(&unique)?.clone();
        Some(self.close(ctx, &self.quote(ctx.lvl, ty)))
    }

    /// What compiling the module made, as a package: see the module docs.
    /// `ctx` is the top-level context once every definition is checked;
    /// `lets` are the definitions' re-bindings, one per group.
    pub(super) fn compile_package(
        &self,
        m: &hir::Module,
        ctx: &Ctx,
        lets: &[super::elab::LetWrap],
        body: Option<(Rc<Core>, Rc<Core>)>,
        rec_defs: &[RecDef],
        meta_solutions: Vec<Option<Rc<Core>>>,
    ) -> CompiledPackage {
        // its own: named from its own range of uniques (not the builtins',
        // the prelude's or another package's)
        let own = |u: &Option<usize>| u.is_some_and(|u| (m.uniques.0..m.uniques.1).contains(&u));
        let mut constants = Vec::new();
        // a constant with no slot: a data type, a constructor, an effect, an
        // operation or a recursive definition
        let constant = |this: &Self, x: &hir::Ident| -> Option<ImportedConstant> {
            let unique = x.unique?;
            let ty = this.constant_type(ctx, unique)?;
            let unfold = this.transparent.get(&unique).map(|u| u.export(this, ctx));
            Some(ImportedConstant { unique, name: x.text.clone(), ty, value: None, unfold })
        };
        for d in m.datas.iter().filter(|d| own(&d.name.unique)) {
            constants.extend(constant(self, &d.name));
            constants.extend(d.ctors.iter().filter_map(|c| constant(self, &c.name)));
        }
        for e in m.effects.iter().filter(|e| own(&e.name.unique)) {
            constants.extend(constant(self, &e.name));
            constants.extend(e.ops.iter().filter_map(|op| constant(self, &op.name)));
        }
        for n in m.natives.iter().filter(|n| own(&n.name.unique)) {
            constants.extend(constant(self, &n.name));
        }
        // the definitions: a non-recursive one has a slot, and a value the
        // types may see unless computing it has effects; a recursive one is a
        // constant already
        let mut code = Vec::new();
        let mut base = None;
        for (group, wrap) in m.groups.iter().zip(lets) {
            let binds = group.binds();
            if !binds.iter().any(|b| own(&b.name.unique)) {
                continue;
            }
            for b in binds {
                let Some(unique) = b.name.unique else { continue };
                match ctx.names.get(&unique) {
                    Some(super::Entry::Local(level)) => {
                        let binder = &ctx.binders[*level];
                        base.get_or_insert(*level);
                        let ty = self.close(ctx, &self.quote(ctx.lvl, binder.ty.clone()));
                        let value = (binder.info == BinderInfo::Defined)
                            .then(|| self.close(ctx, &self.quote(ctx.lvl, ctx.value(*level))));
                        constants.push(ImportedConstant { unique, name: b.name.text.clone(), ty, value, unfold: None });
                    }
                    _ => constants.extend(constant(self, &b.name)),
                }
            }
            if let (hir::BindGroup::NonRec(b), Some((name, rhs))) = (group, wrap) {
                code.extend(b.name.unique.map(|u| (u, name.clone(), rhs.clone())));
            }
        }
        let first_own = ctx.binders.iter().position(|b| own(&b.key));
        let base = base.or(first_own).unwrap_or(ctx.lvl);
        let pick = |set: &HashSet<usize>, u: &usize| set.contains(u);
        let own_set: HashSet<usize> = (m.uniques.0..m.uniques.1).collect();
        CompiledPackage {
            name: m.package.clone(),
            uniques: m.uniques.0..m.uniques.1,
            names: m.names.clone(),
            constants,
            ctors: self.ctors.iter().filter(|(u, _)| pick(&own_set, u)).map(|(u, c)| (*u, c.clone())).collect(),
            datas: self.datas.iter().filter(|(u, _)| pick(&own_set, u)).map(|(u, c)| (*u, c.clone())).collect(),
            positive_params: self.positive_params.iter().filter(|(u, _)| pick(&own_set, u)).map(|(u, p)| (*u, p.clone())).collect(),
            seqs: self.seqs.iter().filter(|(u, _)| pick(&own_set, u)).map(|(u, s)| (*u, *s)).collect(),
            effects: self.effects.iter().filter(|(u, _)| pick(&own_set, u)).map(|(u, e)| (*u, e.clone())).collect(),
            ops: self.ops.iter().filter(|(u, _)| pick(&own_set, u)).map(|(u, o)| (*u, o.clone())).collect(),
            natives: self.natives.iter().filter(|(u, _)| pick(&own_set, u)).map(|(u, n)| (*u, *n)).collect(),
            base,
            lets: code,
            rec_defs: rec_defs.iter().filter(|d| own_set.contains(&d.unique)).cloned().collect(),
            body,
            main: false,
            symbols: Vec::new(),
            meta_solutions,
            hints: {
                let mut hints: Vec<usize> = self.hints.iter().copied().filter(|u| own_set.contains(u)).collect();
                hints.sort_unstable();
                hints
            },
            traits: m.datas.iter().filter(|d| d.is_trait).filter_map(|d| d.name.unique).filter(|u| own_set.contains(u)).collect(),
            instance_fields: {
                let mut fields: Vec<(usize, Vec<Option<Rc<Core>>>)> =
                    self.instance_fields.iter().filter(|(u, _)| own_set.contains(u)).map(|(u, f)| (*u, f.clone())).collect();
                fields.sort_by_key(|(u, _)| *u);
                fields
            },
        }
    }
}

/// A program put together from a root package (checked, with its own code
/// and the prelude's) and the compiled packages it depends on, directly or
/// not, in an order where each comes after those it depends on.  Replaces
/// the root's program, recursive definitions and tables with the whole
/// program's; its metavariables are solved away.
/// Whether the program refers to anything `dep` declares: a definition, a
/// constructor (built, or matched on), an operation.
pub fn uses(root: &TypeCheckResult, dep: &CompiledPackage) -> bool {
    fn mentions(c: &Core, range: &std::ops::Range<usize>) -> bool {
        let go = |c: &Core| mentions(c, range);
        match c {
            Core::Opaque(u, _) => range.contains(u),
            Core::Pi { grade, in_type, dom, cod, .. } => go(grade) || go(in_type) || go(dom) || go(cod),
            Core::Lam(_, b) | Core::BoxIntro(b) | Core::Unbox(b) | Core::Runtime(_, b) => go(b),
            Core::Let(_, a, b)
            | Core::App(a, b)
            | Core::BoxTy(a, b)
            | Core::GradeAdd(a, b)
            | Core::GradeMul(a, b)
            | Core::GradeRange(a, b)
            | Core::Comp(a, b) => go(a) || go(b),
            Core::Case(s, alts) => {
                go(s) || alts.iter().any(|a| matches!(a.head, super::core::Head::Con(u) if range.contains(&u)) || go(&a.body))
            }
            Core::Handle(b, r, ops) => go(b) || go(r) || ops.iter().any(|op| range.contains(&op.op) || go(&op.body)),
            Core::Row(labels, tails) => labels.iter().chain(tails).any(|x| go(x)),
            _ => false,
        }
    }
    let range = &dep.uniques;
    root.program.as_ref().is_some_and(|p| mentions(p, range)) || root.rec_defs.iter().any(|d| mentions(&d.core, range))
}

pub fn link(root: &mut TypeCheckResult, deps: &[&CompiledPackage]) {
    let Some(own) = root.compiled.clone() else { return };
    let base = own.base;
    // each package's definitions get the slots after those before it
    let mut slots: HashMap<usize, Lvl> = HashMap::new();
    let mut offsets = Vec::new();
    let mut next = base;
    for p in deps.iter().copied().chain(std::iter::once(&own)) {
        offsets.push(next);
        for (u, _, _) in &p.lets {
            slots.insert(*u, next);
            next += 1;
        }
    }
    let relocate = |p: &CompiledPackage, offset: Lvl, at: Lvl, core: &Rc<Core>| -> Rc<Core> {
        let moved = |l: Lvl| if l < p.base { l } else { l - p.base + offset };
        let placed = moved(at);
        zonk(p, core).rewrite(0, &|c, d| match c {
            Core::Var(i, n) if *i >= d => {
                let level = at - 1 - (i - d);
                Some(Rc::new(Core::Var(placed + d - 1 - moved(level), n.clone())))
            }
            Core::Opaque(u, n) => slots.get(u).map(|&s| Rc::new(Core::Var(placed + d - 1 - s, n.clone()))),
            _ => None,
        })
    };
    let mut lets: Vec<(String, Rc<Core>)> = Vec::new();
    let packages: Vec<&CompiledPackage> = deps.iter().copied().chain(std::iter::once(&own)).collect();
    // the prelude's recursive definitions, which are the same in every
    // package and no package's own: the root's, as its prelude `let`s are
    let mut rec_defs: Vec<RecDef> =
        root.rec_defs.iter().filter(|d| !packages.iter().any(|p| p.uniques.contains(&d.unique))).cloned().collect();
    for (p, &offset) in packages.iter().zip(&offsets) {
        for (k, (_, name, rhs)) in p.lets.iter().enumerate() {
            lets.push((name.clone(), relocate(p, offset, p.base + k, rhs)));
        }
        for d in &p.rec_defs {
            let lvl = if d.lvl < p.base { d.lvl } else { d.lvl - p.base + offset };
            rec_defs.push(RecDef { lvl, core: relocate(p, offset, d.lvl, &d.core), ..d.clone() });
        }
    }
    let Some((body, ty)) = &own.body else { return };
    let at = own.base + own.lets.len();
    let body = relocate(&own, *offsets.last().expect("the root"), at, body);
    // the prelude's own definitions, outermost: they are the same in every
    // package, and the root's are taken
    let prelude_lets = root.prelude_lets.clone();
    let program = prelude_lets
        .into_iter()
        .chain(lets)
        .rev()
        .fold(body, |acc, (name, rhs)| Rc::new(Core::Let(name, rhs, acc)));
    // (the root's own are among its tables already, unless it is linked
    // from its compiled package alone)
    for p in deps.iter().copied().chain(std::iter::once(&own)) {
        root.ctors.extend(p.ctors.iter().cloned());
        root.ops.extend(p.ops.iter().cloned());
        root.type_constructors.extend(p.datas.iter().map(|(u, _)| *u));
        root.natives.extend(p.natives.iter().cloned());
        root.type_constructors.extend(p.natives.iter().filter(|(_, n)| n.native.is_type()).map(|(u, _)| *u));
    }
    root.program = Some(program);
    root.rec_defs = rec_defs;
    // and the root's own constants, which its checker has not seen when it
    // is linked from its compiled package alone: without them, what its
    // recursive definitions answer has no type to lower by
    let mut known: Vec<&CompiledPackage> = deps.to_vec();
    if own.constants.iter().any(|c| c.value.is_none() && !root.typer.knows(c.unique)) {
        known.push(&own);
    }
    root.typer.import(&known);
    // the top-level `let`s: the prelude's, then each package's in order
    let prelude = root.prelude_lets.len();
    root.toplevel = root.toplevel.iter().take(prelude).copied().chain(packages.iter().flat_map(|p| p.lets.iter().map(|(u, ..)| *u))).collect();
    for p in deps.iter().copied().chain(std::iter::once(&own)) {
        root.symbols.extend(p.symbols.iter().map(|(u, pkg, path)| (*u, (pkg.clone(), path.clone()))));
    }
    root.body_type = Some(zonk(&own, ty));
    root.meta_solutions = Vec::new();
    root.ran_main = own.main;
}

/// A package compiled a part at a time (see `idyllic::parts`), as one
/// package: what its dependents import and what a program is linked from.
/// Each part's code is moved to the slots after those of the parts before
/// it, and its metavariables, numbered by its own checker, are solved
/// away. The root's part, whose body is the package's, is the last.
pub fn merge(parts: Vec<CompiledPackage>) -> CompiledPackage {
    let mut parts = parts.into_iter();
    let Some(first) = parts.next() else { panic!("a package has a part") };
    let mut out = solved(first);
    for p in parts {
        let offset = out.base + out.lets.len();
        let moved = |l: Lvl| if l < p.base { l } else { l - p.base + offset };
        let relocate = |at: Lvl, core: &Rc<Core>| -> Rc<Core> {
            let placed = moved(at);
            zonk(&p, core).rewrite(0, &|c, d| match c {
                Core::Var(i, n) if *i >= d => {
                    let level = at - 1 - (i - d);
                    Some(Rc::new(Core::Var(placed + d - 1 - moved(level), n.clone())))
                }
                _ => None,
            })
        };
        let lets: Vec<_> = p.lets.iter().enumerate().map(|(k, (u, name, rhs))| (*u, name.clone(), relocate(p.base + k, rhs))).collect();
        let rec_defs: Vec<_> = p.rec_defs.iter().map(|d| RecDef { lvl: moved(d.lvl), core: relocate(d.lvl, &d.core), ..d.clone() }).collect();
        let body = p.body.as_ref().map(|(b, ty)| (relocate(p.base + p.lets.len(), b), zonk(&p, ty)));
        let instance_fields: Vec<_> =
            p.instance_fields.iter().map(|(u, fs)| (*u, fs.iter().map(|f| f.as_ref().map(|c| zonk(&p, c))).collect())).collect();
        let names = &p.names;
        out.names.modules.extend(names.modules.iter().cloned());
        out.names.exports.extend(names.exports.iter().cloned());
        out.names.types.extend(names.types.iter().cloned());
        out.names.effect_ops.extend(names.effect_ops.iter().cloned());
        out.names.fields.extend(names.fields.iter().cloned());
        out.names.defaults.extend(names.defaults.iter().cloned());
        out.names.supers.extend(names.supers.iter().cloned());
        out.names.methods.extend(names.methods.iter().cloned());
        out.uniques = out.uniques.start.min(p.uniques.start)..out.uniques.end.max(p.uniques.end);
        out.constants.extend(p.constants.iter().cloned());
        out.ctors.extend(p.ctors.iter().cloned());
        out.datas.extend(p.datas.iter().cloned());
        out.positive_params.extend(p.positive_params.iter().cloned());
        out.seqs.extend(p.seqs.iter().cloned());
        out.effects.extend(p.effects.iter().cloned());
        out.ops.extend(p.ops.iter().cloned());
        out.natives.extend(p.natives.iter().cloned());
        out.lets.extend(lets);
        out.rec_defs.extend(rec_defs);
        out.body = body;
        out.main = p.main;
        out.symbols.extend(p.symbols.iter().cloned());
        out.hints.extend(p.hints.iter().copied());
        out.traits.extend(p.traits.iter().copied());
        out.instance_fields.extend(instance_fields);
    }
    out.symbols.sort();
    out
}

/// A package with its metavariables solved away, in its code and in its
/// instances' fields.
fn solved(mut p: CompiledPackage) -> CompiledPackage {
    p.lets = p.lets.iter().map(|(u, n, rhs)| (*u, n.clone(), zonk(&p, rhs))).collect();
    p.rec_defs = p.rec_defs.iter().map(|d| RecDef { core: zonk(&p, &d.core), ..d.clone() }).collect();
    p.body = p.body.as_ref().map(|(b, ty)| (zonk(&p, b), zonk(&p, ty)));
    p.instance_fields = p.instance_fields.iter().map(|(u, fs)| (*u, fs.iter().map(|f| f.as_ref().map(|c| zonk(&p, c))).collect())).collect();
    p.meta_solutions = Vec::new();
    p
}

/// A term with every metavariable in it replaced by its solution.
fn zonk(p: &CompiledPackage, core: &Rc<Core>) -> Rc<Core> {
    core.rewrite(0, &|c, _| match c {
        Core::Meta(m) => p.meta_solutions.get(*m).cloned().flatten().map(|s| zonk(p, &s)),
        _ => None,
    })
}
