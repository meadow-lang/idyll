//! Name resolution: surface [`idyllic_ast`] to [`idyllic_hir`].
//!
//! Responsibilities:
//!
//! * Resolve every variable to an [`hir::Ident`] with a scope-unique integer;
//!   report an unbound name for anything not in scope.
//!
//! * Introduce fresh uniques for every binder (lambda / Pi / definition), with
//!   the usual lexical scoping (a Pi/lambda domain does not see the name it
//!   binds; later telescope entries do; declaration groups are recursive).
//!
//! * Pair signatures with their equations, and desugar equations and
//!   patterns: `f x [y] = e` becomes `f = \x b -> let [y] = b in e`.  A
//!   function defined by several equations, or by patterns that are not
//!   just variables, becomes a [`hir::TermKind::Match`] of its equations,
//!   for the type checker's match compiler.
//!
//! * Bind the variables a signature mentions but nothing binds, as implicit
//!   arguments in front: `append : Vec n a -> Vec m a -> Vec (n + m) a`
//!   quantifies over `n`, `a` and `m`, as in Haskell and Idris.
//!
//! * Allocate a fresh [`hir::NodeId`] for every [`hir::Term`] and
//!   [`hir::Bind`], recording its source span in the [`hir::SpanTable`] side
//!   table.
//!
//! `let` / top-level groups are emitted as one conservative
//! [`hir::BindGroup::Rec`] group; `idyllic_scc` refines that into
//! dependency-ordered SCCs.

use idyllic_ast::{self as ast, Icit, LTerm, Visibility};
use idyllic_builtins::{Builtin, Prim, FIRST_USER_UNIQUE};
use idyllic_diagnostics::Diagnostic;
use idyllic_hir::{self as hir, Ident, NodeId, SpanTable};
use idyllic_span::{Span, Spanned};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameError {
    /// A name with no binding in scope.
    Unbound { name: String, span: Span },
    /// Two bindings (or two signatures) of the same name in one group / scope.
    Duplicate { name: String, first: Span, again: Span },
    /// `f : a` with no equation for `f` beside it.
    SignatureWithoutDefinition { name: String, span: Span },
    /// A pattern only a function of one equation of variables may have, in
    /// one that is matched: `[x]`, or `(x : a)`.
    PatternNeedsSimpleEquation { span: Span },
    /// A constructor written bare where nothing brought it into scope: it
    /// lives under its type, as `Nat.Z`.  The types that have one of that
    /// name.
    BareConstructor { name: String, types: Vec<String>, span: Span },
    /// `T.x` where `T` has no constructor `x`
    NoSuchConstructor { ty: String, name: String, span: Span },
    /// `use P` where `P` is neither a module nor a type
    UnknownPath { path: String, span: Span },
    /// `use T (..)` or `use T.*` for something with no constructors to give
    NotAType { path: String, span: Span },
    /// `use T as A`: only a module can be given another name
    AliasForType { path: String, span: Span },
    /// a name another module has, but does not let this one see
    NotVisible { name: String, module: String, span: Span },
    /// `M.x` or `use M (x)` where module `M` has no `x`
    NotInModule { module: String, name: String, span: Span },
    /// a module, where a value or a type is wanted
    ModuleNotValue { path: String, span: Span },
    /// a record built, matched, read or updated wrongly: what, and where
    Record { message: String, label: String, span: Span },
    /// `module M where` inside a file, for a module the package has already
    DuplicateModule { path: String, span: Span },
}

impl RenameError {
    pub fn to_diagnostic(&self) -> Diagnostic {
        match self {
            RenameError::Unbound { name, span } => {
                Diagnostic::error(format!("name not in scope: {name}")).label(*span, "used here")
            }
            RenameError::BareConstructor { name, types, span } => {
                let qualified: Vec<String> = types.iter().map(|t| format!("{t}.{name}")).collect();
                let first = types.first().map_or("T", String::as_str);
                Diagnostic::error(format!("name not in scope: {name}"))
                    .label(*span, format!("a constructor lives under its type: {}", qualified.join(" or ")))
                    .help(format!("write {first}.{name}, or bring the constructors into scope with use {first}.*"))
            }
            RenameError::NoSuchConstructor { ty, name, span } => {
                Diagnostic::error(format!("{ty} has no constructor {name}")).label(*span, "used here")
            }
            RenameError::UnknownPath { path, span } => {
                Diagnostic::error(format!("no module or type {path}")).label(*span, "nothing of this name is in scope")
            }
            RenameError::NotAType { path, span } => Diagnostic::error(format!("{path} has no constructors to bring"))
                .label(*span, "use T.* and use T (C) are for a data type's constructors"),
            RenameError::NotVisible { name, module, span } => {
                Diagnostic::error(format!("{name} is private to {module}"))
                    .label(*span, "used here")
                    .help(format!("mark it @pub(pkg) in {module} to let the rest of the package see it, or @pub for everyone"))
            }
            RenameError::NotInModule { module, name, span } => {
                Diagnostic::error(format!("{module} has no {name}")).label(*span, "used here")
            }
            RenameError::Record { message, label, span } => Diagnostic::error(message.clone()).label(*span, label.clone()),
            RenameError::DuplicateModule { path, span } => Diagnostic::error(format!("the package has a module {path} already"))
                .label(*span, "a second module of this name")
                .help("a module inside a file is the module under that file's: give it a name of its own"),
            RenameError::ModuleNotValue { path, span } => Diagnostic::error(format!("{path} is a module"))
                .label(*span, "a module is not a value; name something in it, as in M.x"),
            RenameError::AliasForType { path, span } => {
                Diagnostic::error(format!("{path} is a type, and only a module takes an alias"))
                    .label(*span, "write the type's constructors as they are, or bring them with use T.*")
            }
            RenameError::Duplicate { name, first, again } => {
                Diagnostic::error(format!("duplicate binding of {name}"))
                    .label(*again, "bound again here")
                    .label(*first, "first bound here")
            }
            RenameError::PatternNeedsSimpleEquation { span } => {
                Diagnostic::error("this pattern is only allowed in a single equation of variables")
                    .label(*span, "annotations, grades and unboxing are not supported in a definition by matching yet")
            }
            RenameError::SignatureWithoutDefinition { name, span } => {
                Diagnostic::error(format!("the signature for {name} has no definition"))
                    .label(*span, "declared here, but never defined")
                    .help(format!("add an equation, as in {name} = ..; to annotate an expression instead, parenthesise it: ({name} : ..)"))
            }
        }
    }
}

pub struct RenameResult {
    pub module: hir::Module,
    pub spans: SpanTable,
    pub errors: Vec<RenameError>,
}

/// One lexical scope: surface spelling to the unique it was bound with, and
/// where it was bound (so a later duplicate can point back at the original).
type Scope = HashMap<String, (usize, Span)>;

/// One module of a package: where it sits in the package's tree of modules
/// (the root is the empty path; `src/Math.idl` is `["Math"]`), and what it
/// says.
pub struct ModuleSource<'a> {
    pub path: Vec<String>,
    pub program: &'a ast::Program,
}

/// What the renamer keeps of each module between its passes.
#[derive(Default)]
struct ModuleState {
    /// which package it is in: 0 for the one being renamed, and then each
    /// dependency's in turn
    package: usize,
    path: Vec<String>,
    /// its own declarations' names
    own: Scope,
    /// what its `use`s bring it
    imports: Scope,
    /// `use M as C`: qualifiers for other modules, by index
    aliases: HashMap<String, usize>,
    /// what other modules may take from it: its declarations, each with how
    /// visible it is, and what it gives on with `@pub use`
    exports: HashMap<String, (usize, Visibility)>,
    /// every meaning its `use`s bring each name, in order: `imports` keeps
    /// the last, and two or more make the name an overload
    brought: HashMap<String, Vec<usize>>,
}

struct Renamer {
    /// innermost last; never empty (the first is the global scope)
    scopes: Vec<Scope>,
    next_unique: usize,
    spans: SpanTable,
    errors: Vec<RenameError>,
    /// each data type's constructors, by the type's unique: they live under
    /// it, as `Nat.Z`, and are in scope bare only where a `use` puts them
    ctors: HashMap<usize, Vec<(String, usize)>>,
    /// each data type's name, by its unique, for messages
    type_names: HashMap<usize, String>,
    /// each type whose constructor has its name (`Pair`, made by `Pair`),
    /// and that constructor: the bare name stands for the type where a type
    /// is written, and for the constructor anywhere else (a pattern, a
    /// term).  Written with a path, it is whichever the path leads to:
    /// `Pair.Pair` the constructor, `Std.Tuple.Pair` the type
    twins: HashMap<usize, usize>,
    /// each record's fields, in order, by the record type's unique: each
    /// one's name and its projection's unique (`Point.x`)
    fields: HashMap<usize, Vec<(String, usize)>>,
    /// the definitions marked `@terminating`
    terminating: std::collections::HashSet<usize>,
    /// the definitions marked `@hint`
    hints: std::collections::HashSet<usize>,
    /// each trait's methods with a default, the default, and whether the
    /// method is a function, by the trait's record type: what an instance
    /// leaving one out gets
    defaults: HashMap<usize, Vec<(String, usize, bool)>>,
    /// each trait's superclass projections, by its record type
    supers: HashMap<usize, Vec<usize>>,
    /// each trait's methods: see [`hir::Module::methods`]
    methods: HashMap<usize, (usize, usize, usize)>,
    /// whether the package's modules are all linear (`--linear`)
    linear_package: bool,
    /// whether the module being renamed is: its unannotated binders get
    /// `[1]`, where elsewhere they mean `[]`
    linear: bool,
    /// whether what is being renamed is a type: a signature, an
    /// annotation, a function type, a constructor's fields
    in_type: bool,
    /// each effect's operations, by its unique: naming the effect in a `use`
    /// brings them too
    effect_ops: HashMap<usize, Vec<(String, usize)>>,
    /// the package's name, and its modules, the root first, then its
    /// dependencies' modules
    package: String,
    modules: Vec<ModuleState>,
    /// the packages depended on, by the names this one calls them
    deps: Vec<String>,
    /// the module whose names are in `scopes`
    current: Option<usize>,
    /// the scopes under every module's: the builtins', and the prelude's
    base: Vec<Scope>,
    /// Whether any declaration of the package says how visible it is.  A
    /// package that never does has no visibility at all: every module sees
    /// all of every other, as Meadow has it for small programs.
    visibility_mentioned: bool,
    /// the module each top-level name of a module other than the root is in,
    /// as written before it from outside (`Math` for `Math.double`)
    qualifiers: HashMap<usize, String>,
    /// the names with more than one meaning where they are written (see
    /// [`hir::Module::overloads`])
    overloads: HashMap<NodeId, Vec<usize>>,
    /// what the last name [`Renamer::use_name`] looked up may mean, when it
    /// is more than one thing: for its caller to give the node it makes
    meanings: Option<Vec<usize>>,
}

/// What binding a module's names leaves to do once every module's are bound.
struct Bound<'a> {
    datas: Vec<&'a ast::LData>,
    data_names: Vec<(Ident, Vec<Ident>)>,
    effect_names: Vec<(Ident, Vec<Ident>)>,
    defs: Vec<Def<'a>>,
    /// `@native` declarations: each name, bound, and its type
    natives: Vec<(Ident, &'a ast::LDecl)>,
}

/// A definition whose name is bound: its equations, and its signature.
type Def<'a> = (Vec<&'a ast::LDecl>, Ident, Option<&'a LTerm>);

