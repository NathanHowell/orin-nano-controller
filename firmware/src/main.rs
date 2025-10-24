#![no_std]
#![no_main]

extern crate panic_halt;

use embassy_executor::Spawner;

mod straps;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {}
