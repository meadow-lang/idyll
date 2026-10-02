//! The embedded **`Std`** package: Idyll's standard library.
//!
//! `lib/std/` on disk is the source of truth; its modules are
//! `include_str!`d here, so the compiler always has them, with no file to
//! find. [`compiled`] checks them once, as a package called `Std`, and
//! [`crate::pipeline`] gives it to every package it compiles as an implicit
//! dependency: `use Std.List (map)` works with no `[dependencies]` line, in
//! a package, a lone file and the REPL alike. Nothing of it is in scope
//! unqualified until a `use` brings it, so a program's own `Nat` or `map`
//! never clashes with Std's.
//!
//! It is compiled once a process, and kept as bytes (a compiled package is
//! made of `Rc`s, which cannot be shared between threads); each thread
//! reads its own copy back the first time it wants one.

use crate::pipeline::{self, CompiledPackage, Deps, ModuleFile, Options, PackageSource};
use std::cell::OnceCell;
use std::rc::Rc;
use std::sync::OnceLock;

/// What a package calls it, and what it calls itself.
pub const NAME: &str = "Std";

/// Its modules, by their paths in it, the root first.
pub const MODULES: &[(&str, &str)] = &[
    ("", include_str!("../../../lib/std/src/Lib.idl")),
    ("Bool", include_str!("../../../lib/std/src/Bool.idl")),
    ("Console", include_str!("../../../lib/std/src/Console.idl")),
    ("Dec", include_str!("../../../lib/std/src/Dec.idl")),
    ("Eq", include_str!("../../../lib/std/src/Eq.idl")),
    ("Fin", include_str!("../../../lib/std/src/Fin.idl")),
    ("Function", include_str!("../../../lib/std/src/Function.idl")),
    ("Int", include_str!("../../../lib/std/src/Int.idl")),
    ("List", include_str!("../../../lib/std/src/List.idl")),
    ("Maybe", include_str!("../../../lib/std/src/Maybe.idl")),
    ("Nat", include_str!("../../../lib/std/src/Nat.idl")),
    ("Ordering", include_str!("../../../lib/std/src/Ordering.idl")),
    ("Pair", include_str!("../../../lib/std/src/Pair.idl")),
    ("Result", include_str!("../../../lib/std/src/Result.idl")),
    ("Sigma", include_str!("../../../lib/std/src/Sigma.idl")),
    ("State", include_str!("../../../lib/std/src/State.idl")),
    ("String", include_str!("../../../lib/std/src/String.idl")),
    ("Throw", include_str!("../../../lib/std/src/Throw.idl")),
    ("Vec", include_str!("../../../lib/std/src/Vec.idl")),
    ("Void", include_str!("../../../lib/std/src/Void.idl")),
    ("Yield", include_str!("../../../lib/std/src/Yield.idl")),
];

/// The package to compile: its modules, as files of `lib/std/src/`.
pub fn source() -> PackageSource {
    let modules = MODULES
        .iter()
        .map(|(path, text)| {
            let file = if path.is_empty() { "Lib".to_string() } else { path.to_string() };
            ModuleFile {
                path: if path.is_empty() { Vec::new() } else { vec![path.to_string()] },
                file: format!("<std>/{file}.idl"),
                text: text.to_string(),
            }
        })
        .collect();
    PackageSource { name: NAME.to_string(), modules }
}

/// Std, compiled: once a process, and read back once a thread.
///
/// It must compile; one that does not is a bug in the library, and every
/// diagnostic is in the panic.
pub fn compiled() -> Rc<CompiledPackage> {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    thread_local! {
        static COPY: OnceCell<Rc<CompiledPackage>> = const { OnceCell::new() };
    }
    COPY.with(|copy| {
        copy.get_or_init(|| {
            let bytes = BYTES.get_or_init(|| {
                let package = compile().unwrap_or_else(|errors| panic!("Std does not compile:\n{errors}"));
                postcard::to_allocvec(&package).expect("a compiled package serializes")
            });
            Rc::new(postcard::from_bytes(bytes).expect("Std reads back as it was written"))
        })
        .clone()
    })
}

/// Check Std, as a package depending on nothing: what it compiled to, or
/// its diagnostics, rendered.
pub fn compile() -> Result<CompiledPackage, String> {
    let opts = Options { debug: false, style: idyllic_diagnostics::OutputStyle::PLAIN, live: false };
    let out = pipeline::check_package_with(&source(), Deps::default(), &opts, false);
    let has_errors = out.has_errors();
    match out.checked.and_then(|c| c.compiled) {
        Some(package) if !has_errors => Ok(package),
        _ => Err(idyllic_diagnostics::render_map(&out.diagnostics, &out.sources, opts.style)),
    }
}
