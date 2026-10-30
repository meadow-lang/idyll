//! The **build driver**: filesystem package discovery, compiling the package
//! graph in order with [`idyllic::pipeline::check_package_with`] -- reading
//! back from the cache each package that has not changed -- and linking what
//! a program needs.
//!
//! Every package is checked against the *compiled* packages it depends on,
//! never their source (see `idyllic_typecheck::packages`), so each is compiled
//! once however many packages depend on it, and only when something it was
//! compiled from changed.

use crate::incremental::{self, Cache};
use crate::package::{Diagnostic, PackageGraph};
use crate::profile::Profile;
use crate::status;
use crate::workspace::Workspace;
use idyllic::pipeline::{self, CompiledPackage, Deps, Options};
use std::path::{Path, PathBuf};

/// A package's compile errors, with the files they are about.
pub struct Reported {
    pub package: String,
    pub diagnostics: Vec<idyllic_diagnostics::Diagnostic>,
    pub sources: idyllic_span::SourceMap,
}

impl Reported {
    pub fn errors(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.is_error()).count()
    }

    /// Each diagnostic's message, as a test reads them.
    pub fn messages(&self) -> Vec<String> {
        self.diagnostics.iter().map(|d| d.message.clone()).collect()
    }
}

pub struct BuildOutput {
    /// The program: every package it uses, compiled, dependencies first and
    /// the package itself last. Empty when it could not be compiled.
    pub program: Vec<CompiledPackage>,
    /// Why the packages could not be found, when they could not.
    pub problem: Option<Diagnostic>,
    /// What compiling said, package by package.
    pub reported: Vec<Reported>,
    /// The package that was built: the directory [`crate::artifacts`] puts
    /// its `target` under -- its own, or its workspace's root -- and its name.
    pub package: Option<(PathBuf, String)>,
    /// The packages that were compiled, in the order they were: every other
    /// package the build needed was the same as last time, and read back
    /// instead -- see [`crate::incremental`].
    pub compiled: Vec<String>,
    /// The built package's fingerprint, which is of everything it was compiled
    /// from -- its dependencies' and the compiler's included: what an
    /// executable made of it is known by.
    pub fingerprint: Option<u64>,
}

impl BuildOutput {
    fn failed(d: Diagnostic) -> BuildOutput {
        BuildOutput {
            program: Vec::new(),
            problem: Some(d),
            reported: Vec::new(),
            package: None,
            compiled: Vec::new(),
            fingerprint: None,
        }
    }

    /// How many errors stopped it: none for a build that can be run.
    pub fn errors(&self) -> usize {
        usize::from(self.problem.is_some()) + self.reported.iter().map(Reported::errors).sum::<usize>()
    }

    /// The program's packages, as linking takes them.
    pub fn packages(&self) -> Vec<&CompiledPackage> {
        self.program.iter().collect()
    }

    /// Whether the package has an expression, or a `main`, to run.
    pub fn runnable(&self) -> bool {
        self.program.last().is_some_and(|p| p.body.is_some())
    }
}

/// Discover, compile and link the package rooted at `entry`.
pub fn build(entry: &Path, profile: Profile, opts: &Options) -> BuildOutput {
    let lock_dir = crate::lock::dir_for(entry);
    let mut resolver = crate::package::Resolver::for_entry(&lock_dir);
    build_resolved(entry, profile, opts, &mut resolver)
}

/// The package at `entry` built to run its tests, those `tests` selects
/// (`idyll test`): what runs is `Std.Test`'s runner over them, in order.
pub fn build_tests(entry: &Path, profile: Profile, opts: &Options, tests: &pipeline::TestSelection) -> BuildOutput {
    let lock_dir = crate::lock::dir_for(entry);
    let mut resolver = crate::package::Resolver::for_entry(&lock_dir);
    let graph = match discover_with(&[entry], &mut resolver) {
        Ok(g) => g,
        Err(d) => return BuildOutput::failed(d),
    };
    let package = target_of(&graph, graph.root());
    let cache = cache_for(&package, profile);
    let compiled = compile_graph_testing(&graph, opts, cache.as_ref(), Some(tests));
    let root = graph.root();
    BuildOutput {
        program: program_of(&graph, &compiled, root),
        problem: None,
        reported: compiled.reported,
        package,
        compiled: compiled.compiled,
        fingerprint: Some(compiled.fingerprints[root]),
    }
}

