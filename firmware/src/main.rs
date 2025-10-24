#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(target_os = "none")]
extern crate panic_halt;

#[cfg(target_os = "none")]
use embassy_executor::Spawner;

mod bridge;
mod repl;
mod straps;
mod telemetry;

#[cfg(target_os = "none")]
#[embassy_executor::main]
async fn main(_spawner: Spawner) {}

#[cfg(not(target_os = "none"))]
fn main() {}
