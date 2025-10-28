use core::mem::MaybeUninit;

use cortex_m::interrupt;
use cortex_m::register::primask;
use critical_section::{self, RawRestoreState};
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32 as hal;
use embassy_stm32::gpio::{Level, OutputOpenDrain, Speed};
use embassy_sync::channel::Channel;

use crate::bridge::{BridgeActivityBus, BridgeQueue};
use crate::straps;
use crate::straps::orchestrator::{HardwareStrapDriver, NoopPowerMonitor, StrapOrchestrator};
use crate::telemetry::TelemetryRecorder;
use crate::usb;
use controller_core::orchestrator::register_default_templates;

mod bridge_task;
mod repl_task;
mod strap_task;
mod usb_task;

critical_section::set_impl!(InterruptCriticalSection);

struct InterruptCriticalSection;

unsafe impl critical_section::Impl for InterruptCriticalSection {
    unsafe fn acquire() -> RawRestoreState {
        let primask = primask::read();
        interrupt::disable();
        primask.is_active()
    }

    unsafe fn release(restore_state: RawRestoreState) {
        if restore_state {
            unsafe {
                interrupt::enable();
            }
        }
    }
}

pub(super) static COMMAND_QUEUE: straps::CommandQueue = Channel::new();
pub(super) static BRIDGE_QUEUE: BridgeQueue = BridgeQueue::new();
pub(super) static BRIDGE_ACTIVITY: BridgeActivityBus = BridgeActivityBus::new();
pub(super) static mut USB_STORAGE: MaybeUninit<usb::UsbDeviceStorage> = MaybeUninit::uninit();

#[embassy_executor::main]
pub async fn main(spawner: Spawner) {
    let config = hal::Config::default();
    let hal::Peripherals {
        PA2,
        PA3,
        PA4,
        PA5,
        PB0,
        PB1,
        USB,
        PA11,
        PA12,
        USART5,
        ..
    } = hal::init(config);

    let strap_driver = HardwareStrapDriver::new(
        OutputOpenDrain::new(PA4, Level::High, Speed::Low),
        OutputOpenDrain::new(PA3, Level::High, Speed::Low),
        OutputOpenDrain::new(PA2, Level::High, Speed::Low),
        OutputOpenDrain::new(PA5, Level::High, Speed::Low),
    );

    let command_receiver = COMMAND_QUEUE.receiver();
    let mut orchestrator =
        StrapOrchestrator::with_components(command_receiver, NoopPowerMonitor::new(), strap_driver);
    {
        let registry = orchestrator.templates_mut();
        register_default_templates(registry).expect("strap template registration");
    }

    let telemetry = TelemetryRecorder::new();

    spawner
        .spawn(strap_task::run(orchestrator, telemetry))
        .expect("failed to spawn strap orchestrator task");

    spawner
        .spawn(usb_task::run(USB, PA12, PA11))
        .expect("failed to spawn USB task");

    spawner
        .spawn(repl_task::run())
        .expect("failed to spawn REPL task");
    spawner
        .spawn(bridge_task::run(
            &BRIDGE_QUEUE,
            &BRIDGE_ACTIVITY,
            USART5,
            PB0,
            PB1,
        ))
        .expect("failed to spawn bridge task");

    core::future::pending::<()>().await;
}
