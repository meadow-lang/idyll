//! The Idyll **build system**, as Meadow's: filesystem package discovery
//! ([`package`], [`workspace`]), dependencies fetched from git and pinned in
//! `idyll.lock` ([`git`], [`lock`], [`semver`]), the package graph compiled in
//! order with each unchanged package read back from the last build
//! ([`pipeline`], [`incremental`]), and programs run on the AxCut machine or
//! compiled to native code ([`native`]).
//!
//! It sits on top of the compiler (compiler/, `idyllic`), which checks one
//! package at a time against the packages it depends on, compiled. The `idyll`
//! binary -- the CLI and the REPL -- is a thin shell over this crate.

pub mod add;
pub mod artifacts;
pub mod clean;
pub mod git;
pub mod incremental;
pub mod init;
pub mod lock;
pub mod native;
pub mod package;
pub mod pipeline;
pub mod profile;
pub mod semver;
pub mod status;
pub mod update;
pub mod workspace;

pub use profile::{Backend, Profile, Resolved};
