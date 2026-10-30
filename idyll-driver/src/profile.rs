//! Build **profiles** — the driver-level names for how a program is built
//! and run.
//!
//! A profile is what a user selects (`idyll build --release`). Three layers,
//! each overriding the one before it:
//!
//! 1. the profile's built-in meaning — [`Profile::backend`];
//! 2. the package's `[profile.<name>]` section, if it has a manifest -- or its
//!    workspace root's, if it is in a workspace;
//! 3. flags on the command line.
//!
//! [`Resolved`] is what that produces. The built-in layer is what makes a
//! manifest optional, and the manifest layer is what lets a package say
//! `backend = "silo"` in debug builds without every `idyll run` needing a
//! flag.
//!
//! Checking a package does not depend on the profile: the same compiled
//! package serves both, and only how the program runs differs.

use crate::package::{Manifest, ProfileConfig};
use std::path::Path;

/// Which of Meadow's runtimes runs a built program: Idyll lowers it to Cut
/// and hands it to `meadow cut`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backend {
    /// Glade, Meadow's bytecode: starts at once.
    #[default]
    Glade,
    /// Silo, native code, built into an executable first.
    Silo,
}

impl Backend {
    pub fn parse(s: &str) -> Option<Backend> {
        match s {
            "glade" => Some(Backend::Glade),
            "silo" => Some(Backend::Silo),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Backend::Glade => "glade",
            Backend::Silo => "silo",
        }
    }

    /// The runtime, as the compiler names it.
    pub const fn runtime(self) -> idyllic::meadow::Runtime {
        match self {
            Backend::Glade => idyllic::meadow::Runtime::Glade,
            Backend::Silo => idyllic::meadow::Runtime::Silo,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    /// The edit-run loop: start at once.
    #[default]
    Debug,
    /// Shipping: native code.
    Release,
}

impl Profile {
    pub const fn name(self) -> &'static str {
        match self {
            Profile::Debug => "debug",
            Profile::Release => "release",
        }
    }

    /// How this profile runs a program before a manifest or a flag says
    /// otherwise: debug on Glade, which starts at once; release as an
    /// executable Silo builds.
    pub const fn backend(self) -> Backend {
        match self {
            Profile::Debug => Backend::Glade,
            Profile::Release => Backend::Silo,
        }
    }
}

/// A profile plus the overrides that apply to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Resolved {
    pub profile: Profile,
    pub backend: Backend,
    /// Whether a flag or the manifest named `backend`, rather than the profile
    /// supplying it. A backend nobody asked for gives way when it cannot be
    /// had -- see [`Resolved::fallback`].
    pub backend_named: bool,
}

impl Resolved {
    /// Just the built-in meaning of `profile`.
    pub const fn new(profile: Profile) -> Resolved {
        Resolved { profile, backend: profile.backend(), backend_named: false }
    }

    /// Layer the package manifest at `path` over the profile, then `flags` over
    /// that.
    ///
    /// `path` is whatever the user named on the command line — a package
    /// directory or a single `.idl` file. A package with no manifest, or one
    /// that says nothing about this profile, simply keeps the built-in meaning.
    ///
    /// In a workspace the manifest is the workspace root's, whichever member
    /// `path` names: every member builds with the same profiles.
    pub fn resolve(profile: Profile, path: &Path, flags: ProfileConfig) -> Resolved {
        let from_manifest = crate::workspace::Workspace::find(path)
            .ok()
            .flatten()
            .map(|ws| ws.manifest)
            .or_else(|| Manifest::find(path))
            .map(|m| m.profile(profile.name()))
            .unwrap_or_default();
        let named = flags.backend.or(from_manifest.backend);
        Resolved { profile, backend: named.unwrap_or(profile.backend()), backend_named: named.is_some() }
    }

    /// What runs the program when `backend` cannot -- Silo, where it cannot
    /// build an executable: Glade, if the backend was only the profile's
    /// default, and nothing, if someone asked for it.
    pub fn fallback(self) -> Option<Resolved> {
        (self.backend == Backend::Silo && !self.backend_named).then_some(Resolved { backend: Backend::Glade, ..self })
    }

    /// What runs the program, as a build reports it.
    pub const fn how(self) -> &'static str {
        self.backend.name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flag_beats_the_manifest_beats_the_profile() {
        let dir = std::env::temp_dir().join(format!("idyll-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Idyll.toml"), "[package]\nname = \"P\"\n\n[profile.release]\nbackend = \"glade\"\n").unwrap();
        assert_eq!(Resolved::resolve(Profile::Debug, &dir, ProfileConfig::default()).backend, Backend::Glade);
        let release = Resolved::resolve(Profile::Release, &dir, ProfileConfig::default());
        assert_eq!(release.backend, Backend::Glade);
        assert!(release.backend_named);
        let flag = ProfileConfig { backend: Some(Backend::Silo) };
        assert_eq!(Resolved::resolve(Profile::Release, &dir, flag).backend, Backend::Silo);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_a_default_gives_way() {
        assert_eq!(Resolved::new(Profile::Release).fallback().map(|r| r.backend), Some(Backend::Glade));
        let named = Resolved { backend_named: true, ..Resolved::new(Profile::Release) };
        assert_eq!(named.fallback(), None);
        assert_eq!(Resolved::new(Profile::Debug).fallback(), None);
    }
}