/// [`build`], resolving dependencies with `resolver` rather than the one the
/// command line set up: how a test fetches into a cache of its own.
pub fn build_resolved(
    entry: &Path,
    profile: Profile,
    opts: &Options,
    resolver: &mut crate::package::Resolver,
) -> BuildOutput {
    let graph = match discover_with(&[entry], resolver) {
        Ok(g) => g,
        Err(d) => return BuildOutput::failed(d),
    };
    let package = target_of(&graph, graph.root());
    let cache = cache_for(&package, profile);
    let compiled = compile_graph(&graph, opts, cache.as_ref());
    let root = graph.root();
    BuildOutput {
        program: program_of(&graph, &compiled, root),
        problem: None,
        reported: compiled.reported,
        package,
        compiled: compiled.compiled,
        fingerprint: Some(compiled.fingerprints[root]),
    }
}

/// Several packages built at once -- a workspace's members -- sharing
/// everything they have in common: each package is compiled once however
/// many of them depend on it.
pub struct ManyOutput {
    /// Each package asked for, with what it depends on, in the order asked;
    /// empty when the packages could not be found.
    pub each: Vec<BuildOutput>,
    /// Why they could not be found, when they could not.
    pub problem: Option<Diagnostic>,
    /// What compiling said, for all of them.
    pub reported: Vec<Reported>,
    /// The packages compiled rather than reused, for all of them.
    pub compiled: Vec<String>,
}

impl ManyOutput {
    pub fn errors(&self) -> usize {
        usize::from(self.problem.is_some()) + self.reported.iter().map(Reported::errors).sum::<usize>()
    }
}

/// Build every package in `entries`, each a program of its own.
pub fn build_each(entries: &[&Path], profile: Profile, opts: &Options) -> ManyOutput {
    let lock_dir = crate::lock::dir_for(entries.first().copied().unwrap_or(Path::new(".")));
    let mut resolver = crate::package::Resolver::for_entry(&lock_dir);
    let graph = match discover_with(entries, &mut resolver) {
        Ok(g) => g,
        Err(d) => {
            return ManyOutput { each: Vec::new(), problem: Some(d), reported: Vec::new(), compiled: Vec::new() };
        }
    };
    let cache = cache_for(&target_of(&graph, graph.root()), profile);
    let compiled = compile_graph(&graph, opts, cache.as_ref());
    let each = graph
        .roots()
        .iter()
        .map(|&root| BuildOutput {
            program: program_of(&graph, &compiled, root),
            problem: None,
            reported: Vec::new(),
            package: target_of(&graph, root),
            compiled: Vec::new(),
            fingerprint: Some(compiled.fingerprints[root]),
        })
        .collect();
    ManyOutput { each, problem: None, reported: compiled.reported, compiled: compiled.compiled }
}

/// `root` and everything it depends on, compiled, dependencies first: or
/// nothing, if any of them could not be.
fn program_of(graph: &PackageGraph, compiled: &Compiled, root: usize) -> Vec<CompiledPackage> {
    let ids = graph.closure(root);
    ids.iter().map(|&id| compiled.packages[id].clone()).collect::<Option<Vec<_>>>().unwrap_or_default()
}

/// Where a build whose `target` is `package`'s keeps compiled packages for
/// the next one: none for a lone file, which has no `target`.
fn cache_for(package: &Option<(PathBuf, String)>, profile: Profile) -> Option<Cache> {
    package.as_ref().and_then(|(root, _)| Cache::new(root, profile))
}

