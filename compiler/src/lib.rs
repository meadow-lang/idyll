//! The Idyll compiler: a small Calculus-of-Constructions style dependently
//! typed language.
//!
//! The pipeline is [`lexer`] → [`layout`] → [`parser`] → [`rename`] →
//! [`scc`] → [`typecheck`], with every pass reporting through
//! [`diagnostics`].  [`pipeline`] strings them together.

pub mod ast;
pub mod axcut;
pub mod builtins;
pub mod diagnostics;
pub mod grade;
pub mod hir;
pub mod layout;
pub mod lexer;
pub mod parser;
pub mod pipeline;
pub mod rename;
pub mod scc;
pub mod span;
pub mod token;
pub mod typecheck;