/// What a dotted path names.
#[derive(Clone)]
enum Target {
    Module(usize),
    /// a directory of modules with no module of its own, `Geometry` for
    /// `src/Geometry/Shapes.idl`: only a step on the way to one (in this
    /// package)
    Namespace(usize, Vec<String>),
    Name(usize),
}

/// Rename a whole program, one module: its declarations become the
/// top-level bindings and its trailing expression, if any, the body.
pub fn rename_module(prog: &ast::Program, prelude: &ast::Program) -> RenameResult {
    rename_package("Main", &[ModuleSource { path: Vec::new(), program: prog }], prelude, &[], FIRST_USER_UNIQUE, false)
}

/// Rename a package: every module of it at once, so that any may mention
/// any other, even one that mentions it.  Each module is its own namespace,
/// whose names others reach through `use`, or written in full.  The root
/// module (the first) is the one whose trailing expression is the body and
/// whose `main` is run.  For the sake of one-liners, a root that is nothing
/// but `let ... in body` is taken as those declarations and that body.
///
/// The prelude's declarations come first, in scopes of their own, for every
/// module's to shadow.  What packages it depends on give is reached through
/// their names, `use Util (double)`, and only what they make `@pub`.  The
/// package names its own things with uniques from `first_unique` on (the
/// prelude's are the same in every package).
pub fn rename_package(
    package: &str,
    modules: &[ModuleSource<'_>],
    prelude: &ast::Program,
    deps: &[(&str, &hir::PackageNames)],
    first_unique: usize,
    linear: bool,
) -> RenameResult {
    let programs: Vec<&ast::Program> = modules.iter().map(|m| m.program).collect();
    let part = PartOf { siblings: &[], root: true, visibility_mentioned: package_mentions_visibility(&programs) };
    rename_part(package, modules, prelude, deps, first_unique, linear, part)
}

/// Where the modules being renamed are in their package, when they are one
/// part of it (see `idyllic::parts`) rather than all of it.
pub struct PartOf<'a> {
    /// what the package's earlier parts that these modules import give:
    /// their modules are this package's, reached by their paths, with all
    /// the package may see of them
    pub siblings: &'a [&'a hir::PackageNames],
    /// whether the first module is the package's root, whose trailing
    /// expression is the body and whose `main` is run
    pub root: bool,
    /// whether any module of the whole package says how visible something
    /// in it is (see [`package_mentions_visibility`])
    pub visibility_mentioned: bool,
}

/// Whether any of a package's modules, or a module written inside one,
/// says how visible something is: until one does, everything is visible
/// to the whole package.
pub fn package_mentions_visibility(programs: &[&ast::Program]) -> bool {
    programs.iter().any(|p| mentions_visibility(p) || p.modules.iter().any(|(_, inner)| package_mentions_visibility(&[inner])))
}

/// Rename one part of a package: [`rename_package`], with the names of the
/// parts before it.
#[allow(clippy::too_many_arguments)]
pub fn rename_part(
    package: &str,
    modules: &[ModuleSource<'_>],
    prelude: &ast::Program,
    deps: &[(&str, &hir::PackageNames)],
    first_unique: usize,
    linear: bool,
    part: PartOf<'_>,
) -> RenameResult {
    // a module written inside a file is a module of the package's like any
    // other, under the one it is written in
    let mut errors = Vec::new();
    let modules = &lifted(modules, &mut errors);
    let mut r = Renamer::new(package);
    r.errors = errors;
    r.linear_package = linear;
    // the prelude's names get scopes of their own (what it brings in with
    // `use`, then what it declares), so that redefining a builtin shadows it
    // rather than clashing with it
    r.scopes.push(Scope::new());
    r.scopes.push(Scope::new());
    r.linear = prelude.linear;
    let bound = r.bind_program(prelude, &prelude.decls);
    for u in &prelude.uses {
        r.rename_use(u);
    }
    let (mut datas, mut effects, mut natives, prelude_binds) = r.rename_program(prelude, bound);
    r.linear = false;
    let n = r.scopes.len();
    let names = r.scopes[n - 2..].iter().flatten().map(|(x, &(u, _))| (x.clone(), u)).collect();
    r.base = std::mem::take(&mut r.scopes);
    r.visibility_mentioned = part.visibility_mentioned;
    r.modules = modules.iter().map(|m| ModuleState { path: m.path.clone(), ..ModuleState::default() }).collect();
    // the package's earlier parts: modules of its own, renamed already
    for names in part.siblings {
        for path in &names.modules {
            let exports = names.exports.iter().filter(|(m, ..)| m == path).map(|(_, x, u, vis)| (x.clone(), (*u, *vis))).collect();
            r.modules.push(ModuleState { package: 0, path: path.clone(), exports, ..ModuleState::default() });
        }
        r.take_names(names);
    }
    // what the packages depended on give
    for (i, (name, names)) in deps.iter().enumerate() {
        r.deps.push(name.to_string());
        for path in &names.modules {
            let exports = names
                .exports
                .iter()
                .filter(|(m, ..)| m == path)
                .map(|(_, x, u, vis)| (x.clone(), (*u, *vis)))
                .collect();
            r.modules.push(ModuleState { package: i + 1, path: path.clone(), exports, ..ModuleState::default() });
        }
        r.take_names(names);
    }
    r.next_unique = r.next_unique.max(first_unique);
    let first = r.next_unique;
    let parts: Vec<(&[ast::LDecl], Option<&LTerm>)> = modules
        .iter()
        .enumerate()
        .map(|(i, m)| match (&m.program.decls[..], m.program.body.as_ref()) {
            ([], Some(Spanned { node: ast::Term::Let(decls, body), .. })) if i == 0 && part.root => (&decls[..], Some(&**body)),
            (decls, body) => (decls, body.filter(|_| i == 0 && part.root)),
        })
        .collect();
    // 1. every module's names, so that any module can mention any other's
    let mut bound = Vec::new();
    for (i, m) in modules.iter().enumerate() {
        r.enter(i);
        r.linear = r.linear_package || m.program.linear;
        let b = r.bind_program(m.program, parts[i].0);
        r.export(i, m.program, &b);
        r.leave();
        bound.push(b);
    }
    // 2. what their `use`s bring.  Twice, since a `@pub use` may give on
    // what another module's `use` brings it; only the second time reports.
    for quiet in [true, false] {
        let mark = r.errors.len();
        for (i, m) in modules.iter().enumerate() {
            r.enter(i);
            for u in &m.program.uses {
                r.rename_use(u);
            }
            r.leave();
        }
        if quiet {
            r.errors.truncate(mark);
        }
    }
    // 3. what each of them says
    let mut binds = Vec::new();
    let mut body = None;
    let mut main = None;
    for (i, (m, b)) in modules.iter().zip(bound).enumerate() {
        r.enter(i);
        r.linear = r.linear_package || m.program.linear;
        let (d, e, n, bs) = r.rename_program(m.program, b);
        datas.extend(d);
        effects.extend(e);
        natives.extend(n);
        binds.extend(bs);
        if i == 0 && part.root {
            body = parts[0].1.map(|b| r.rename_term(b));
            main = r.scopes.last().and_then(|own| own.get("main")).map(|&(u, _)| u);
        }
        r.leave();
    }
    let groups = [prelude_binds, binds].into_iter().filter(|bs| !bs.is_empty()).map(hir::BindGroup::Rec).collect();
    // what it gives other packages
    let own = |u: &Option<usize>| u.is_some_and(|u| u >= first);
    let names_given = hir::PackageNames {
        modules: modules.iter().map(|m| m.path.clone()).collect(),
        exports: r
            .modules
            .iter()
            .take(modules.len())
            .flat_map(|m| m.exports.iter().map(|(x, &(u, vis))| (m.path.clone(), x.clone(), u, vis)))
            .collect(),
        types: datas
            .iter()
            .filter(|d: &&hir::DataDecl| own(&d.name.unique))
            .filter_map(|d| Some((d.name.unique?, d.name.text.clone(), r.ctors.get(&d.name.unique?)?.clone())))
            .collect(),
        effect_ops: effects
            .iter()
            .filter(|e: &&hir::EffectDecl| own(&e.name.unique))
            .filter_map(|e| Some((e.name.unique?, r.effect_ops.get(&e.name.unique?)?.clone())))
            .collect(),
        fields: datas
            .iter()
            .filter(|d: &&hir::DataDecl| own(&d.name.unique))
            .filter_map(|d| Some((d.name.unique?, r.fields.get(&d.name.unique?)?.clone())))
            .collect(),
        defaults: datas
            .iter()
            .filter(|d: &&hir::DataDecl| own(&d.name.unique))
            .filter_map(|d| Some((d.name.unique?, r.defaults.get(&d.name.unique?)?.clone())))
            .collect(),
        supers: datas
            .iter()
            .filter(|d: &&hir::DataDecl| own(&d.name.unique))
            .filter_map(|d| Some((d.name.unique?, r.supers.get(&d.name.unique?)?.clone())))
            .collect(),
        methods: {
            let mut methods: Vec<(usize, (usize, usize, usize))> = r.methods.iter().filter(|(m, _)| **m >= first).map(|(m, i)| (*m, *i)).collect();
            methods.sort_unstable();
            methods
        },
    };
    let module = hir::Module {
        datas,
        data_after: Vec::new(),
        effects,
        natives,
        groups,
        body,
        prelude: names,
        main,
        qualifiers: r.qualifiers,
        package: package.to_string(),
        names: names_given,
        uniques: (first, r.next_unique),
        terminating: r.terminating,
        hints: r.hints,
        supers: r.supers,
        methods: r.methods,
        overloads: r.overloads,
        version: None,
        linear_body: linear || modules.first().is_some_and(|m| m.program.linear),
    };
    RenameResult { module, spans: r.spans, errors: r.errors }
}

/// Every module of a package, and each module written inside one of their
/// files, after them: `module Inner where` in `src/Outer.idl` is
/// `Outer.Inner`. One whose path a module has already is an error, and is
/// left out.
fn lifted<'a>(modules: &[ModuleSource<'a>], errors: &mut Vec<RenameError>) -> Vec<ModuleSource<'a>> {
    let mut all: Vec<ModuleSource<'a>> = modules.iter().map(|m| ModuleSource { path: m.path.clone(), program: m.program }).collect();
    let mut i = 0;
    while i < all.len() {
        let (path, program) = (all[i].path.clone(), all[i].program);
        for (name, inner) in &program.modules {
            let mut at = path.clone();
            at.push(name.node.clone());
            if all.iter().any(|m| m.path == at) {
                errors.push(RenameError::DuplicateModule { path: at.join("."), span: name.span });
                continue;
            }
            all.push(ModuleSource { path: at, program: inner });
        }
        i += 1;
    }
    all
}

/// Whether a module says how visible anything in it is.
fn mentions_visibility(p: &ast::Program) -> bool {
    !p.visibility.is_empty()
        || p.uses.iter().any(|u| u.node.public)
        || p.datas.iter().any(|d| d.node.visibility != Visibility::Private)
        || p.effects.iter().any(|e| e.node.visibility != Visibility::Private)
}

/// A pattern whose names have been bound: what is left to do is wrap the
/// lambda around the body.
struct BoundParam {
    span: Span,
    name: Ident,
    /// for an unboxing pattern `[x]`, the binder of the box itself
    boxed: Option<Ident>,
    grade: Option<hir::GradeAnn>,
    ty: Option<hir::Term>,
}

