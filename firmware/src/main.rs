#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![cfg_attr(target_os = "none", allow(static_mut_refs))]

#[cfg(target_os = "none")]
extern crate panic_halt;

mod bridge;
mod repl;
mod straps;
mod telemetry;
mod usb;

#[cfg(target_os = "none")]
mod runtime;

#[cfg(not(target_os = "none"))]
fn main() {}
