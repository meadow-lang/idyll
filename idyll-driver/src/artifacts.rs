//! **Where a build puts what it makes.**
//!
//! Everything a build writes goes under the package's own `target` directory,
//! beside its `Idyll.toml` (or its workspace's), one directory per profile:
//!
//! ```text
//!   <package>/target/release/native/<name>(.exe)   the executable
//!   <package>/target/debug/cut/<name>.cut          the Cut (`--emit cut`)
//!   <package>/target/debug/incremental/            compiled packages, to reuse
//!   <package>/target/release/...
//! ```
//!
//! One place, so that cleaning is deleting it and ignoring it is one line --
//! which `idyll init` writes (see [`crate::init`]). Nothing in it is a source
//! of truth: every file can be made again from the package.

use crate::profile::Profile;
use std::path::{Path, PathBuf};

/// The directory's name, under a package root.
pub const TARGET: &str = "target";

/// `<root>/target/<profile>`.
pub fn profile_dir(root: &Path, profile: Profile) -> PathBuf {
    root.join(TARGET).join(profile.name())
}

/// Where native code and executables go.
pub fn native_dir(root: &Path, profile: Profile) -> PathBuf {
    profile_dir(root, profile).join("native")
}

/// Where package `name`'s executable goes.
pub fn exe_path(root: &Path, profile: Profile, name: &str) -> PathBuf {
    native_dir(root, profile).join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// Where package `name`'s Cut goes as text -- `--emit cut`.
pub fn cut_path(root: &Path, profile: Profile, name: &str) -> PathBuf {
    profile_dir(root, profile).join("cut").join(format!("{name}.cut"))
}

/// Where compiled packages are kept for the next build to reuse -- see
/// [`crate::incremental`].
pub fn incremental_dir(root: &Path, profile: Profile) -> PathBuf {
    profile_dir(root, profile).join("incremental")
}

/// Write `text` to `path`, making the directories it needs.
pub fn write_text(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("could not write {}: {e}", path.display()))
}