impl Renamer {
    /// The starting environment: a single global scope pre-populated with the
    /// primitive types and operations, holding the uniques they reserve.
    fn new(package: &str) -> Self {
        let builtins = Builtin::ALL.iter().map(|b| (b.name(), b.unique()));
        let prims = Prim::ALL.iter().flat_map(|p| [(p.name(), p.unique()), (p.alias(), p.unique())]);
        let global: Scope =
            builtins.chain(prims).map(|(n, u)| (n.to_string(), (u, Span::at(0)))).collect();
        Renamer {
            scopes: vec![global],
            next_unique: FIRST_USER_UNIQUE,
            spans: SpanTable::default(),
            errors: Vec::new(),
            ctors: HashMap::new(),
            type_names: HashMap::new(),
            twins: HashMap::new(),
            fields: HashMap::new(),
            terminating: Default::default(),
            hints: Default::default(),
            defaults: HashMap::new(),
            supers: HashMap::new(),
            methods: HashMap::new(),
            linear_package: false,
            linear: false,
            in_type: false,
            effect_ops: HashMap::new(),
            package: package.to_string(),
            modules: Vec::new(),
            deps: Vec::new(),
            current: None,
            base: Vec::new(),
            visibility_mentioned: false,
            qualifiers: HashMap::new(),
            overloads: HashMap::new(),
            meanings: None,
        }
    }

    /// Put module `i`'s names in scope: the prelude's, then what it uses,
    /// then its own.
    fn enter(&mut self, i: usize) {
        self.scopes = self.base.clone();
        self.scopes.push(std::mem::take(&mut self.modules[i].imports));
        self.scopes.push(std::mem::take(&mut self.modules[i].own));
        self.current = Some(i);
    }

    fn leave(&mut self) {
        let i = self.current.take().expect("a module was entered");
        self.modules[i].own = self.scopes.pop().expect("entered");
        self.modules[i].imports = self.scopes.pop().expect("entered");
    }

    /// The names of a program's data types, constructors, effects,
    /// operations and definitions (`decls`, which may be a `let`'s), all in
    /// the innermost scope.
    fn bind_program<'a>(&mut self, prog: &'a ast::Program, decls: &'a [ast::LDecl]) -> Bound<'a> {
        let effect_names: Vec<(Ident, Vec<Ident>)> = prog
            .effects
            .iter()
            .map(|e| {
                let name = self.bind_name(&e.node.name);
                let ops: Vec<Ident> = e.node.ops.iter().map(|op| self.bind_name(&op.name)).collect();
                if let Some(u) = name.unique {
                    self.effect_ops.insert(u, ops.iter().filter_map(|o| Some((o.text.clone(), o.unique?))).collect());
                }
                (name, ops)
            })
            .collect();
        let datas: Vec<&ast::LData> = prog.datas.iter().collect();
        let data_names = self.bind_datas(&datas);
        let natives = decls
            .iter()
            .filter_map(|d| match &d.node {
                ast::Decl::Native(name, _) => Some((self.bind_name(name), d)),
                _ => None,
            })
            .collect();
        let defs = self.bind_decls(decls);
        // a record's fields, by their projections, which the parser named
        // `Point.x` and are bound (under that name) with the definitions
        for (d, (name, _)) in datas.iter().zip(&data_names) {
            let (Some(fields), Some(u)) = (&d.node.fields, name.unique) else { continue };
            let found = fields
                .iter()
                .filter_map(|f| {
                    let key = format!("{}.{}", d.node.name.node, f.node);
                    let scope = self.scopes.last().expect("scope stack is never empty");
                    scope.get(&key).map(|&(p, _)| (f.node.clone(), p))
                })
                .collect();
            self.fields.insert(u, found);
            // a trait's methods, the definitions named as its fields are
            if d.node.is_trait {
                let scope = self.scopes.last().expect("scope stack is never empty");
                for (i, f) in fields.iter().enumerate() {
                    if let Some(&(m, _)) = scope.get(&f.node).filter(|_| !f.node.starts_with("#super")) {
                        self.methods.insert(m, (u, i, d.node.params.len()));
                    }
                }
            }
            // a trait's superclasses, the fields named `#super0` and on
            if d.node.is_trait {
                let supers: Vec<usize> =
                    self.fields.get(&u).map(|fs| fs.iter().filter(|(f, _)| f.starts_with("#super")).map(|(_, p)| *p).collect()).unwrap_or_default();
                if !supers.is_empty() {
                    self.supers.insert(u, supers);
                }
            }
            // a trait's defaults, bound (`Describe.#twice`) with the rest
            if !d.node.defaults.is_empty() {
                let scope = self.scopes.last().expect("scope stack is never empty");
                let found = d
                    .node
                    .defaults
                    .iter()
                    .filter_map(|(m, def, function)| scope.get(def).map(|&(p, _)| (m.node.clone(), p, *function)))
                    .collect();
                self.defaults.insert(u, found);
            }
        }
        for (_, name, _) in &defs {
            if let (Some(u), true) = (name.unique, prog.terminating.iter().any(|x| x.node == name.text)) {
                self.terminating.insert(u);
            }
            if let (Some(u), true) = (name.unique, prog.hints.iter().any(|x| x.node == name.text)) {
                self.hints.insert(u);
            }
        }
        Bound { datas, data_names, effect_names, defs, natives }
    }

    /// What a program says, its names bound and its `use`s taken: data
    /// types and effects first, then definitions.  An effect's name is bound
    /// before the data types are renamed, and a data type's before the
    /// effects are, so either can mention the other.
    fn rename_program(
        &mut self,
        prog: &ast::Program,
        b: Bound<'_>,
    ) -> (Vec<hir::DataDecl>, Vec<hir::EffectDecl>, Vec<hir::NativeDecl>, Vec<hir::Bind>) {
        let datas = self.rename_datas(&b.datas, b.data_names);
        let effects = prog.effects.iter().zip(b.effect_names).map(|(e, names)| self.rename_effect(e, names)).collect();
        let natives = b
            .natives
            .into_iter()
            .map(|(name, d)| {
                let ast::Decl::Native(_, ty) = &d.node else { unreachable!("only natives") };
                let id = self.fresh_node(d.span);
                hir::NativeDecl { id, name, ty: self.rename_sig(ty) }
            })
            .collect();
        let binds = self.rename_defs(b.defs);
        (datas, effects, natives, binds)
    }

    /// Record what module `i` declares, and how visible each is, for the
    /// others' `use`s.
    fn export(&mut self, i: usize, prog: &ast::Program, b: &Bound<'_>) {
        let declared: HashMap<&str, Visibility> = prog.visibility.iter().map(|(x, v)| (x.node.as_str(), *v)).collect();
        let mut exports: Vec<(Ident, Visibility)> = Vec::new();
        for (d, (name, _)) in b.datas.iter().zip(&b.data_names) {
            exports.push((name.clone(), d.node.visibility));
        }
        for (e, (name, ops)) in prog.effects.iter().zip(&b.effect_names) {
            exports.push((name.clone(), e.node.visibility));
            exports.extend(ops.iter().map(|op| (op.clone(), e.node.visibility)));
        }
        for name in b.defs.iter().map(|(_, name, _)| name).chain(b.natives.iter().map(|(name, _)| name)) {
            let vis = declared.get(name.text.as_str()).copied().unwrap_or_default();
            exports.push((name.clone(), vis));
        }
        let qualifier = self.modules[i].path.join(".");
        for (x, vis) in exports {
            let Some(u) = x.unique else { continue };
            if !qualifier.is_empty() {
                self.qualifiers.insert(u, qualifier.clone());
            }
            self.modules[i].exports.insert(x.text, (u, vis));
        }
    }

    /// The module at this path in a package, if it has one.
    fn module_at(&self, package: usize, path: &[&str]) -> Option<usize> {
        self.modules
            .iter()
            .position(|m| m.package == package && m.path.len() == path.len() && m.path.iter().zip(path).all(|(a, b)| a == b))
    }

    /// A module, as a `use` writes it: the package's name for the root, and
    /// another package's name in front of its modules.
    fn module_name(&self, m: usize) -> String {
        let module = &self.modules[m];
        let package = if module.package == 0 { &self.package } else { &self.deps[module.package - 1] };
        match (&module.path[..], module.package) {
            ([], _) => package.clone(),
            (path, 0) => path.join("."),
            (path, _) => format!("{package}.{}", path.join(".")),
        }
    }

    /// What is at this path under a package's root: a module, or failing
    /// that a directory with modules in it.
    fn under_root(&self, package: usize, path: &[&str]) -> Option<Target> {
        if let Some(m) = self.module_at(package, path) {
            return Some(Target::Module(m));
        }
        let inside =
            |m: &ModuleState| m.package == package && m.path.len() > path.len() && m.path.iter().zip(path).all(|(a, b)| a == b);
        self.modules.iter().any(inside).then(|| Target::Namespace(package, path.iter().map(|s| s.to_string()).collect()))
    }

    /// What another package, or an earlier part of this one, says of its
    /// types, effects, records and traits.
    fn take_names(&mut self, names: &hir::PackageNames) {
        for (u, ty, ctors) in &names.types {
            self.ctors.insert(*u, ctors.clone());
            self.type_names.insert(*u, ty.clone());
            self.note_twin(*u);
        }
        self.effect_ops.extend(names.effect_ops.iter().cloned());
        self.fields.extend(names.fields.iter().cloned());
        self.defaults.extend(names.defaults.iter().cloned());
        self.supers.extend(names.supers.iter().cloned());
        self.methods.extend(names.methods.iter().cloned());
    }

    /// May the current module see something of module `owner`'s, that is
    /// this visible?  Its own, and its descendants', always; its parent's
    /// subtree, with `@pub(super)`; the package, with `@pub(pkg)` or `@pub`.
    fn visible(&self, owner: usize, vis: Visibility) -> bool {
        // another package gives only what it makes `@pub`
        if self.modules[owner].package != 0 {
            return vis == Visibility::Public;
        }
        let Some(from) = self.current else { return true };
        if !self.visibility_mentioned {
            return true;
        }
        let (from, owner) = (&self.modules[from].path, &self.modules[owner].path);
        match vis {
            Visibility::Public | Visibility::Package => true,
            Visibility::Super => from.starts_with(&owner[..owner.len().saturating_sub(1)]),
            Visibility::Private => from.starts_with(owner),
        }
    }

