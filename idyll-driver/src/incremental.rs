//! **Incremental compilation**: a package whose inputs have not changed is
//! read back from the last build instead of compiled again.
//!
//! The package is the compilation unit -- its modules are renamed and checked
//! together -- so it is also the unit of reuse. After a package
//! compiles cleanly its [`CompiledPackage`] is written under the `target`
//! directory, beside a **fingerprint** of everything that went into it:
//!
//! - the compiler itself (this binary, by version, size and modification time,
//!   which also covers the prelude embedded in it);
//! - the package's name and every module: its path, its file and its text;
//! - the first unique it may name things with (see below);
//! - its dependencies: the names it calls them, and their fingerprints, in
//!   order.
//!
//! A dependency's fingerprint is part of its dependents', so a change reaches
//! exactly the packages downstream of it and no others: editing `app` in a
//! workspace recompiles `app`, and editing `util` recompiles `util` and what
//! uses it -- but not a member beside it that does not.
//!
//! **Uniques.** Every package in a build names its things with uniques above
//! those of every package compiled before it, so that packages linked
//! together never share one (see `pipeline::compile_graph`). That makes a
//! package's uniques depend on the sizes of unrelated packages ahead of it in
//! the build order, which would make any edit anywhere a change to everything
//! after it. So each package starts on a boundary of [`ID_CHUNK`]: a package
//! growing within its chunk moves nobody else.
//!
//! The prelude is not kept here: it is checked with every package, and named
//! the same in each.
//!
//! What is written is only ever a package that compiled without a diagnostic,
//! against dependencies that did too, so that a build with errors reports them
//! every time. A file that cannot be read, is from another compiler or does
//! not match is simply compiled over.

use crate::package::Package;
use crate::profile::Profile;
use idyllic::pipeline::CompiledPackage;
use std::hash::Hasher;
use std::path::{Path, PathBuf};

/// Uniques per package slot -- see the module docs.
pub const ID_CHUNK: usize = 1 << 16;

/// `floor` rounded up to the next package slot.
pub fn align(floor: usize) -> usize {
    floor.div_ceil(ID_CHUNK) * ID_CHUNK
}

/// What a saved package starts with, so that some other file is not taken for
/// one.
const MAGIC: &[u8; 8] = b"IDLPKG\x00\x01";

/// Where the compiled packages of one build configuration live:
/// `target/<profile>/incremental`.
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    /// The cache for builds in `profile` whose `target` is under `root`;
    /// none with `IDYLL_INCREMENTAL=0`.
    pub fn new(root: &Path, profile: Profile) -> Option<Cache> {
        if !enabled() {
            return None;
        }
        let dir = crate::artifacts::incremental_dir(root, profile);
        prune(&dir);
        Some(Cache { dir })
    }

    /// The package known as `ident` saved with `fingerprint`, if there is
    /// one.  (Known by who it is, `Json@1.2.0`, so that two versions of one
    /// package in a build keep a file each.)
    pub fn load(&self, ident: &str, fingerprint: u64) -> Option<CompiledPackage> {
        read(&self.dir.join(format!("{ident}.ipk")), fingerprint)
    }

    /// Save `package` under `fingerprint`. Best effort: a build that cannot
    /// write its cache has still built.
    pub fn store(&self, ident: &str, package: &CompiledPackage, fingerprint: u64) {
        write(&self.dir.join(format!("{ident}.ipk")), package, fingerprint);
    }
}

/// The parts of one package (see `idyllic::parts`) kept beside it, a file
/// each, `<package>.part-<which>.ipk`: what lets a build of a package whose
/// sources changed check only the parts the change reaches.
pub struct Parts<'a> {
    pub cache: &'a Cache,
    /// the package, as [`Cache::load`] knows it
    pub ident: String,
    /// see [`parts_salt`]
    pub salt: u64,
}

impl idyllic::pipeline::PartStore for Parts<'_> {
    fn salt(&self) -> u64 {
        self.salt
    }

    fn load(&self, id: &str, key: u64) -> Option<CompiledPackage> {
        read(&self.part_file(id), key)
    }

    fn store(&self, id: &str, key: u64, part: &CompiledPackage) {
        write(&self.part_file(id), part, key);
    }
}

impl Parts<'_> {
    fn part_file(&self, id: &str) -> PathBuf {
        let mut h = hasher();
        text(&mut h, id);
        self.cache.dir.join(format!("{}.part-{:016x}.ipk", self.ident, h.finish()))
    }
}

