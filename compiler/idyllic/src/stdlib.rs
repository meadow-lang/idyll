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
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;

/// What a package calls it, and what it calls itself.
pub const NAME: &str = "Std";

/// Its modules, by their paths in it (`Collections.List` is the file
/// `src/Collections/List.idl`), the root first.
pub const MODULES: &[(&str, &str)] = &[
    ("", include_str!("../../../lib/Std/src/Lib.idl")),
    ("Bench", include_str!("../../../lib/Std/src/Bench.idl")),
    ("Bool", include_str!("../../../lib/Std/src/Bool.idl")),
    ("Bytes", include_str!("../../../lib/Std/src/Bytes.idl")),
    ("Char", include_str!("../../../lib/Std/src/Char.idl")),
    ("Cmp", include_str!("../../../lib/Std/src/Cmp.idl")),
    ("Collections.Array", include_str!("../../../lib/Std/src/Collections/Array.idl")),
    ("Collections.HashMap", include_str!("../../../lib/Std/src/Collections/HashMap.idl")),
    ("Collections.HashTable", include_str!("../../../lib/Std/src/Collections/HashTable.idl")),
    ("Collections.List", include_str!("../../../lib/Std/src/Collections/List.idl")),
    ("Collections.Map", include_str!("../../../lib/Std/src/Collections/Map.idl")),
    ("Collections.MArray", include_str!("../../../lib/Std/src/Collections/MArray.idl")),
    ("Collections.Set", include_str!("../../../lib/Std/src/Collections/Set.idl")),
    ("Collections.Tree", include_str!("../../../lib/Std/src/Collections/Tree.idl")),
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
    ("Log", include_str!("../../../lib/Std/src/Log.idl")),
    ("Maybe", include_str!("../../../lib/Std/src/Maybe.idl")),
    ("Num.Bits", include_str!("../../../lib/Std/src/Num/Bits.idl")),
    ("Num.Fin", include_str!("../../../lib/Std/src/Num/Fin.idl")),
    ("Num.Float", include_str!("../../../lib/Std/src/Num/Float.idl")),
    ("Num.Int", include_str!("../../../lib/Std/src/Num/Int.idl")),
    ("Num.Nat", include_str!("../../../lib/Std/src/Num/Nat.idl")),
    ("Ordering", include_str!("../../../lib/Std/src/Ordering.idl")),
    ("Path", include_str!("../../../lib/Std/src/Path.idl")),
    ("Process", include_str!("../../../lib/Std/src/Process.idl")),
    ("Random", include_str!("../../../lib/Std/src/Random.idl")),
    ("Ref", include_str!("../../../lib/Std/src/Ref.idl")),
    ("Result", include_str!("../../../lib/Std/src/Result.idl")),
    ("Sigma", include_str!("../../../lib/Std/src/Sigma.idl")),
    ("Sort", include_str!("../../../lib/Std/src/Sort.idl")),
    ("St", include_str!("../../../lib/Std/src/St.idl")),
    ("State", include_str!("../../../lib/Std/src/State.idl")),
    ("Stream", include_str!("../../../lib/Std/src/Stream.idl")),
    ("String", include_str!("../../../lib/Std/src/String.idl")),
    ("String.Parse", include_str!("../../../lib/Std/src/String/Parse.idl")),
    ("String.Parse.Char", include_str!("../../../lib/Std/src/String/Parse/Char.idl")),
    ("String.Parse.Lexer", include_str!("../../../lib/Std/src/String/Parse/Lexer.idl")),
    ("Test", include_str!("../../../lib/Std/src/Test.idl")),
    ("Test.Fuzz", include_str!("../../../lib/Std/src/Test/Fuzz.idl")),
    ("Test.Property", include_str!("../../../lib/Std/src/Test/Property.idl")),
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

/// Std, compiled: once a process, and read back once a thread; or, where
/// [`keep_in`] has said, read from what an earlier run of this binary kept.
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
                let kept = KEPT.get();
                if let Some(bytes) = kept.and_then(|file| read(file, binary())) {
                    return bytes;
                }
                let package = compile().unwrap_or_else(|errors| panic!("Std does not compile:\n{errors}"));
                let bytes = postcard::to_allocvec(&package).expect("a compiled package serializes");
                if let Some(file) = kept {
                    write(file, binary(), &bytes);
                }
                bytes
            });
            Rc::new(postcard::from_bytes(bytes).expect("Std reads back as it was written"))
        })
        .clone()
    })
}