    /// What a dotted path names: a module, or a name in one (or in scope),
    /// and then what lives under that, a type's constructors.  The first
    /// segment is a module's alias, a name in scope, the package's own name
    /// (its root module), or a module under the root; a later one is a
    /// module's child, its export, or a type's constructor.  Reports what
    /// it cannot find.
    fn resolve_path(&mut self, segments: &[&str], span: Span) -> Option<Target> {
        let first = segments[0];
        let alias = self.current.and_then(|c| self.modules[c].aliases.get(first).copied());
        let mut at = if let Some(m) = alias {
            Target::Module(m)
        } else if let Some(u) = self.lookup(first) {
            // the name another module of that name gives, as `use Matrix
            // (Matrix)` brought the type in (on the first of the two passes
            // over the `use`s): the path is that module's still. (In the
            // module itself, `use Matrix.*` is its own type's constructors.)
            match self.under_root(0, &[first]) {
                Some(Target::Module(m))
                    if Some(m) != self.current && self.modules[m].exports.get(first).map(|e| e.0) == Some(u) =>
                {
                    Target::Module(m)
                }
                _ => Target::Name(u),
            }
        } else if first == self.package && !self.modules.is_empty() {
            // the root module, or in a part without it, what is under it
            match self.module_at(0, &[]) {
                Some(m) => Target::Module(m),
                None => Target::Namespace(0, Vec::new()),
            }
        } else if let Some(t) = self.under_root(0, &[first]) {
            t
        } else if let Some(m) = self.deps.iter().position(|d| d == first).and_then(|i| self.module_at(i + 1, &[])) {
            Target::Module(m)
        } else {
            self.errors.push(RenameError::UnknownPath { path: first.to_string(), span });
            return None;
        };
        for (i, seg) in segments.iter().enumerate().skip(1) {
            let here = segments[..i].join(".");
            at = match at {
                Target::Namespace(package, path) => {
                    let mut child: Vec<&str> = path.iter().map(String::as_str).collect();
                    child.push(seg);
                    match self.under_root(package, &child) {
                        Some(t) => t,
                        None => {
                            self.errors.push(RenameError::NotInModule { module: here, name: seg.to_string(), span });
                            return None;
                        }
                    }
                }
                Target::Module(m) => {
                    // (named as it is, not as an alias has it)
                    let here = self.module_name(m);
                    let mut child: Vec<&str> = self.modules[m].path.iter().map(String::as_str).collect();
                    child.push(seg);
                    if let Some(t) = self.under_root(self.modules[m].package, &child) {
                        t
                    } else if let Some(&(u, vis)) = self.modules[m].exports.get(*seg) {
                        if !self.visible(m, vis) {
                            self.errors.push(RenameError::NotVisible { name: seg.to_string(), module: here, span });
                        }
                        Target::Name(u)
                    } else {
                        self.errors.push(RenameError::NotInModule { module: here, name: seg.to_string(), span });
                        return None;
                    }
                }
                Target::Name(u) => {
                    let Some(ctors) = self.ctors.get(&u) else {
                        self.errors.push(RenameError::NotAType { path: here, span });
                        return None;
                    };
                    let field = self.fields.get(&u).and_then(|fs| fs.iter().find(|(f, _)| f == seg)).map(|&(_, p)| p);
                    match ctors.iter().find(|(c, _)| c == seg) {
                        Some(&(_, c)) => Target::Name(c),
                        // a record's field, `Point.x`: its projection
                        None if field.is_some() => Target::Name(field.expect("checked")),
                        None => {
                            let error = RenameError::NoSuchConstructor { ty: here, name: seg.to_string(), span };
                            self.errors.push(error);
                            return None;
                        }
                    }
                }
            };
        }
        Some(at)
    }

    fn fresh_unique(&mut self) -> usize {
        let u = self.next_unique;
        self.next_unique += 1;
        u
    }

    /// Allocate a node id and record its span in the side table.
    fn fresh_node(&mut self, span: Span) -> NodeId {
        self.spans.push(span)
    }

    fn node(&mut self, span: Span, kind: hir::TermKind) -> hir::Term {
        hir::Term { id: self.fresh_node(span), kind }
    }