/// What a package's parts are compiled from besides their own modules: the
/// compiler, where the package starts naming things, its name, and its
/// dependencies' fingerprints.
pub fn parts_salt(package: &Package, deps: &[u64], floor: usize) -> u64 {
    let mut h = hasher();
    h.write_u64(compiler());
    h.write_usize(floor);
    text(&mut h, &package.name);
    for (d, name) in deps.iter().zip(&package.dep_names) {
        h.write_u64(*d);
        text(&mut h, name);
    }
    h.finish()
}

/// Remove from `dir` what an earlier build of the compiler wrote: a package
/// from before this binary existed was written by another one, and is never
/// read again -- its fingerprint names that compiler (see [`compiler`]).
/// Without this, the file of a package no build has any more would stay for
/// good. Once per directory per process.
fn prune(dir: &Path) {
    static DONE: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());
    {
        let mut done = DONE.lock().unwrap_or_else(|p| p.into_inner());
        if done.iter().any(|d| d == dir) {
            return;
        }
        done.push(dir.to_path_buf());
    }
    let Some(built) = std::env::current_exe()
        .and_then(std::fs::metadata)
        .and_then(|m| m.modified())
        .ok()
    else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let ours = path
            .extension()
            .is_some_and(|e| e == "ipk");
        let older = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t < built);
        if ours && older {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// The package saved at `path`, if it was saved with `fingerprint`.
fn read(path: &Path, fingerprint: u64) -> Option<CompiledPackage> {
    let bytes = std::fs::read(path).ok()?;
    let rest = bytes.strip_prefix(MAGIC)?;
    let (saved, payload) = rest.split_first_chunk::<8>()?;
    if u64::from_le_bytes(*saved) != fingerprint {
        return None;
    }
    postcard::from_bytes(payload).ok()
}

fn write(path: &Path, package: &CompiledPackage, fingerprint: u64) {
    let Ok(payload) = postcard::to_stdvec(package) else {
        return;
    };
    let mut bytes = Vec::with_capacity(MAGIC.len() + 8 + payload.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&fingerprint.to_le_bytes());
    bytes.extend_from_slice(&payload);
    if let Some(dir) = path.parent()
        && std::fs::create_dir_all(dir).is_err()
    {
        return;
    }
    // Written aside and renamed into place, so that two builds at once never
    // read half a file.
    let partial = path.with_extension(format!("ipk.{}", std::process::id()));
    if std::fs::write(&partial, &bytes).is_err() || std::fs::rename(&partial, path).is_err() {
        let _ = std::fs::remove_file(&partial);
    }
}

/// Everything compiling `package` depends on -- see the module docs.
pub fn fingerprint(package: &Package, deps: &[u64], floor: usize) -> u64 {
    let mut h = hasher();
    h.write_u64(compiler());
    h.write_usize(floor);
    text(&mut h, &package.name);
    h.write_usize(package.modules.len());
    for m in &package.modules {
        h.write_usize(m.path.len());
        for seg in &m.path {
            text(&mut h, seg);
        }
        text(&mut h, &m.name);
        text(&mut h, &m.file);
        text(&mut h, &m.text);
    }
    h.write_usize(deps.len());
    for (d, name) in deps.iter().zip(&package.dep_names) {
        h.write_u64(*d);
        text(&mut h, name);
    }
    h.finish()
}

/// Whether builds reuse what earlier ones compiled: unless
/// `IDYLL_INCREMENTAL=0`, which is for ruling the cache out when something
/// looks wrong.
pub fn enabled() -> bool {
    std::env::var("IDYLL_INCREMENTAL").map_or(true, |v| v != "0")
}

/// A hasher that answers the same in every process of one binary. Nothing
/// promises that across Rust releases, but the binary is in every
/// fingerprint anyway.
fn hasher() -> std::hash::DefaultHasher {
    std::hash::DefaultHasher::new()
}

/// `s`, with its length, so that two strings cannot run together into one.
fn text(h: &mut impl Hasher, s: &str) {
    h.write_usize(s.len());
    h.write(s.as_bytes());
}

/// This compiler: a rebuilt `idyll` never reads what an older one wrote.
fn compiler() -> u64 {
    static ID: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *ID.get_or_init(|| {
        let mut h = hasher();
        text(&mut h, env!("CARGO_PKG_VERSION"));
        let exe = std::env::current_exe().and_then(std::fs::metadata);
        match exe {
            Ok(meta) => {
                h.write_u64(meta.len());
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
                h.write_u128(modified.map_or(0, |d| d.as_nanos()));
            }
            // Nothing to tell one build of the compiler from another: a
            // fingerprint no file will ever match.
            Err(_) => h.write_u128(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos()),
            ),
        }
        h.finish()
    })
}
