//! The Idyll **build system**, as Meadow's: filesystem package discovery
//! ([`package`], [`workspace`]), dependencies fetched from git and pinned in
//! `idyll.lock` ([`git`], [`lock`], [`semver`]), the package graph compiled in
//! order with each unchanged package read back from the last build
//! ([`pipeline`], [`incremental`]), and programs lowered to Cut and run on
//! Meadow's runtimes: Glade, or Silo for native code ([`native`]).
//!
//! The REPL's searching is here too: [`find`] (by name, or by type, as
//! Hoogle does), [`prove`] (a term of a type), and the pop-ups that show
//! them and the compiler's IRs ([`finder`], [`viewer`]).
//!
//! It sits on top of the compiler (compiler/, `idyllic`), which checks one
//! package at a time against the packages it depends on, compiled. The `idyll`
//! binary -- the CLI and the REPL -- is a thin shell over this crate.

pub mod add;
pub mod artifacts;
pub mod clean;
pub mod complete;
pub mod find;
pub mod finder;
pub mod git;
pub mod incremental;
pub mod init;
pub mod lock;
pub mod native;
pub mod package;
pub mod pipeline;
pub mod profile;
pub mod prove;
pub mod semver;
pub mod status;
pub mod update;
pub mod viewer;
pub mod workspace;

pub use profile::{Backend, Profile, Resolved};