/// [`discover`], with a resolver of the caller's: how a test fetches into a
/// cache of its own, and how `idyll update` re-resolves.
pub fn discover_with(
    entries: &[&Path],
    resolver: &mut crate::package::Resolver,
) -> Result<PackageGraph, Diagnostic> {
    // A package inside a workspace it is not a member of would build with the
    // wrong profiles into the wrong `target`: say so instead.
    for entry in entries {
        if let Err(msg) = Workspace::find(entry) {
            return Err(Diagnostic { msg, filename: entry.display().to_string(), label: String::new() });
        }
    }
    // Dependencies are resolved against the lockfile, and what was resolved is
    // written back -- so a first build pins what it found, and every build
    // after it uses those commits until `idyll update` says otherwise.
    let lock_dir = crate::lock::dir_for(entries.first().copied().unwrap_or(Path::new(".")));
    let graph = PackageGraph::build_all_with(entries, resolver)?;
    // Said before anything is compiled, so that a manifest on its way out is
    // seen whether or not the build goes on to succeed.
    for w in graph.warnings() {
        status::warning(w);
    }
    if !resolver.seen.is_empty() {
        resolver.lock.retain(&resolver.seen);
        if let Err(e) = resolver.lock.save(&lock_dir) {
            status::warning(format!("could not write {}: {e}", crate::lock::FILE));
        }
    }
    Ok(graph)
}

/// Where package `id` of `graph` keeps its `target` directory, and its name:
/// its own directory, or its workspace's root. A lone `.idl` file is compiled
/// as a package of one, but has no directory of its own to keep one in.
fn target_of(graph: &PackageGraph, id: usize) -> Option<(PathBuf, String)> {
    let pkg = &graph.packages[id];
    if !pkg.root.is_dir() {
        return None;
    }
    let dir = match Workspace::find(&pkg.root) {
        Ok(Some(ws)) => ws.root,
        _ => pkg.root.clone(),
    };
    Some((dir, pkg.name.clone()))
}

/// Every package of a graph, compiled.
struct Compiled {
    /// Indexed by package id; `None` for one that could not be compiled, or
    /// depends on one that could not.
    packages: Vec<Option<CompiledPackage>>,
    reported: Vec<Reported>,
    /// Which were compiled rather than read back from the cache.
    compiled: Vec<String>,
    /// Each package's fingerprint, by id: see [`incremental::fingerprint`].
    fingerprints: Vec<u64>,
}

/// Who each package of `graph` is, for keeping its compiled self under:
/// `name@version`, which tells two copies of one package apart. Two copies
/// of the *same* version -- the same package from two places -- take where
/// they came from as well, since nothing else distinguishes them.
fn identities(graph: &PackageGraph) -> Vec<String> {
    let plain: Vec<String> = graph.packages.iter().map(|p| p.ident()).collect();
    graph
        .packages
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let shared = plain.iter().enumerate().any(|(j, other)| j != i && *other == plain[i]);
            if shared {
                let mut h: u64 = 0xcbf2_9ce4_8422_2325;
                for b in p.origin.bytes() {
                    h ^= u64::from(b);
                    h = h.wrapping_mul(0x0100_0000_01b3);
                }
                format!("{}#{:08x}", plain[i], h as u32)
            } else {
                plain[i].clone()
            }
        })
        .collect()
}

/// Compile every package of `graph`, dependencies first -- reading back from
/// `cache` each one whose inputs are what they were when it was saved.
fn compile_graph(graph: &PackageGraph, opts: &Options, cache: Option<&Cache>) -> Compiled {
    compile_graph_testing(graph, opts, cache, None)
}

