//! The Idyll compiler: a small Calculus-of-Constructions style dependently
//! typed language.
//!
//! Each pass is a crate of its own: [`idyllic_lexer`] → [`idyllic_layout`] →
//! [`idyllic_parser`] → [`idyllic_rename`] → [`idyllic_scc`] → [`idyllic_typecheck`],
//! and then, for a program that checks, [`idyllic_cut`], which lowers it to
//! Cut, the IR Meadow's runtimes take: [`meadow`] runs it on Glade or Silo.
//! Idyll has no runtime of its own.  Every pass reports through
//! [`idyllic_diagnostics`].  This crate strings them together: [`pipeline`]
//! runs the passes, and `main.rs` is the `idyllic` command.  The REPL is not
//! here: it is the `idyll` command, in idyll-driver/.

pub mod inspect;
pub mod meadow;
pub mod parts;
pub mod pipeline;
pub mod stdlib;
