#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

// Shared logic for the Orin controller feature set.
//
// This crate stays portable across MCU firmware and host tooling by avoiding the
// Rust standard library and exposing abstractions the other crates can adopt.
