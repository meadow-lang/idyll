//! The embedded **`Std`** package: Idyll's standard library.
//!
//! `lib/Std/` on disk is the source of truth; its modules are
//! `include_str!`d here, so the compiler always has them, with no file to
//! find. [`compiled`] checks them once, as a package called `Std`, and
//! [`crate::pipeline`] gives it to every package it compiles as an implicit
//! dependency: `use Std.Collections.List (map)` works with no `[dependencies]` line, in
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

/// Its modules, by their paths in it (`Collections.List` is the file
/// `src/Collections/List.idl`), the root first.
pub const MODULES: &[(&str, &str)] = &[
    ("", include_str!("../../../lib/Std/src/Lib.idl")),
    ("Bool", include_str!("../../../lib/Std/src/Bool.idl")),
    ("Bytes", include_str!("../../../lib/Std/src/Bytes.idl")),
    ("Char", include_str!("../../../lib/Std/src/Char.idl")),
    ("Cmp", include_str!("../../../lib/Std/src/Cmp.idl")),
    ("Collections.Array", include_str!("../../../lib/Std/src/Collections/Array.idl")),
    ("Collections.HashMap", include_str!("../../../lib/Std/src/Collections/HashMap.idl")),
    ("Collections.List", include_str!("../../../lib/Std/src/Collections/List.idl")),
    ("Collections.Map", include_str!("../../../lib/Std/src/Collections/Map.idl")),
    ("Collections.MArray", include_str!("../../../lib/Std/src/Collections/MArray.idl")),
    ("Collections.Set", include_str!("../../../lib/Std/src/Collections/Set.idl")),
    ("Collections.Vec", include_str!("../../../lib/Std/src/Collections/Vec.idl")),
    ("Collections.Vector", include_str!("../../../lib/Std/src/Collections/Vector.idl")),
    ("Console", include_str!("../../../lib/Std/src/Console.idl")),
    ("Dec", include_str!("../../../lib/Std/src/Dec.idl")),
    ("Either", include_str!("../../../lib/Std/src/Either.idl")),
    ("Eq", include_str!("../../../lib/Std/src/Eq.idl")),
    ("Exn", include_str!("../../../lib/Std/src/Exn.idl")),
    ("Fs", include_str!("../../../lib/Std/src/Fs.idl")),
    ("Function", include_str!("../../../lib/Std/src/Function.idl")),
    ("Hash", include_str!("../../../lib/Std/src/Hash.idl")),
    ("Json", include_str!("../../../lib/Std/src/Json.idl")),
    ("Maybe", include_str!("../../../lib/Std/src/Maybe.idl")),
    ("Num.Bits", include_str!("../../../lib/Std/src/Num/Bits.idl")),
    ("Num.Fin", include_str!("../../../lib/Std/src/Num/Fin.idl")),
    ("Num.Int", include_str!("../../../lib/Std/src/Num/Int.idl")),
    ("Num.Nat", include_str!("../../../lib/Std/src/Num/Nat.idl")),
    ("Ordering", include_str!("../../../lib/Std/src/Ordering.idl")),
    ("Process", include_str!("../../../lib/Std/src/Process.idl")),
    ("Random", include_str!("../../../lib/Std/src/Random.idl")),
    ("Result", include_str!("../../../lib/Std/src/Result.idl")),
    ("Sigma", include_str!("../../../lib/Std/src/Sigma.idl")),
    ("Sort", include_str!("../../../lib/Std/src/Sort.idl")),
    ("State", include_str!("../../../lib/Std/src/State.idl")),
    ("Stream", include_str!("../../../lib/Std/src/Stream.idl")),
    ("String", include_str!("../../../lib/Std/src/String.idl")),
    ("String.Parse", include_str!("../../../lib/Std/src/String/Parse.idl")),
    ("String.Parse.Char", include_str!("../../../lib/Std/src/String/Parse/Char.idl")),
    ("String.Parse.Lexer", include_str!("../../../lib/Std/src/String/Parse/Lexer.idl")),
    ("Test", include_str!("../../../lib/Std/src/Test.idl")),
    ("Time", include_str!("../../../lib/Std/src/Time.idl")),
    ("Tuple", include_str!("../../../lib/Std/src/Tuple.idl")),
    ("Void", include_str!("../../../lib/Std/src/Void.idl")),
    ("Yield", include_str!("../../../lib/Std/src/Yield.idl")),
];

/// The package to compile: its modules, as files of `lib/Std/src/`.
pub fn source() -> PackageSource {
    let modules = MODULES
        .iter()
        .map(|(path, text)| {
            let file = if path.is_empty() { "Lib".to_string() } else { path.replace('.', "/") };
            ModuleFile {
                path: if path.is_empty() { Vec::new() } else { path.split('.').map(String::from).collect() },
                file: format!("<std>/{file}.idl"),
                text: text.to_string(),
            }
        })
        .collect();
    PackageSource { name: NAME.to_string(), version: None, linear: false, modules }
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