    /// Run `f` in a fresh innermost scope.
    fn scoped<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.push(Scope::new());
        let x = f(self);
        self.scopes.pop();
        x
    }

    /// Bind `name` in the innermost scope, returning its fresh `Ident`.
    /// Records a duplicate-binding error if the name is already bound in that
    /// same scope.
    fn bind_name(&mut self, name: &ast::Ident) -> Ident {
        let u = self.fresh_unique();
        // `_` binds a slot nothing can refer to
        if name.node == "_" {
            return Ident { unique: Some(u), text: name.node.clone() };
        }
        let scope = self.scopes.last_mut().expect("scope stack is never empty");
        if let Some(&(_, first)) = scope.get(&name.node) {
            self.errors.push(RenameError::Duplicate {
                name: name.node.clone(),
                first,
                again: name.span,
            });
        }
        scope.insert(name.node.clone(), (u, name.span));
        Ident { unique: Some(u), text: name.node.clone() }
    }

    /// Resolve a use of `name`.  On failure, log an error and return an
    /// `Ident` with no unique so downstream passes still get a well-formed
    /// tree.
    fn lookup(&self, name: &str) -> Option<usize> {
        self.scopes.iter().rev().find_map(|s| s.get(name)).map(|&(u, _)| u)
    }

    /// What a name may mean where it is written, if more than one thing: a
    /// module's own top-level names and what its `use`s bring are one
    /// layer, where a clash is an overload, chosen by type. A local name
    /// shadows them, and they shadow the prelude's.
    fn meanings(&self, name: &str) -> Option<Vec<usize>> {
        let c = self.current?;
        // the module's two scopes, just above the prelude's
        let (imports, own) = (self.base.len(), self.base.len() + 1);
        let innermost = self.scopes.iter().rposition(|s| s.contains_key(name))?;
        if innermost != imports && innermost != own {
            return None;
        }
        let mut all: Vec<usize> = Vec::new();
        let mut add = |u: usize| {
            let u = self.as_written(u);
            if !all.contains(&u) {
                all.push(u);
            }
        };
        if let Some(&(u, _)) = self.scopes.get(own).and_then(|s| s.get(name)) {
            add(u);
        }
        match self.modules[c].brought.get(name) {
            Some(us) => us.iter().for_each(|&u| add(u)),
            None => {
                if let Some(&(u, _)) = self.scopes.get(imports).and_then(|s| s.get(name)) {
                    add(u);
                }
            }
        }
        (all.len() > 1).then_some(all)
    }

    /// The meanings [`Renamer::use_name`] left, for the node at `id`.
    fn overloaded_at(&mut self, id: NodeId) {
        if let Some(us) = self.meanings.take() {
            self.overloads.insert(id, us);
        }
    }

    fn use_name(&mut self, name: &ast::Ident) -> Ident {
        self.meanings = None;
        if name.node == ast::TUPLE {
            return self.tuple(name.span);
        }
        // what a string's holes are rendered and joined with: the prelude's
        // `display` and `debug`, and the primitive `++`, whatever else is in
        // scope by those names
        match name.node.as_str() {
            "#display" => return self.prelude_name("display", name.span),
            "#debug" => return self.prelude_name("debug", name.span),
            // and what derived instances compare with
            "#eq" => return self.prelude_name("==", name.span),
            "#lt" => return self.prelude_name("<", name.span),
            "#append" => return Ident { unique: Some(idyllic_builtins::Prim::StrAppend.unique()), text: "++".into() },
            _ => {}
        }
        if name.node.contains('.') && name.node.len() > 1 {
            return self.use_qualified(name);
        }
        let found = self.lookup(&name.node);
        if found.is_none() {
            // a constructor, written as if it were in scope
            let types: Vec<String> = self.constructor_types(&name.node);
            let span = name.span;
            let error = if types.is_empty() {
                RenameError::Unbound { name: name.node.clone(), span }
            } else {
                RenameError::BareConstructor { name: name.node.clone(), types, span }
            };
            self.errors.push(error);
        }
        self.meanings = self.meanings(&name.node);
        // the node holds the first meaning, the module's own if it has one
        let first = self.meanings.as_ref().and_then(|us| us.first().copied());
        Ident { unique: first.or(found.map(|u| self.as_written(u))), text: name.node.clone() }
    }

    /// An entry of a row, `Div` or `State Int`: its head names an effect, so
    /// a constructor of that name is passed over for what is further out,
    /// as a constructor `Div` brought in with `use Op.*` would otherwise
    /// hide the effect.
    fn rename_row_entry(&mut self, t: &LTerm) -> hir::Term {
        let sp = t.span;
        match &t.node {
            ast::Term::Var(x) if !x.node.contains('.') && x.node != ast::TUPLE => {
                let found = self.scopes.iter().rev().filter_map(|s| s.get(&x.node)).map(|&(u, _)| u).find(|u| !self.is_constructor(*u));
                match found {
                    Some(u) => {
                        let i = Ident { unique: Some(self.as_written(u)), text: x.node.clone() };
                        self.node(sp, hir::TermKind::Var(i))
                    }
                    None => self.rename_term(t),
                }
            }
            ast::Term::App(f, a) => {
                let f = self.rename_row_entry(f);
                let a = self.rename_term(a);
                self.node(sp, hir::TermKind::App(Box::new(f), Box::new(a)))
            }
            _ => self.rename_term(t),
        }
    }

    /// Whether `u` is a data type's constructor.
    fn is_constructor(&self, u: usize) -> bool {
        self.ctors.values().any(|cs| cs.iter().any(|&(_, c)| c == u))
    }

    /// The prelude's `Pair`, which a tuple is made of: the type in a type,
    /// the constructor elsewhere. (In the prelude itself, its own.)
    fn tuple(&mut self, span: Span) -> Ident {
        let found = self.base.iter().rev().find_map(|s| s.get("Pair")).map(|&(u, _)| u).or_else(|| self.lookup("Pair"));
        if found.is_none() {
            self.errors.push(RenameError::Unbound { name: "Pair".into(), span });
        }
        Ident { unique: found.map(|u| self.as_written(u)), text: "Pair".into() }
    }

    /// A name of the prelude's, as it is there: from the prelude's scopes,
    /// or, in the prelude itself, its own.
    fn prelude_name(&mut self, text: &str, span: Span) -> Ident {
        let found = self.base.iter().rev().find_map(|s| s.get(text)).map(|&(u, _)| u).or_else(|| self.lookup(text));
        if found.is_none() {
            self.errors.push(RenameError::Unbound { name: text.into(), span });
        }
        Ident { unique: found, text: text.into() }
    }

    /// Remember a type whose constructor has its name.
    fn note_twin(&mut self, ty: usize) {
        let name = &self.type_names[&ty];
        if let Some(&(_, c)) = self.ctors[&ty].iter().find(|(c, _)| c == name) {
            self.twins.insert(ty, c);
        }
    }

    /// What a name that found `u` stands for here: a type and the
    /// constructor that shares its name are one name, the type in a type
    /// and the constructor elsewhere.
    fn as_written(&self, u: usize) -> usize {
        let pair = match self.twins.get(&u) {
            Some(&c) => Some((u, c)),
            None => self.twins.iter().find(|(_, &c)| c == u).map(|(&t, &c)| (t, c)),
        };
        match pair {
            Some((t, _)) if self.in_type => t,
            Some((_, c)) => c,
            None => u,
        }
    }

    /// Rename what `f` renames as a type.
    fn as_type<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let outer = std::mem::replace(&mut self.in_type, true);
        let x = f(self);
        self.in_type = outer;
        x
    }

    /// ... and as a term: a pattern, say, which matches constructors.
    fn as_term<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let outer = std::mem::replace(&mut self.in_type, false);
        let x = f(self);
        self.in_type = outer;
        x
    }

    /// The types in scope with a constructor of this name, by name.
    fn constructor_types(&self, name: &str) -> Vec<String> {
        let mut types: Vec<String> = self
            .ctors
            .iter()
            .filter(|(_, cs)| cs.iter().any(|(c, _)| c == name))
            // a type in scope under its name: that name, and not another
            // type of the same name (a dependency's), is how to write it
            .filter_map(|(d, _)| self.type_names.get(d).filter(|t| self.lookup(t) == Some(*d)).cloned())
            .collect();
        types.sort();
        types
    }

    /// `Nat.Z`, `M.double`, `App.Syntax.Expr.Int`: a name, from wherever
    /// its path says.
    fn use_qualified(&mut self, name: &ast::Ident) -> Ident {
        let segments: Vec<&str> = name.node.split('.').collect();
        let last = segments.last().expect("split gives one").to_string();
        match self.resolve_path(&segments, name.span) {
            // a path says which of two twins it means: `Pair.Pair`, through
            // the type, is the constructor, and `Std.Tuple.Pair`, the
            // module's, is the type, wherever either is written
            Some(Target::Name(u)) => Ident { unique: Some(u), text: last },
            Some(Target::Module(_) | Target::Namespace(..)) => {
                self.errors.push(RenameError::ModuleNotValue { path: name.node.clone(), span: name.span });
                Ident { unique: None, text: last }
            }
            None => Ident { unique: None, text: last },
        }
    }

    /// Bring a name into the current module's scope from elsewhere, and,
    /// with `@pub use`, give it on to whoever uses this module.
    fn import(&mut self, name: String, unique: usize, span: Span, public: bool) {
        // naming an effect brings its operations
        let ops = self.effect_ops.get(&unique).cloned().unwrap_or_default();
        let imports = self.scopes.len() - 2;
        for (x, u) in std::iter::once((name, unique)).chain(ops) {
            if let Some(c) = self.current {
                if public {
                    self.modules[c].exports.insert(x.clone(), (u, Visibility::Public));
                }
                let brought = self.modules[c].brought.entry(x.clone()).or_default();
                if !brought.contains(&u) {
                    brought.push(u);
                }
            }
            self.scopes[imports].insert(x, (u, span));
        }
    }

    /// `use M`, `use M as C`, `use M (a, b)`: another module's names; `use
    /// T.*` and `use T (C, D)`: a type's constructors, bare.  Into the scope
    /// under the module's own.
    fn rename_use(&mut self, u: &Spanned<ast::Use>) {
        let segments: Vec<&str> = u.node.path.iter().map(|s| s.node.as_str()).collect();
        let path = segments.join(".");
        let at = u.span;
        let public = u.node.public;
        let Some(target) = self.resolve_path(&segments, at) else { return };
        let listed = |names: &Option<Vec<ast::Ident>>| -> Vec<(String, Span)> {
            names.iter().flatten().map(|n| (n.node.clone(), n.span)).collect()
        };
        match target {
            Target::Namespace(..) => {
                self.errors.push(RenameError::NotInModule { module: path, name: "module of its own".into(), span: at });
            }
            Target::Module(m) => {
                if let (Some(alias), Some(c)) = (&u.node.alias, self.current) {
                    self.modules[c].aliases.insert(alias.node.clone(), m);
                }
                // everything it gives, unless it has an alias or a list
                let wanted: Vec<(String, Span)> = if u.node.names.is_some() {
                    listed(&u.node.names)
                } else if u.node.alias.is_none() || u.node.glob {
                    let mut all: Vec<(String, usize, Visibility)> =
                        self.modules[m].exports.iter().map(|(x, &(v, vis))| (x.clone(), v, vis)).collect();
                    all.sort();
                    all.into_iter().filter(|(_, _, vis)| self.visible(m, *vis)).map(|(x, _, _)| (x, at)).collect()
                } else {
                    Vec::new()
                };
                for (name, span) in wanted {
                    match self.modules[m].exports.get(&name).copied() {
                        Some((v, vis)) => {
                            if !self.visible(m, vis) {
                                self.errors.push(RenameError::NotVisible { name: name.clone(), module: path.clone(), span });
                            }
                            self.import(name, v, span, public);
                        }
                        None => self.errors.push(RenameError::NotInModule { module: path.clone(), name, span }),
                    }
                }
            }
            Target::Name(t) => {
                let Some(ctors) = self.ctors.get(&t).cloned() else {
                    // `use M.x`: the one name
                    if u.node.glob || u.node.names.is_some() {
                        self.errors.push(RenameError::NotAType { path, span: at });
                    } else {
                        let last = segments.last().expect("a path has a segment").to_string();
                        self.import(last, t, at, public);
                    }
                    return;
                };
                if u.node.alias.is_some() {
                    self.errors.push(RenameError::AliasForType { path, span: at });
                    return;
                }
                let wanted: Vec<(String, Span)> = match &u.node.names {
                    _ if u.node.glob => ctors.iter().map(|(c, _)| (c.clone(), at)).collect(),
                    Some(_) => listed(&u.node.names),
                    // the type on its own: `use M.T` brings just the type
                    None => {
                        if segments.len() > 1 {
                            let last = segments.last().expect("a path has a segment").to_string();
                            self.import(last, t, at, public);
                        }
                        Vec::new()
                    }
                };
                for (name, span) in wanted {
                    match ctors.iter().find(|(c, _)| *c == name) {
                        Some(&(_, c)) => self.import(name, c, span, public),
                        None => self.errors.push(RenameError::NoSuchConstructor { ty: path.clone(), name, span }),
                    }
                }
            }
        }
    }

    /// The record type a name written as a record's names, and its fields,
    /// in order: `None` (and an error) if it is no record.
    fn record_named(&mut self, name: &ast::Ident, report: bool) -> Option<Vec<String>> {
        let ty = self.as_type(|r| r.use_name(name));
        let fields = ty.unique.and_then(|u| self.fields.get(&u)).map(|fs| fs.iter().map(|(f, _)| f.clone()).collect());
        if fields.is_none() && report && ty.unique.is_some() {
            self.errors.push(RenameError::Record {
                message: format!("{} is not a record", name.node),
                label: "fields, `{ x = .. }`, are for a type declared with `record`".into(),
                span: name.span,
            });
        }
        fields
    }

    /// The values (or patterns) given for a record's fields, in the order
    /// the fields are declared: each field's, if it was given. Reports a
    /// field given that the record has not, or one given twice.
    fn in_field_order<'v, T>(&mut self, record: &str, fields: &[String], given: &'v [(ast::Ident, T)], report: bool) -> Vec<Option<&'v T>> {
        if report {
            for (i, (f, _)) in given.iter().enumerate() {
                if !fields.contains(&f.node) {
                    self.errors.push(RenameError::Record {
                        message: format!("{record} has no field {}", f.node),
                        label: format!("its fields are {}", fields.join(", ")),
                        span: f.span,
                    });
                } else if given[..i].iter().any(|(g, _)| g.node == f.node) {
                    self.errors.push(RenameError::Record {
                        message: format!("the field {} is given twice", f.node),
                        label: "given again here".into(),
                        span: f.span,
                    });
                }
            }
        }
        fields.iter().map(|f| given.iter().find(|(g, _)| g.node == *f).map(|(_, v)| v)).collect()
    }

    /// `Point { x = 1, y = 2 }`: the constructor, given every field, in the
    /// order they are declared (and so computed in that order).
    /// The same, with the type's arguments given first, by hand.
    fn record_of(&mut self, name: &ast::Ident, args: &[LTerm], values: &[(ast::Ident, LTerm)], instance: &ast::Ident, span: Span) -> Option<LTerm> {
        let built = self.record_built(name, values, Some(instance), span)?;
        // `C f g`: the arguments go between `C` and the fields
        fn under(t: LTerm, args: &[LTerm], span: Span) -> LTerm {
            match t.node {
                ast::Term::App(f, a) => Spanned::new(ast::Term::App(Box::new(under(*f, args, span)), a), t.span),
                ctor => args
                    .iter()
                    .fold(Spanned::new(ctor, t.span), |f, a| Spanned::new(ast::Term::ImplicitApp(Box::new(f), Box::new(a.clone())), span)),
            }
        }
        Some(under(built, args, span))
    }

    fn record_literal(&mut self, name: &ast::Ident, values: &[(ast::Ident, LTerm)], span: Span) -> Option<LTerm> {
        self.record_built(name, values, None, span)
    }

    /// The same, as the body of `instance`, if it is one: a trait's default
    /// for a method left out is given it, `Describe.#twice @{instance}`.
    fn record_built(&mut self, name: &ast::Ident, values: &[(ast::Ident, LTerm)], instance: Option<&ast::Ident>, span: Span) -> Option<LTerm> {
        let fields = self.record_named(name, true)?;
        let ordered = self.in_field_order(&name.node, &fields, values, true);
        // a trait's method left out is its default, if it has one
        let defaults = self.as_type(|r| r.use_name(name)).unique.and_then(|u| self.defaults.get(&u)).cloned().unwrap_or_default();
        let filled: Vec<Option<LTerm>> = fields
            .iter()
            .zip(&ordered)
            .map(|(f, v)| match v {
                Some(v) => Some((*v).clone()),
                // a superclass is what a search for it finds
                None if f.starts_with("#super") && instance.is_some() => Some(Spanned::new(ast::Term::Search, span)),
                None => defaults.iter().find(|(m, ..)| m == f).map(|&(_, u, function)| {
                    let default = Spanned::new(ast::Term::Resolved(u, format!("{}.#{f}", name.node)), span);
                    // given the instance it fills, not one searched for
                    let default = match instance {
                        Some(me) => {
                            let me = Spanned::new(ast::Term::Var(me.clone()), span);
                            Spanned::new(ast::Term::AutoApp(Box::new(default), Box::new(me)), span)
                        }
                        None => default,
                    };
                    if !function {
                        return default;
                    }
                    // `\x -> default x`: under a lambda, so that the instance
                    // it is given (the one being built) is not needed to
                    // make itself, as Meadow has it
                    let x = Spanned::new("#d".to_string(), span);
                    let var = Spanned::new(ast::Term::Var(x.clone()), span);
                    let call = Spanned::new(ast::Term::App(Box::new(default), Box::new(var)), span);
                    let param = Spanned::new(ast::Param { name: x, unbox: false, grade: None, ty: None }, span);
                    Spanned::new(ast::Term::Lam(Box::new(param), Box::new(call)), span)
                }),
            })
            .collect();
        let ordered: Vec<Option<&LTerm>> = filled.iter().map(Option::as_ref).collect();
        let missing: Vec<&String> = fields.iter().zip(&ordered).filter(|(_, v)| v.is_none()).map(|(f, _)| f).collect();
        if !missing.is_empty() {
            let missing: Vec<&str> = missing.iter().map(|f| f.as_str()).collect();
            self.errors.push(RenameError::Record {
                message: format!("{} is missing {}", name.node, missing.join(", ")),
                label: "a record is built with every one of its fields".into(),
                span,
            });
            return None;
        }
        let ctor = Spanned::new(ast::Term::Var(name.clone()), name.span);
        Some(ordered.into_iter().flatten().fold(ctor, |f, v| {
            Spanned::new(ast::Term::App(Box::new(f), Box::new(v.clone())), span)
        }))
    }

    /// `Point { x = p }`: the constructor's pattern, `_` for a field not
    /// named.
    fn record_pattern(&mut self, name: &ast::Ident, given: &[(ast::Ident, ast::LPat)], span: Span, report: bool) -> Option<ast::LPat> {
        let fields = self.record_named(name, report)?;
        let ordered = self.in_field_order(&name.node, &fields, given, report);
        let pats = ordered.into_iter().map(|p| p.cloned().unwrap_or_else(|| Spanned::new(ast::Pat::Wild, span))).collect();
        Some(Spanned::new(ast::Pat::Con(name.clone(), pats), span))
    }

    /// The records in scope, by their type's name as it is written here, that
    /// have every one of `wanted`'s fields.
    fn records_with(&self, wanted: &[&str]) -> Vec<(String, Vec<String>)> {
        let mut found: Vec<(String, Vec<String>)> = self
            .fields
            .iter()
            .filter(|(_, fs)| wanted.iter().all(|w| fs.iter().any(|(f, _)| f == w)))
            .filter_map(|(t, fs)| {
                let name = self.type_names.get(t).filter(|n| self.lookup(n) == Some(*t))?;
                Some((name.clone(), fs.iter().map(|(f, _)| f.clone()).collect()))
            })
            .collect();
        found.sort();
        found
    }

    /// The one record in scope with these fields, or the error of there
    /// being none, or several.
    fn the_record_with(&mut self, wanted: &[&str], span: Span, what: &str) -> Option<(String, Vec<String>)> {
        let mut found = self.records_with(wanted);
        match found.len() {
            1 => found.pop(),
            0 => {
                self.errors.push(RenameError::Record {
                    message: format!("no record in scope has the field{} {}", if wanted.len() == 1 { "" } else { "s" }, wanted.join(", ")),
                    label: format!("{what} a record's fields, which this does not name"),
                    span,
                });
                None
            }
            _ => {
                let names: Vec<String> = found.into_iter().map(|(n, _)| n).collect();
                self.errors.push(RenameError::Record {
                    message: format!("{} records have {}: {}", names.len(), wanted.join(", "), names.join(", ")),
                    label: format!("say which: write {}.{} e", names[0], wanted[0]),
                    span,
                });
                None
            }
        }
    }

    /// `e.x`: the projection of the one record in scope with a field `x`,
    /// applied to `e`.
    fn projection(&mut self, e: &LTerm, field: &ast::Ident, span: Span) -> Option<LTerm> {
        let (record, _) = self.the_record_with(&[field.node.as_str()], field.span, "reads")?;
        let projection = Spanned::new(ast::Term::Var(Spanned::new(format!("{record}.{}", field.node), field.span)), field.span);
        Some(Spanned::new(ast::Term::App(Box::new(projection), Box::new(e.clone())), span))
    }

    /// `{ e | x = v }`: `e` taken apart and built again, with `v` for `x`.
    fn record_update(&mut self, e: &LTerm, values: &[(ast::Ident, LTerm)], span: Span) -> Option<LTerm> {
        let wanted: Vec<&str> = values.iter().map(|(f, _)| f.node.as_str()).collect();
        let (record, fields) = self.the_record_with(&wanted, span, "updates")?;
        let ordered = self.in_field_order(&record, &fields, values, true);
        let at = |t: ast::Term| Spanned::new(t, span);
        let name = Spanned::new(record.clone(), span);
        // what is kept is bound under a name nothing else can have
        let kept = |f: &str| format!("#{f}");
        let pats = fields
            .iter()
            .zip(&ordered)
            .map(|(f, v)| match v {
                Some(_) => Spanned::new(ast::Pat::Wild, span),
                None => Spanned::new(ast::Pat::Bind(ast::Param { name: Spanned::new(kept(f), span), unbox: false, grade: None, ty: None }), span),
            })
            .collect();
        let rebuilt = fields.iter().zip(&ordered).fold(at(ast::Term::Var(name.clone())), |acc, (f, v)| {
            let value = match v {
                Some(v) => (*v).clone(),
                None => at(ast::Term::Var(Spanned::new(kept(f), span))),
            };
            at(ast::Term::App(Box::new(acc), Box::new(value)))
        });
        let alt = ast::Alt { pat: Spanned::new(ast::Pat::Con(name, pats), span), rhs: ast::Rhs::Plain(rebuilt) };
        Some(at(ast::Term::Case(Box::new(e.clone()), vec![alt])))
    }

    /// `[1]`, written where nothing was.
    fn grade_one(&mut self, span: Span) -> hir::GradeAnn {
        let one = self.node(span, hir::TermKind::Lit(ast::Lit::Int(1)));
        hir::GradeAnn { subject: Box::new(one), in_type: None }
    }

    fn rename_grades(&mut self, g: &ast::GradeAnn) -> hir::GradeAnn {
        hir::GradeAnn {
            subject: Box::new(self.rename_term(&g.subject)),
            in_type: g.in_type.as_ref().map(|r| Box::new(self.rename_term(r))),
        }
    }

    fn rename_term(&mut self, t: &LTerm) -> hir::Term {
        use ast::Term as A;
        use hir::TermKind as H;
        let sp = t.span;
        match &t.node {
            A::Type => self.node(sp, H::Type),
            A::Hole => self.node(sp, H::Hole),
            A::Inf => self.node(sp, H::Inf),
            A::Lit(l) => self.node(sp, H::Lit(l.clone())),
            // `otherwise` is `True`, unless it has been defined to be
            // something else
            A::Var(x) if x.node == "otherwise" && self.lookup(&x.node).is_none() => {
                self.node(sp, H::Lit(ast::Lit::Bool(true)))
            }
            A::Var(x) => {
                let i = self.use_name(x);
                let node = self.node(sp, H::Var(i));
                self.overloaded_at(node.id);
                node
            }
            A::App(f, a) => {
                let f = self.rename_term(f);
                let a = self.rename_term(a);
                self.node(sp, H::App(Box::new(f), Box::new(a)))
            }
            A::Resolved(u, text) => self.node(sp, H::Var(Ident { unique: Some(*u), text: text.clone() })),
            A::Search => self.node(sp, H::Search),
            // equations: each a clause of a match, as a definition's are
            A::Equations(eqs) => {
                let clauses = eqs.iter().map(|(pats, rhs)| self.rename_clause(sp, pats, None, rhs)).collect();
                self.node(sp, H::Match(clauses))
            }
            A::AutoApp(f, a) => {
                let f = self.rename_term(f);
                let a = self.rename_term(a);
                self.node(sp, H::AutoApp(Box::new(f), Box::new(a)))
            }
            // what is given is most often a type, and read as one
            A::ImplicitApp(f, a) => {
                let f = self.rename_term(f);
                let a = self.as_type(|r| r.rename_term(a));
                self.node(sp, H::ImplicitApp(Box::new(f), Box::new(a)))
            }
            A::Ann(e, a) => {
                let e = self.rename_term(e);
                let a = self.as_type(|r| r.rename_term(a));
                self.node(sp, H::Ann(Box::new(e), Box::new(a)))
            }
            A::Range(lo, hi) => {
                let lo = self.rename_term(lo);
                let hi = self.rename_term(hi);
                self.node(sp, H::Range(Box::new(lo), Box::new(hi)))
            }
            A::Eff(t, row) => {
                let t = self.rename_term(t);
                let row = match &row.node {
                    A::Row(..) => self.rename_term(row),
                    _ => self.rename_row_entry(row),
                };
                self.node(sp, H::Eff(Box::new(t), Box::new(row)))
            }
            A::Row(labels, tail) => {
                let labels = labels.iter().map(|l| self.rename_row_entry(l)).collect();
                let tail = tail.as_ref().map(|t| Box::new(self.rename_term(t)));
                self.node(sp, H::Row(labels, tail))
            }
            A::Handle(body, clauses) => {
                let body = self.rename_term(body);
                let clauses = clauses
                    .iter()
                    .map(|c| {
                        // `return` is not a name
                        let op = (c.op.node != "return").then(|| self.use_name(&c.op));
                        let id = self.fresh_node(c.op.span);
                        self.scoped(|r| {
                            let params = c.params.iter().map(|x| r.bind_name(x)).collect();
                            hir::HandleClause { id, op, params, body: r.rename_term(&c.body) }
                        })
                    })
                    .collect();
                self.node(sp, H::Handle(Box::new(body), clauses))
            }
            A::Pi(mb, icit, grade, dom, cod) => {
                // neither the domain nor the grades can see the bound name
                let grade = match grade {
                    Some(g) => Some(self.rename_grades(g)),
                    // in a linear module, an unannotated binder is `[1]`
                    None if self.linear && *icit == Icit::Explicit => Some(self.grade_one(sp)),
                    None => None,
                };
                let dom = self.as_type(|r| r.rename_term(dom));
                self.scoped(|r| {
                    let mb = mb.as_ref().map(|x| r.bind_name(x));
                    let cod = r.as_type(|r| r.rename_term(cod));
                    r.node(sp, H::Pi(mb, *icit, grade, Box::new(dom), Box::new(cod)))
                })
            }
            A::Lam(param, body) => self.scoped(|r| {
                let p = r.bind_param(param);
                let body = r.rename_term(body);
                r.wrap_param(sp, p, body)
            }),
            A::BoxIntro(e) => {
                let e = self.rename_term(e);
                self.node(sp, H::BoxIntro(Box::new(e)))
            }
            A::LetBox(x, e, body) => {
                // not recursive: the boxed term cannot see the name it binds
                let e = self.rename_term(e);
                self.scoped(|r| {
                    let x = r.bind_name(x);
                    let body = r.rename_term(body);
                    r.node(sp, H::LetBox(x, Box::new(e), Box::new(body)))
                })
            }
            A::Let(decls, body) => self.scoped(|r| {
                let bs = r.rename_decls(decls);
                let body = r.rename_term(body);
                r.node(sp, H::Let(vec![hir::BindGroup::Rec(bs)], Box::new(body)))
            }),
            // `#[a, b]` is `a` in front of `#[b]`, which is `b` in front of
            // `#[]`
            A::ArrayLit(items) => {
                let items = items.iter().map(|e| self.rename_term(e)).collect();
                self.node(sp, H::ArrayLit(items))
            }
            A::RecordLit(name, values) => match self.record_literal(name, values, sp) {
                Some(built) => self.rename_term(&built),
                // (reported; nothing to say about it after)
                None => self.node(sp, H::Var(Ident { unique: None, text: "_".into() })),
            },
            A::RecordOf(name, args, values, instance) => match self.record_of(name, args, values, instance, sp) {
                Some(built) => self.rename_term(&built),
                None => self.node(sp, H::Var(Ident { unique: None, text: "_".into() })),
            },
            A::Proj(e, field) => match self.projection(e, field, sp) {
                Some(read) => self.rename_term(&read),
                // (reported; nothing to say about it after)
                None => self.node(sp, H::Var(Ident { unique: None, text: "_".into() })),
            },
            A::Update(e, values) => match self.record_update(e, values, sp) {
                Some(rebuilt) => self.rename_term(&rebuilt),
                // (reported; nothing to say about it after)
                None => self.node(sp, H::Var(Ident { unique: None, text: "_".into() })),
            },
            A::Seq(items) => {
                let items: Vec<(Span, hir::Term)> = items.iter().map(|e| (e.span, self.rename_term(e))).collect();
                let nil = self.node(sp, H::SeqNil);
                items.into_iter().rev().fold(nil, |tail, (at, head)| {
                    self.node(Span::new(at.start, sp.end), H::SeqCons(Box::new(head), Box::new(tail)))
                })
            }
            A::Case(scrutinee, alts) => {
                let scrutinee = self.rename_term(scrutinee);
                let clauses = alts
                    .iter()
                    .map(|alt| {
                        let span = alt.pat.span.merge(rhs_span(&alt.rhs).unwrap_or(alt.pat.span));
                        self.rename_clause(span, std::slice::from_ref(&alt.pat), None, &alt.rhs)
                    })
                    .collect();
                self.node(sp, H::Case(Box::new(scrutinee), clauses))
            }
        }
    }

    /// A signature.  The lower-case names it mentions that nothing binds are
    /// bound in front of it, implicitly, in order of appearance, their types
    /// left to the checker: `Vec n a -> a` is `{n : _} {a : _} -> Vec n a -> a`.
    fn rename_sig(&mut self, ty: &LTerm) -> hir::Term {
        let mut free = Vec::new();
        free_variables(ty, &mut Vec::new(), &|x| self.lookup(x).is_some(), &mut free);
        self.scoped(|r| {
            let bound: Vec<(Ident, Span)> = free.iter().map(|x| (r.bind_name(x), x.span)).collect();
            let body = r.as_type(|r| r.rename_term(ty));
            bound.into_iter().rev().fold(body, |acc, (x, span)| {
                let dom = r.node(span, hir::TermKind::Omitted);
                r.node(ty.span, hir::TermKind::Pi(Some(x), Icit::Implicit, None, Box::new(dom), Box::new(acc)))
            })
        })
    }

    /// One equation or alternative: its patterns bind names for the rest of
    /// it (all of them before any `.e` is renamed, so a dot pattern can
    /// mention a name bound to its right), then the right-hand side, under an
    /// annotation if one was written.
    fn rename_clause(
        &mut self,
        span: Span,
        pats: &[ast::LPat],
        result_ty: Option<&LTerm>,
        rhs: &ast::Rhs,
    ) -> hir::Clause {
        let id = self.fresh_node(span);
        self.scoped(|r| {
            let mut names = std::collections::VecDeque::new();
            for p in pats {
                r.bind_pattern(p, &mut names);
            }
            let pats = pats.iter().map(|p| r.rename_pattern(p, &mut names)).collect();
            let annotate = |r: &mut Renamer, e: &LTerm| {
                let body = r.rename_term(e);
                match result_ty {
                    None => body,
                    Some(ty) => {
                        let span = e.span.merge(ty.span);
                        let ty = r.as_type(|r| r.rename_term(ty));
                        r.node(span, hir::TermKind::Ann(Box::new(body), Box::new(ty)))
                    }
                }
            };
            let rhs = match rhs {
                ast::Rhs::Plain(e) => hir::Rhs::Plain(annotate(r, e)),
                ast::Rhs::Guarded(gs) => {
                    hir::Rhs::Guarded(gs.iter().map(|(g, e)| (r.rename_term(g), annotate(r, e))).collect())
                }
                ast::Rhs::Impossible => hir::Rhs::Impossible,
            };
            hir::Clause { id, pats, rhs }
        })
    }

    /// Bind the names in a pattern, in order.
    fn bind_pattern(&mut self, p: &ast::LPat, names: &mut std::collections::VecDeque<Ident>) {
        match &p.node {
            ast::Pat::Bind(param) => names.push_back(self.bind_name(&param.name)),
            ast::Pat::As(x, q) => {
                names.push_back(self.bind_name(x));
                self.bind_pattern(q, names);
            }
            ast::Pat::Con(_, ps) | ast::Pat::Seq(ps) => ps.iter().for_each(|q| self.bind_pattern(q, names)),
            // in the order the fields are declared, which is the order
            // `rename_pattern` takes the names in
            ast::Pat::Record(r, fs) => {
                if let Some(desugared) = self.record_pattern(r, fs, p.span, false) {
                    self.bind_pattern(&desugared, names);
                }
            }
            ast::Pat::Implicit(_, q) => self.bind_pattern(q, names),
            ast::Pat::Wild | ast::Pat::Lit(_) | ast::Pat::Dot(_) => {}
        }
    }

    /// A pattern, taking the names [`Self::bind_pattern`] bound for it.
    fn rename_pattern(&mut self, p: &ast::LPat, names: &mut std::collections::VecDeque<Ident>) -> hir::Pattern {
        let kind = match &p.node {
            ast::Pat::Bind(param) => {
                if param.unbox || param.grade.is_some() || param.ty.is_some() {
                    self.errors.push(RenameError::PatternNeedsSimpleEquation { span: p.span });
                }
                hir::PatKind::Var(names.pop_front().expect("bound"))
            }
            ast::Pat::As(_, q) => {
                let x = names.pop_front().expect("bound");
                hir::PatKind::As(x, Box::new(self.rename_pattern(q, names)))
            }
            ast::Pat::Con(c, ps) => {
                let c = self.as_term(|r| r.use_name(c));
                let meanings = self.meanings.take();
                let ps = ps.iter().map(|q| self.rename_pattern(q, names)).collect();
                let id = self.fresh_node(p.span);
                if let Some(us) = meanings {
                    self.overloads.insert(id, us);
                }
                return hir::Pattern { id, kind: hir::PatKind::Con(c, ps) };
            }
            ast::Pat::Record(r, fs) => match self.record_pattern(r, fs, p.span, true) {
                Some(desugared) => return self.rename_pattern(&desugared, names),
                None => hir::PatKind::Wild,
            },
            ast::Pat::Implicit(icit, q) => hir::PatKind::Implicit(*icit, Box::new(self.rename_pattern(q, names))),
            ast::Pat::Wild => hir::PatKind::Wild,
            ast::Pat::Lit(l) => hir::PatKind::Lit(l.clone()),
            ast::Pat::Dot(e) => hir::PatKind::Dot(self.as_term(|r| r.rename_term(e))),
            ast::Pat::Seq(ps) => {
                let ps: Vec<(Span, hir::Pattern)> = ps.iter().map(|q| (q.span, self.rename_pattern(q, names))).collect();
                let nil = hir::Pattern { id: self.fresh_node(p.span), kind: hir::PatKind::SeqNil };
                let whole = ps.into_iter().rev().fold(nil, |tail, (at, head)| {
                    let id = self.fresh_node(Span::new(at.start, p.span.end));
                    hir::Pattern { id, kind: hir::PatKind::SeqCons(vec![head, tail]) }
                });
                return whole;
            }
        };
        hir::Pattern { id: self.fresh_node(p.span), kind }
    }

    /// Bind each data type's name, and give its constructors theirs, under
    /// it: a type may mention itself, and the ones after it.
    fn bind_datas(&mut self, datas: &[&ast::LData]) -> Vec<(Ident, Vec<Ident>)> {
        datas
            .iter()
            .map(|d| {
                let name = self.bind_name(&d.node.name);
                let mut ctors: Vec<(String, usize)> = Vec::new();
                let mut spans: HashMap<&str, Span> = HashMap::new();
                let idents = d
                    .node
                    .ctors
                    .iter()
                    .map(|(c, _)| {
                        if let Some(&first) = spans.get(c.node.as_str()) {
                            self.errors.push(RenameError::Duplicate { name: c.node.clone(), first, again: c.span });
                        }
                        spans.insert(&c.node, c.span);
                        let u = self.fresh_unique();
                        ctors.push((c.node.clone(), u));
                        Ident { unique: Some(u), text: c.node.clone() }
                    })
                    .collect();
                if let Some(u) = name.unique {
                    self.ctors.insert(u, ctors);
                    self.type_names.insert(u, name.text.clone());
                    self.note_twin(u);
                }
                (name, idents)
            })
            .collect()
    }

    /// Data declarations, their names bound: the parameters of each scope
    /// over its constructors' fields.
    fn rename_datas(&mut self, datas: &[&ast::LData], names: Vec<(Ident, Vec<Ident>)>) -> Vec<hir::DataDecl> {
        datas
            .iter()
            .zip(names)
            .map(|(d, (name, ctor_names))| {
                let id = self.fresh_node(d.span);
                self.scoped(|r| {
                    // a parameter's kind can mention the parameters before it
                    let params = d
                        .node
                        .params
                        .iter()
                        .map(|(x, kind)| {
                            let kind = kind.as_ref().map(|k| r.as_type(|r| r.rename_term(k)));
                            (r.bind_name(x), kind)
                        })
                        .collect();
                    let kind = d.node.kind.as_ref().map(|k| r.as_type(|r| r.rename_term(k)));
                    let ctors = d
                        .node
                        .ctors
                        .iter()
                        .zip(ctor_names)
                        .map(|((c, shape), name)| {
                            let shape = match shape {
                                ast::Ctor::Fields(fields) => {
                                    hir::CtorShape::Fields(fields.iter().map(|f| r.as_type(|r| r.rename_term(f))).collect())
                                }
                                ast::Ctor::Sig(ty) => {
                                    // (a field that says nothing has the
                                    // module's default, as any binder does)
                                    hir::CtorShape::Sig(r.rename_sig(ty))
                                }
                            };
                            hir::CtorDecl { id: r.fresh_node(c.span), name, shape }
                        })
                        .collect();
                    // this type's own constructors, as the checker makes sure
                    let own = |r: &mut Renamer, c: &ast::Ident| {
                        let found = name.unique.and_then(|u| r.ctors[&u].iter().find(|(x, _)| *x == c.node)).map(|&(_, u)| u);
                        if found.is_none() {
                            let ty = name.text.clone();
                            r.errors.push(RenameError::NoSuchConstructor { ty, name: c.node.clone(), span: c.span });
                        }
                        Ident { unique: found, text: c.node.clone() }
                    };
                    let seq = d.node.seq.as_ref().map(|s| (own(r, &s.nil), own(r, &s.cons)));
                    hir::DataDecl { id, name, params, kind, ctors, seq, linear: r.linear, is_trait: d.node.is_trait }
                })
            })
            .collect()
    }

    /// An effect declaration, its names already bound: the parameters scope
    /// over the operations' types, which bind what else they mention.
    fn rename_effect(&mut self, e: &ast::LEffect, (name, op_names): (Ident, Vec<Ident>)) -> hir::EffectDecl {
        let id = self.fresh_node(e.span);
        self.scoped(|r| {
            let params = e
                .node
                .params
                .iter()
                .map(|(x, kind)| {
                    let kind = kind.as_ref().map(|k| r.as_type(|r| r.rename_term(k)));
                    (r.bind_name(x), kind)
                })
                .collect();
            let ops = e
                .node
                .ops
                .iter()
                .zip(op_names)
                .map(|(op, name)| hir::OpDecl {
                    id: r.fresh_node(op.name.span),
                    name,
                    ty: r.rename_sig(&op.ty),
                    many: op.many,
                })
                .collect();
            hir::EffectDecl { id, name, params, ops }
        })
    }

    /// Bind a pattern's name in the innermost scope.  Its annotation is
    /// renamed first, so it cannot see the name being bound.
    fn bind_param(&mut self, p: &ast::LParam) -> BoundParam {
        let grade = p.node.grade.as_ref().map(|g| self.rename_grades(g));
        let ty = p.node.ty.as_ref().map(|t| self.as_type(|r| r.rename_term(t)));
        let name = self.bind_name(&p.node.name);
        // the box an unboxing pattern receives shares the name's spelling (it
        // is what shows up in an inferred type) but cannot be referred to
        let boxed = p
            .node
            .unbox
            .then(|| Ident { unique: Some(self.fresh_unique()), text: p.node.name.node.clone() });
        BoundParam { span: p.span, name, boxed, grade, ty }
    }

    /// `\x -> body`, or for an unboxing pattern
    /// `\[x] -> body  ~~>  \b -> let [x] = b in body`.
    fn wrap_param(&mut self, span: Span, p: BoundParam, body: hir::Term) -> hir::Term {
        use hir::TermKind as H;
        match p.boxed {
            None => self.node(span, H::Lam(p.name, p.grade, p.ty.map(Box::new), Box::new(body))),
            Some(boxed) => {
                let scrutinee = self.node(p.span, H::Var(boxed.clone()));
                let unboxed = self.node(span, H::LetBox(p.name, Box::new(scrutinee), Box::new(body)));
                self.node(span, H::Lam(boxed, None, None, Box::new(unboxed)))
            }
        }
    }

    /// Rename a block of declarations as one recursive group: every defined
    /// name is in scope for every right-hand side (and for the body, handled
    /// by the caller).  Signatures are paired up with their equations.
    fn rename_decls(&mut self, decls: &[ast::LDecl]) -> Vec<hir::Bind> {
        let defs = self.bind_decls(decls);
        self.rename_defs(defs)
    }

    /// The first half of [`Self::rename_decls`]: pair signatures with
    /// equations, and bind every name the block defines.
    fn bind_decls<'a>(&mut self, decls: &'a [ast::LDecl]) -> Vec<Def<'a>> {
        let mut sigs: HashMap<&str, (&ast::Ident, &LTerm)> = HashMap::new();
        for d in decls {
            if let ast::Decl::Sig(name, ty) = &d.node {
                if let Some((first, _)) = sigs.insert(&name.node, (name, ty)) {
                    self.errors.push(RenameError::Duplicate {
                        name: name.node.clone(),
                        first: first.span,
                        again: name.span,
                    });
                }
            }
        }
        // Equations for the same name in a row are one definition, if they
        // have patterns to tell them apart (`x = 1; x = 2` is two).
        let mut equations: Vec<Vec<&ast::LDecl>> = Vec::new();
        for d in decls {
            let ast::Decl::Def(name, pats, ..) = &d.node else { continue };
            let continues = |first: &ast::LDecl| {
                matches!(&first.node, ast::Decl::Def(f, ps, ..) if f.node == name.node && !ps.is_empty() && !pats.is_empty())
            };
            match equations.last_mut() {
                Some(last) if continues(last[0]) => last.push(d),
                _ => equations.push(vec![d]),
            }
        }
        // introduce all names so the group is mutually recursive, each
        // with its signature if it has one
        let defs: Vec<Def<'a>> = equations
            .into_iter()
            .map(|eqs| {
                let ast::Decl::Def(name, ..) = &eqs[0].node else { unreachable!("only equations") };
                let ident = self.bind_name(name);
                let sig = sigs.remove(ident.text.as_str()).map(|(_, ty)| ty);
                (eqs, ident, sig)
            })
            .collect();
        let mut orphans: Vec<&ast::Ident> = sigs.into_values().map(|(name, _)| name).collect();
        orphans.sort_by_key(|name| name.span.start);
        for name in orphans {
            self.errors.push(RenameError::SignatureWithoutDefinition {
                name: name.node.clone(),
                span: name.span,
            });
        }
        defs
    }

    /// The second half: each definition, every name bound.
    fn rename_defs(&mut self, defs: Vec<Def<'_>>) -> Vec<hir::Bind> {
        defs.into_iter().map(|(eqs, ident, sig)| self.rename_def(&eqs, ident, sig)).collect()
    }

    fn rename_def(&mut self, eqs: &[&ast::LDecl], name: Ident, sig: Option<&LTerm>) -> hir::Bind {
        let d = eqs[0];
        let ast::Decl::Def(_, params, inline, rhs) = &d.node else {
            unreachable!("rename_def is only given equations")
        };
        let span = eqs.iter().fold(d.span, |acc, e| acc.merge(e.span));
        let id = self.fresh_node(span);
        // `x : a = e` is a signature written inline; anywhere else an inline
        // type is the *result* type, an annotation on the right-hand side
        let (sig, result_ty) = match (sig, inline) {
            (None, Some(ty)) if params.is_empty() && eqs.len() == 1 => (Some(ty), None),
            _ => (sig, inline.as_ref()),
        };
        let ty = sig.map(|t| self.rename_sig(t));
        // A definition whose type ends in a universe defines a type, so its
        // body is one: `pairOf a = (a, a)` is the pair type, not a pair.
        if sig.is_some_and(ends_in_universe) && !self.in_type {
            return self.as_type(|r| r.rename_def_body(d, eqs, id, span, name, ty, params, rhs, result_ty));
        }
        self.rename_def_body(d, eqs, id, span, name, ty, params, rhs, result_ty)
    }

    #[allow(clippy::too_many_arguments)]
    fn rename_def_body(
        &mut self,
        d: &ast::LDecl,
        eqs: &[&ast::LDecl],
        id: NodeId,
        span: Span,
        name: Ident,
        ty: Option<hir::Term>,
        params: &[ast::LPat],
        rhs: &ast::Rhs,
        result_ty: Option<&LTerm>,
    ) -> hir::Bind {
        // One equation of variables is a lambda; anything else is matched.
        let simple = |p: &ast::LPat| matches!(p.node, ast::Pat::Bind(_) | ast::Pat::Wild);
        let rhs = match rhs {
            ast::Rhs::Plain(rhs) if eqs.len() == 1 && params.iter().all(simple) => rhs,
            _ => return self.rename_match(id, span, eqs, name, ty),
        };
        let params: Vec<ast::LParam> = params
            .iter()
            .map(|p| match &p.node {
                ast::Pat::Bind(param) => Spanned::new(param.clone(), p.span),
                _ => {
                    let name = Spanned::new("_".to_string(), p.span);
                    Spanned::new(ast::Param { name, unbox: false, grade: None, ty: None }, p.span)
                }
            })
            .collect();
        self.scoped(|r| {
            r.scope_quantified(ty.as_ref());
            // Patterns scope over each other's later annotations, the result
            // type, and the body.  They get a scope of their own: shadowing a
            // quantified variable is not a duplicate binding.
            let (params, body) = r.scoped(|r| {
                let ps: Vec<BoundParam> = params.iter().map(|p| r.bind_param(p)).collect();
                let mut body = r.rename_term(rhs);
                if let Some(ty) = result_ty {
                    let span = rhs.span.merge(ty.span);
                    let ty = r.as_type(|r| r.rename_term(ty));
                    body = r.node(span, hir::TermKind::Ann(Box::new(body), Box::new(ty)));
                }
                (ps, body)
            });
            // f (x : a) y = e  ~~>  f = \x y -> e
            let rhs = params.into_iter().rev().fold(body, |body, p| r.wrap_param(d.span, p, body));
            hir::Bind { id, name, ty, rhs, linear: r.linear }
        })
    }

    /// A definition by matching: each equation a clause.
    fn rename_match(
        &mut self,
        id: NodeId,
        span: Span,
        eqs: &[&ast::LDecl],
        name: Ident,
        ty: Option<hir::Term>,
    ) -> hir::Bind {
        self.scoped(|r| {
            r.scope_quantified(ty.as_ref());
            let clauses = eqs
                .iter()
                .map(|d| {
                    let ast::Decl::Def(_, pats, result_ty, rhs) = &d.node else { unreachable!("only equations") };
                    r.rename_clause(d.span, pats, result_ty.as_ref(), rhs)
                })
                .collect();
            let rhs = r.node(span, hir::TermKind::Match(clauses));
            hir::Bind { id, name, ty, rhs, linear: r.linear }
        })
    }

    /// The variables a signature quantifies over scope over the equation,
    /// as type variables do in Granule (and in Haskell with
    /// ScopedTypeVariables).
    fn scope_quantified(&mut self, ty: Option<&hir::Term>) {
        let mut quantified = ty;
        while let Some(hir::Term { id, kind: hir::TermKind::Pi(Some(x), Icit::Implicit | Icit::Auto, _, _, cod) }) = quantified {
            if let Some(u) = x.unique {
                let span = self.spans.get(*id).unwrap_or(Span::at(0));
                self.scopes.last_mut().expect("scope stack is never empty").insert(x.text.clone(), (u, span));
            }
            quantified = Some(cod);
        }
    }
}

