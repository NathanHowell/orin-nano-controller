#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(target_os = "none")]
extern crate panic_halt;

mod bridge;
mod repl;
mod straps;
mod telemetry;
mod usb;

#[cfg(target_os = "none")]
use embassy_executor::Spawner;
#[cfg(target_os = "none")]
use embassy_stm32 as hal;
#[cfg(target_os = "none")]
use embassy_sync::channel::Channel;
#[cfg(target_os = "none")]
use embassy_time::{Duration, Timer};

#[cfg(target_os = "none")]
use crate::bridge::{BridgeActivityBus, BridgeQueue};
#[cfg(target_os = "none")]
use crate::repl::ReplSession;
#[cfg(target_os = "none")]
use crate::straps::CommandProducer;
#[cfg(target_os = "none")]
use crate::straps::orchestrator::{HardwareStrapDriver, NoopPowerMonitor, StrapOrchestrator};
#[cfg(target_os = "none")]
use crate::telemetry::TelemetryRecorder;
#[cfg(target_os = "none")]
use controller_core::orchestrator::SequenceScheduler;
#[cfg(target_os = "none")]
use controller_core::repl::commands::CommandExecutor;
#[cfg(target_os = "none")]
use controller_core::sequences::{recovery_entry_template, recovery_immediate_template};
#[cfg(target_os = "none")]
use embassy_stm32::gpio::{Level, OutputOpenDrain, Speed};

#[cfg(target_os = "none")]
static COMMAND_QUEUE: straps::CommandQueue = Channel::new();
#[cfg(target_os = "none")]
static BRIDGE_QUEUE: BridgeQueue = BridgeQueue::new();
#[cfg(target_os = "none")]
static BRIDGE_ACTIVITY: BridgeActivityBus = BridgeActivityBus::new();

#[cfg(target_os = "none")]
#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let config = hal::Config::default();
    let hal::Peripherals {
        PA2, PA3, PA4, PA5, ..
    } = hal::init(config);

    let strap_driver = HardwareStrapDriver::new(
        OutputOpenDrain::new(PA4, Level::High, Speed::Low),
        OutputOpenDrain::new(PA3, Level::High, Speed::Low),
        OutputOpenDrain::new(PA2, Level::High, Speed::Low),
        OutputOpenDrain::new(PA5, Level::High, Speed::Low),
    );

    let command_receiver = COMMAND_QUEUE.receiver();
    let orchestrator =
        StrapOrchestrator::with_components(command_receiver, NoopPowerMonitor::new(), strap_driver);
    let telemetry = TelemetryRecorder::new();

    spawner
        .spawn(strap_task(orchestrator, telemetry))
        .expect("failed to spawn strap orchestrator task");

    spawner.spawn(usb_task()).expect("failed to spawn USB task");

    spawner
        .spawn(repl_task())
        .expect("failed to spawn REPL task");
    spawner
        .spawn(bridge_task(&BRIDGE_QUEUE, &BRIDGE_ACTIVITY))
        .expect("failed to spawn bridge task");

    core::future::pending::<()>().await;
}

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(target_os = "none")]
#[embassy_executor::task]
async fn strap_task(
    orchestrator: StrapOrchestrator<'static, NoopPowerMonitor, HardwareStrapDriver<'static>>,
    mut telemetry: TelemetryRecorder,
) -> ! {
    orchestrator.run(&mut telemetry).await;
}

#[cfg(target_os = "none")]
#[embassy_executor::task]
async fn repl_task() -> ! {
    let command_sender = COMMAND_QUEUE.sender();
    let producer = CommandProducer::new(command_sender);
    let mut scheduler = SequenceScheduler::new(producer);

    {
        let templates = scheduler.templates_mut();
        templates
            .register(recovery_entry_template())
            .expect("register recovery entry template");
        templates
            .register(recovery_immediate_template())
            .expect("register recovery immediate template");
    }

    let executor = CommandExecutor::new(scheduler);
    let mut session = ReplSession::new(executor);
    session.run().await;
}

#[cfg(target_os = "none")]
#[embassy_executor::task]
async fn bridge_task(_queue: &'static BridgeQueue, _activity: &'static BridgeActivityBus) -> ! {
    loop {
        Timer::after(Duration::from_secs(1)).await;
    }
}

#[cfg(target_os = "none")]
#[embassy_executor::task]
async fn usb_task() -> ! {
    loop {
        Timer::after(Duration::from_secs(1)).await;
    }
}