/// Where compiled Std is kept between runs, if anywhere: see [`keep_in`].
static KEPT: OnceLock<PathBuf> = OnceLock::new();

/// Keep compiled Std in `dir` between runs, and read it from there: a file
/// for each binary that keeps one, stamped with that binary, so that a
/// rebuilt compiler never reads what another wrote. Checking Std takes
/// seconds, which every run of `idyll` and `idyllic` paid before it did
/// anything else; they say where (`$IDYLL_HOME/cache`). The tests do not,
/// and check it once a process. A file that cannot be read or written is
/// simply compiled over.
pub fn keep_in(dir: &Path) {
    let mut h = std::hash::DefaultHasher::new();
    std::env::current_exe().unwrap_or_default().hash(&mut h);
    let _ = KEPT.set(dir.join(format!("std-{:016x}.ipk", h.finish())));
}

/// Where Idyll keeps what runs and projects share: `$IDYLL_HOME`, or
/// `~/.idyll`.
pub fn home() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("IDYLL_HOME") {
        return Some(PathBuf::from(dir));
    }
    let base = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok()?;
    Some(PathBuf::from(base).join(".idyll"))
}

/// What a kept Std starts with, so that some other file is not taken for one.
const MAGIC: &[u8; 8] = b"IDLSTD\x00\x01";

/// This binary, by version, size and when it was built: what a kept Std was
/// compiled by, Std's sources and the prelude being in it.
fn binary() -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    env!("CARGO_PKG_VERSION").hash(&mut h);
    if let Ok(meta) = std::env::current_exe().and_then(std::fs::metadata) {
        meta.len().hash(&mut h);
        if let Ok(built) = meta.modified() {
            built.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().hash(&mut h);
        }
    }
    h.finish()
}

/// The compiled Std kept at `file`, if `stamp` kept it and it reads back.
fn read(file: &Path, stamp: u64) -> Option<Vec<u8>> {
    let bytes = std::fs::read(file).ok()?;
    let rest = bytes.strip_prefix(MAGIC)?;
    let (kept, payload) = rest.split_first_chunk::<8>()?;
    if u64::from_le_bytes(*kept) != stamp {
        return None;
    }
    postcard::from_bytes::<CompiledPackage>(payload).ok()?;
    Some(payload.to_vec())
}

fn write(file: &Path, stamp: u64, payload: &[u8]) {
    let mut bytes = Vec::with_capacity(MAGIC.len() + 8 + payload.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&stamp.to_le_bytes());
    bytes.extend_from_slice(payload);
    if file.parent().is_some_and(|dir| std::fs::create_dir_all(dir).is_err()) {
        return;
    }
    // (written aside and renamed into place, so that two runs at once never
    // read half a file)
    let partial = file.with_extension(format!("ipk.{}", std::process::id()));
    if std::fs::write(&partial, &bytes).is_err() || std::fs::rename(&partial, file).is_err() {
        let _ = std::fs::remove_file(&partial);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_std_is_read_back_only_by_the_binary_that_kept_it() {
        let dir = std::env::temp_dir().join(format!("idyll-kept-std-{}", std::process::id()));
        let file = dir.join("std.ipk");
        let payload = postcard::to_allocvec(&*compiled()).expect("Std serializes");
        write(&file, 7, &payload);
        assert_eq!(read(&file, 7).as_deref(), Some(&payload[..]), "what was kept is what is read");
        // another binary's, or a rebuilt one's
        assert_eq!(read(&file, 8), None);
        // and a file that is not one, or is cut short, is compiled over
        std::fs::write(&file, b"not Std").expect("writes");
        assert_eq!(read(&file, 7), None);
        write(&file, 7, &payload[..payload.len() / 2]);
        assert_eq!(read(&file, 7), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