/// Whether a signature's type ends in a universe, `.. -> Type`: what it
/// gives is a type.
fn ends_in_universe(t: &LTerm) -> bool {
    match &t.node {
        ast::Term::Pi(_, _, _, _, cod) => ends_in_universe(cod),
        ast::Term::Type => true,
        ast::Term::App(f, _) => matches!(f.node, ast::Term::Type),
        _ => false,
    }
}

fn rhs_span(rhs: &ast::Rhs) -> Option<Span> {
    match rhs {
        ast::Rhs::Plain(e) => Some(e.span),
        ast::Rhs::Guarded(gs) => gs.last().map(|(_, e)| e.span),
        ast::Rhs::Impossible => None,
    }
}

/// The lower-case names `t` mentions that are neither bound inside it nor
/// `known` (in scope), in order of first appearance.
fn free_variables(t: &LTerm, bound: &mut Vec<String>, known: &dyn Fn(&str) -> bool, out: &mut Vec<ast::Ident>) {
    use ast::Term as A;
    let under = |x: Option<&str>, t: &LTerm, bound: &mut Vec<String>, out: &mut Vec<ast::Ident>| {
        bound.extend(x.map(str::to_string));
        free_variables(t, bound, known, out);
        if x.is_some() {
            bound.pop();
        }
    };
    let grades = |g: &Option<ast::GradeAnn>, bound: &mut Vec<String>, out: &mut Vec<ast::Ident>| {
        if let Some(g) = g {
            free_variables(&g.subject, bound, known, out);
            if let Some(r) = &g.in_type {
                free_variables(r, bound, known, out);
            }
        }
    };
    match &t.node {
        A::Var(x) => {
            let lower = x.node.starts_with(|c: char| c.is_ascii_lowercase() || c == '_');
            let seen = bound.contains(&x.node) || out.iter().any(|y| y.node == x.node);
            if lower && !seen && !known(&x.node) && x.node != "otherwise" {
                out.push(x.clone());
            }
        }
        A::Type | A::Lit(_) | A::Hole | A::Inf | A::Resolved(..) | A::Search | A::Equations(_) => {}
        A::Pi(x, _, g, dom, cod) => {
            grades(g, bound, out);
            free_variables(dom, bound, known, out);
            under(x.as_ref().map(|x| x.node.as_str()), cod, bound, out);
        }
        A::Lam(p, body) => {
            grades(&p.node.grade, bound, out);
            if let Some(ty) = &p.node.ty {
                free_variables(ty, bound, known, out);
            }
            under(Some(&p.node.name.node), body, bound, out);
        }
        A::App(f, a) | A::ImplicitApp(f, a) | A::AutoApp(f, a) | A::Ann(f, a) | A::Range(f, a) | A::Eff(f, a) => {
            free_variables(f, bound, known, out);
            free_variables(a, bound, known, out);
        }
        A::Row(labels, tail) => {
            for l in labels.iter().chain(tail.as_deref()) {
                free_variables(l, bound, known, out);
            }
        }
        A::BoxIntro(e) => free_variables(e, bound, known, out),
        A::Seq(items) | A::ArrayLit(items) => items.iter().for_each(|e| free_variables(e, bound, known, out)),
        A::RecordLit(_, values) => values.iter().for_each(|(_, e)| free_variables(e, bound, known, out)),
        A::RecordOf(_, args, values, _) => {
            args.iter().for_each(|e| free_variables(e, bound, known, out));
            values.iter().for_each(|(_, e)| free_variables(e, bound, known, out));
        }
        A::Proj(e, _) => free_variables(e, bound, known, out),
        A::Update(e, values) => {
            free_variables(e, bound, known, out);
            values.iter().for_each(|(_, v)| free_variables(v, bound, known, out));
        }
        A::LetBox(x, e, body) => {
            free_variables(e, bound, known, out);
            under(Some(&x.node), body, bound, out);
        }
        // not what signatures are made of: only the body, with the names
        // the block defines bound
        A::Let(decls, body) => {
            let names: Vec<String> = decls
                .iter()
                .filter_map(|d| match &d.node {
                    ast::Decl::Def(x, ..) => Some(x.node.clone()),
                    ast::Decl::Sig(..) | ast::Decl::Native(..) => None,
                })
                .collect();
            let n = names.len();
            bound.extend(names);
            free_variables(body, bound, known, out);
            bound.truncate(bound.len() - n);
        }
        A::Case(scrutinee, _) | A::Handle(scrutinee, _) => free_variables(scrutinee, bound, known, out),
    }
}
