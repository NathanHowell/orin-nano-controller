//! REPL tooling shared between firmware and emulator targets.
//!
//! The REPL grammar lives in [`grammar`] and is implemented with a
//! token/parse pipeline that stays compatible with `no_std`.

pub mod completion;
pub mod commands;
pub mod grammar;