/// [`compile_graph`]; with `tests`, the root package's program is its tests
/// (`idyll test`), compiled afresh and not kept, since it is no build of the
/// package another could reuse.
fn compile_graph_testing(graph: &PackageGraph, opts: &Options, cache: Option<&Cache>, tests: Option<&pipeline::TestSelection>) -> Compiled {
    let n = graph.packages.len();
    let mut packages: Vec<Option<CompiledPackage>> = (0..n).map(|_| None).collect();
    let mut fingerprints = vec![0u64; n];
    // Compiled without a diagnostic, and so did everything under it: only
    // such a package is saved.
    let mut clean = vec![false; n];
    let mut compiled = Vec::new();
    let mut reported = Vec::new();
    // Each package's uniques start above every package compiled before it,
    // not only above its own dependencies: two packages beside each other
    // would otherwise share them, and one program holding both -- a package
    // depending on each, or a workspace -- would have one stand for the
    // other. On a slot boundary, so that one package growing does not move
    // the uniques of every package after it, which would make each of them a
    // change.
    // The first slot is above the builtins' and the prelude's uniques, which
    // are the same in every package.
    let mut floor = incremental::ID_CHUNK;
    let idents = identities(graph);

    // The bar counts every package in the graph. One found up to date moves it
    // on without a line of its own, as cargo does with a crate that is fresh.
    let mut bar = status::Building::new(graph.order().len());
    for &pid in graph.order() {
        let pkg = &graph.packages[pid];
        let dep_prints: Vec<u64> = pkg.deps.iter().map(|&d| fingerprints[d]).collect();
        let fingerprint = incremental::fingerprint(pkg, &dep_prints, floor);
        fingerprints[pid] = fingerprint;
        bar.step();
        // what it depends on, directly or not, in the order they are linked
        let closure: Vec<usize> = graph.closure(pid).into_iter().filter(|&id| id != pid).collect();
        let Some(all) = closure.iter().map(|&id| packages[id].as_ref()).collect::<Option<Vec<&CompiledPackage>>>()
        else {
            // (what it depends on was not compiled, which has said why) It
            // keeps its slot all the same, so that the packages after it
            // start where they did and are not compiled again for it.
            floor = incremental::align(floor + 1);
            continue;
        };
        let testing = tests.filter(|_| pid == graph.root());
        let reused = cache.filter(|_| testing.is_none()).and_then(|c| c.load(&idents[pid], fingerprint));
        let cp = match reused {
            Some(cp) => {
                clean[pid] = true;
                cp
            }
            None => {
                let version = pkg.version.as_deref().map(|v| format!(" v{v}")).unwrap_or_default();
                status::status("Compiling", format!("{}{version} ({})", pkg.name, pkg.origin));
                bar.working_on(&pkg.name);
                // every dependency by the name *this* package calls it, which
                // is what its `use` lines say and need not be what it calls
                // itself
                let direct: Vec<(&str, &CompiledPackage)> = pkg
                    .deps
                    .iter()
                    .zip(&pkg.dep_names)
                    .map(|(&d, alias)| (alias.as_str(), packages[d].as_ref().expect("topological order")))
                    .collect();
                // its parts compiled before, whose inputs did not change
                let parts = cache.map(|cache| incremental::Parts {
                    cache,
                    ident: idents[pid].clone(),
                    salt: incremental::parts_salt(pkg, &dep_prints, floor),
                });
                let deps = Deps {
                    all: &all,
                    direct: &direct,
                    first_unique: floor,
                    parts: parts.as_ref().map(|p| p as &dyn pipeline::PartStore),
                    tests: testing,
                };
                // Every package is compiled as a program, its `main` to be
                // run, whether or not it is the one asked for: what a
                // dependent links is only its definitions, and one compiled
                // package serves both.
                let out = pipeline::check_package_with(&pkg.source(), deps, opts, true);
                compiled.push(pkg.name.clone());
                let has_errors = out.has_errors();
                let quiet = out.diagnostics.is_empty();
                let cp = out.checked.and_then(|c| c.compiled);
                if !quiet {
                    reported.push(Reported {
                        package: pkg.name.clone(),
                        diagnostics: out.diagnostics,
                        sources: out.sources,
                    });
                }
                let cp = match cp {
                    Some(cp) if !has_errors => cp,
                    // Its slot too: what it would have named its things with,
                    // as far as it got.
                    failed => {
                        let end = failed.map_or(0, |cp| cp.uniques.end);
                        floor = incremental::align(end.max(floor + 1));
                        continue;
                    }
                };
                clean[pid] = quiet && pkg.deps.iter().all(|&d| clean[d]);
                if clean[pid]
                    && testing.is_none()
                    && let Some(cache) = cache
                {
                    cache.store(&idents[pid], &cp, fingerprint);
                }
                cp
            }
        };
        floor = incremental::align(floor.max(cp.uniques.end));
        packages[pid] = Some(cp);
    }
    drop(bar);
    Compiled { packages, reported, compiled, fingerprints }
}

