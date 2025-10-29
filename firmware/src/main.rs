#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![cfg_attr(target_os = "none", allow(static_mut_refs))]

mod bridge;
mod hw;
mod repl;
mod status;
mod straps;
mod telemetry;
mod usb;

#[cfg(target_os = "none")]
mod runtime;

#[cfg(target_os = "none")]
mod panic;

#[cfg(not(target_os = "none"))]
fn main() {}
