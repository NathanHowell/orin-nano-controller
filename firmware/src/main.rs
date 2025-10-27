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
use core::mem::MaybeUninit;
#[cfg(target_os = "none")]
use embassy_executor::Spawner;
#[cfg(target_os = "none")]
use embassy_futures::join::join3;
#[cfg(target_os = "none")]
use embassy_futures::select::{Either3, select3};
#[cfg(target_os = "none")]
use embassy_stm32 as hal;
#[cfg(target_os = "none")]
use embassy_sync::channel::Channel;
#[cfg(target_os = "none")]
use embassy_time::{Duration, Instant, Timer};
#[cfg(target_os = "none")]
use embassy_usb::driver::EndpointError;

#[cfg(target_os = "none")]
use crate::bridge::{
    BridgeActivityBus, BridgeActivityEvent, BridgeActivityKind, BridgeFrame, BridgeQueue,
};
#[cfg(target_os = "none")]
use crate::repl::{REPL_RX_QUEUE, REPL_TX_QUEUE, ReplFrame, ReplSession};
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
embassy_stm32::bind_interrupts!(struct UsbIrqs {
    USB => embassy_stm32::usb::InterruptHandler<hal::peripherals::USB>,
});

#[cfg(target_os = "none")]
static mut USB_STORAGE: MaybeUninit<usb::UsbDeviceStorage> = MaybeUninit::uninit();
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
        PA2,
        PA3,
        PA4,
        PA5,
        USB,
        PA11,
        PA12,
        ..
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

    spawner
        .spawn(usb_task(USB, PA12, PA11))
        .expect("failed to spawn USB task");

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
async fn usb_task(
    usb: hal::peripherals::USB,
    dp: hal::peripherals::PA12,
    dm: hal::peripherals::PA11,
) -> ! {
    let storage = unsafe { USB_STORAGE.write(usb::UsbDeviceStorage::new()) };
    let driver = embassy_stm32::usb::Driver::new(usb, UsbIrqs, dp, dm);

    let mut composite = usb::UsbComposite::new(driver, storage, usb::UsbDeviceStrings::default());

    let mut device = composite.device;

    let usb::CdcAcmHandle {
        sender: mut repl_sender,
        receiver: mut repl_receiver,
        control: repl_control,
        ..
    } = composite
        .take_repl()
        .expect("REPL CDC interface unavailable");

    let usb::CdcAcmHandle {
        sender: mut bridge_sender,
        receiver: mut bridge_receiver,
        control: bridge_control,
        ..
    } = composite
        .take_bridge()
        .expect("bridge CDC interface unavailable");

    let repl_future = async move {
        let mut repl_rx_queue = REPL_RX_QUEUE.sender();
        let mut repl_tx_queue = REPL_TX_QUEUE.receiver();
        let mut ingress = [0u8; usb::MAX_PACKET_SIZE as usize];
        let mut tx_packet = [0u8; usb::MAX_PACKET_SIZE as usize];
        let mut pending_tx: Option<ReplFrame> = None;

        loop {
            embassy_futures::join::join(
                repl_receiver.wait_connection(),
                repl_sender.wait_connection(),
            )
            .await;
            wait_for_dtr(&repl_control, &mut repl_sender).await;
            pending_tx = None;

            defmt::info!("usb: REPL interface connected");

            loop {
                match select3(
                    repl_receiver.read_packet(&mut ingress),
                    async {
                        if pending_tx.is_none() {
                            let frame = repl_tx_queue.receive().await;
                            pending_tx = Some(frame);
                        }
                        let frame = pending_tx
                            .as_ref()
                            .expect("pending frame missing during REPL write");
                        let len = frame.len().min(tx_packet.len());
                        tx_packet[..len].copy_from_slice(&frame.as_slice()[..len]);

                        match repl_sender.write_packet(&tx_packet[..len]).await {
                            Ok(()) => {
                                pending_tx = None;
                                Ok(len)
                            }
                            Err(err) => Err(err),
                        }
                    },
                    repl_control.control_changed(),
                )
                .await
                {
                    Either3::First(Ok(count)) => {
                        if count == 0 {
                            continue;
                        }

                        let mut frame = ReplFrame::new();
                        if frame.extend_from_slice(&ingress[..count]).is_err() {
                            defmt::warn!("usb: dropping REPL frame len={} (overflow)", count);
                            continue;
                        }

                        repl_rx_queue.send(frame).await;
                    }
                    Either3::First(Err(EndpointError::Disabled)) => {
                        defmt::warn!("usb: REPL interface disabled");
                        break;
                    }
                    Either3::First(Err(_)) => {
                        defmt::warn!("usb: REPL read error");
                    }
                    Either3::Second(Ok(_)) => {}
                    Either3::Second(Err(EndpointError::Disabled)) => {
                        defmt::warn!("usb: REPL write disabled");
                        break;
                    }
                    Either3::Second(Err(_)) => {
                        defmt::warn!("usb: REPL write error");
                    }
                    Either3::Third(()) => {
                        if !repl_sender.dtr() {
                            defmt::warn!("usb: REPL host dropped DTR");
                            pending_tx = None;
                            break;
                        }
                    }
                }
            }
        }
    };

    let bridge_future = async move {
        let mut usb_to_ttl = BRIDGE_QUEUE.usb_to_ttl_sender();
        let mut ttl_to_usb = BRIDGE_QUEUE.ttl_to_usb_receiver();
        let mut activity_tx = BRIDGE_ACTIVITY.sender();
        let mut ingress = [0u8; usb::MAX_PACKET_SIZE as usize];
        let mut tx_packet = [0u8; usb::MAX_PACKET_SIZE as usize];
        let mut pending_tx: Option<BridgeFrame> = None;

        loop {
            embassy_futures::join::join(
                bridge_receiver.wait_connection(),
                bridge_sender.wait_connection(),
            )
            .await;
            wait_for_dtr(&bridge_control, &mut bridge_sender).await;

            defmt::info!("usb: bridge interface connected");

            loop {
                match select3(
                    bridge_receiver.read_packet(&mut ingress),
                    async {
                        if pending_tx.is_none() {
                            let frame = ttl_to_usb.receive().await;
                            pending_tx = Some(frame);
                        }

                        let frame = pending_tx
                            .as_ref()
                            .expect("pending frame missing during bridge write");
                        let len = frame.len().min(tx_packet.len());
                        tx_packet[..len].copy_from_slice(&frame.as_slice()[..len]);

                        match bridge_sender.write_packet(&tx_packet[..len]).await {
                            Ok(()) => {
                                pending_tx = None;
                                Ok(len)
                            }
                            Err(err) => Err(err),
                        }
                    },
                    bridge_control.control_changed(),
                )
                .await
                {
                    Either3::First(Ok(count)) => {
                        if count == 0 {
                            continue;
                        }

                        let mut frame = BridgeFrame::new();
                        if frame.extend_from_slice(&ingress[..count]).is_err() {
                            defmt::warn!("usb: dropping bridge frame len={} (overflow)", count);
                            continue;
                        }

                        usb_to_ttl.send(frame).await;

                        activity_tx
                            .send(BridgeActivityEvent {
                                kind: BridgeActivityKind::UsbToJetson,
                                timestamp: Instant::now(),
                                bytes: count,
                            })
                            .await;
                    }
                    Either3::First(Err(EndpointError::Disabled)) => {
                        defmt::warn!("usb: bridge interface disabled");
                        break;
                    }
                    Either3::First(Err(_)) => {
                        defmt::warn!("usb: bridge read error");
                    }
                    Either3::Second(Ok(len)) => {
                        activity_tx
                            .send(BridgeActivityEvent {
                                kind: BridgeActivityKind::JetsonToUsb,
                                timestamp: Instant::now(),
                                bytes: len,
                            })
                            .await;
                    }
                    Either3::Second(Err(EndpointError::Disabled)) => {
                        defmt::warn!("usb: bridge write disabled");
                        break;
                    }
                    Either3::Second(Err(_)) => {
                        defmt::warn!("usb: bridge write error");
                    }
                    Either3::Third(()) => {
                        if !bridge_sender.dtr() {
                            defmt::warn!("usb: bridge host dropped DTR");
                            break;
                        }
                    }
                }
            }
        }
    };

    join3(device.run(), repl_future, bridge_future).await;
}

#[cfg(target_os = "none")]
async fn wait_for_dtr<D>(
    control: &embassy_usb::class::cdc_acm::ControlChanged<'static>,
    sender: &mut embassy_usb::class::cdc_acm::Sender<'static, D>,
) where
    D: embassy_usb::driver::Driver<'static>,
{
    if sender.dtr() {
        return;
    }

    while !sender.dtr() {
        control.control_changed().await;
    }
}
