//! The Idyll compiler: a small Calculus-of-Constructions style dependently
//! typed language.
//!
//! Each pass is a crate of its own: [`idyllic_lexer`] → [`idyllic_layout`] →
//! [`idyllic_parser`] → [`idyllic_rename`] → [`idyllic_scc`] → [`idyllic_typecheck`],
//! and then, for a program that checks, [`idyllic_seq`] (lowering and the
//! abstract machine) and [`idyllic_llvm`] (native code).  Every pass reports
//! through [`idyllic_diagnostics`].  This crate is the driver: [`pipeline`]
//! strings the passes together, and `main.rs` is the `idyllic` command.

pub mod pipeline;
