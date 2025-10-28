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
use cortex_m::interrupt;
#[cfg(target_os = "none")]
use cortex_m::register::primask;
#[cfg(target_os = "none")]
use critical_section::{self, RawRestoreState};
#[cfg(target_os = "none")]
use defmt_rtt as _;
#[cfg(target_os = "none")]
struct InterruptCriticalSection;
#[cfg(target_os = "none")]
critical_section::set_impl!(InterruptCriticalSection);
#[cfg(target_os = "none")]
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
use embedded_io_async::{Read, Write};

#[cfg(target_os = "none")]
use crate::bridge::{
    BridgeActivityBus, BridgeActivityEvent, BridgeActivityKind, BridgeFrame, BridgeQueue,
};
#[cfg(target_os = "none")]
use crate::repl::{REPL_RX_QUEUE, REPL_TX_QUEUE, ReplFrame, ReplSession};
#[cfg(target_os = "none")]
use crate::straps::orchestrator::{HardwareStrapDriver, NoopPowerMonitor, StrapOrchestrator};
#[cfg(target_os = "none")]
use crate::straps::{CommandProducer, FirmwareInstant};
#[cfg(target_os = "none")]
use crate::telemetry::TelemetryRecorder;
#[cfg(target_os = "none")]
use controller_core::orchestrator::SequenceScheduler;
#[cfg(target_os = "none")]
use controller_core::repl::commands::CommandExecutor;
#[cfg(target_os = "none")]
use controller_core::sequences::{
    SequenceTemplate, fault_recovery_template, normal_reboot_template, recovery_entry_template,
    recovery_immediate_template,
};
#[cfg(target_os = "none")]
use embassy_stm32::Peri;
#[cfg(target_os = "none")]
use embassy_stm32::gpio::{Level, OutputOpenDrain, Speed};
#[cfg(target_os = "none")]
use embassy_stm32::usart::{BufferedUart, Config as UartConfig, DataBits, Parity, StopBits};

#[cfg(target_os = "none")]
embassy_stm32::bind_interrupts!(struct UsbIrqs {
    USB_UCPD1_2 => embassy_stm32::usb::InterruptHandler<hal::peripherals::USB>;
});

#[cfg(target_os = "none")]
embassy_stm32::bind_interrupts!(struct UartIrqs {
    USART3_4_5_6_LPUART1 => embassy_stm32::usart::BufferedInterruptHandler<hal::peripherals::USART5>;
});

#[cfg(target_os = "none")]
const BRIDGE_UART_BUFFER_SIZE: usize = bridge::BRIDGE_FRAME_SIZE * bridge::BRIDGE_QUEUE_DEPTH;

#[cfg(target_os = "none")]
static mut UART_TX_BUFFER: [u8; BRIDGE_UART_BUFFER_SIZE] = [0; BRIDGE_UART_BUFFER_SIZE];
#[cfg(target_os = "none")]
static mut UART_RX_BUFFER: [u8; BRIDGE_UART_BUFFER_SIZE] = [0; BRIDGE_UART_BUFFER_SIZE];

#[cfg(target_os = "none")]
const JETSON_UART_BAUD: u32 = 115_200;

#[cfg(target_os = "none")]
static mut USB_STORAGE: MaybeUninit<usb::UsbDeviceStorage> = MaybeUninit::uninit();
#[cfg(target_os = "none")]
static COMMAND_QUEUE: straps::CommandQueue = Channel::new();
#[cfg(target_os = "none")]
static BRIDGE_QUEUE: BridgeQueue = BridgeQueue::new();
#[cfg(target_os = "none")]
static BRIDGE_ACTIVITY: BridgeActivityBus = BridgeActivityBus::new();

#[cfg(target_os = "none")]
const REGISTERED_TEMPLATES: [SequenceTemplate; 4] = [
    normal_reboot_template(),
    recovery_entry_template(),
    recovery_immediate_template(),
    fault_recovery_template(),
];

#[cfg(target_os = "none")]
#[embassy_executor::main]
async fn main(spawner: Spawner) {
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
        for template in REGISTERED_TEMPLATES {
            registry
                .register(template)
                .expect("strap template registration");
        }
    }
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
        .spawn(bridge_task(
            &BRIDGE_QUEUE,
            &BRIDGE_ACTIVITY,
            USART5,
            PB0,
            PB1,
        ))
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
        for template in REGISTERED_TEMPLATES {
            templates
                .register(template)
                .expect("scheduler template registration");
        }
    }

    let executor = CommandExecutor::new(scheduler);
    let mut session = ReplSession::new(executor);
    session.run().await;
}

#[cfg(target_os = "none")]
#[embassy_executor::task]
async fn bridge_task(
    queue: &'static BridgeQueue,
    activity: &'static BridgeActivityBus,
    usart: Peri<'static, hal::peripherals::USART5>,
    tx_pin: Peri<'static, hal::peripherals::PB0>,
    rx_pin: Peri<'static, hal::peripherals::PB1>,
) -> ! {
    let mut config = UartConfig::default();
    config.baudrate = JETSON_UART_BAUD;
    config.data_bits = DataBits::DataBits8;
    config.stop_bits = StopBits::STOP1;
    config.parity = Parity::ParityNone;

    let uart = unsafe {
        BufferedUart::new(
            usart,
            rx_pin,
            tx_pin,
            &mut UART_TX_BUFFER,
            &mut UART_RX_BUFFER,
            UartIrqs,
            config,
        )
        .expect("failed to initialize bridge UART")
    };

    let (mut uart_tx, mut uart_rx) = uart.split();

    let usb_to_ttl = queue.usb_to_ttl_receiver();
    let ttl_to_usb = queue.ttl_to_usb_sender();

    let usb_activity = activity.sender();
    let jetson_activity = activity.sender();

    let usb_to_uart = async move {
        loop {
            let frame = usb_to_ttl.receive().await;
            if frame.is_empty() {
                continue;
            }

            let data = frame.as_slice();
            let mut written = 0usize;

            while written < data.len() {
                match uart_tx.write(&data[written..]).await {
                    Ok(count) if count > 0 => {
                        written += count;
                    }
                    Ok(_) => {}
                    Err(_) => {
                        defmt::warn!("bridge: UART write error");
                        Timer::after(Duration::from_millis(5)).await;
                        break;
                    }
                }
            }

            if written == data.len() {
                if let Err(_) = uart_tx.flush().await {
                    defmt::warn!("bridge: UART flush error");
                    Timer::after(Duration::from_millis(5)).await;
                    continue;
                }

                usb_activity
                    .send(BridgeActivityEvent {
                        kind: BridgeActivityKind::UsbToJetson,
                        timestamp: FirmwareInstant::from(Instant::now()),
                        bytes: data.len(),
                    })
                    .await;
            }
        }
    };

    let uart_to_usb = async move {
        let mut ingress = [0u8; bridge::BRIDGE_FRAME_SIZE];
        loop {
            match uart_rx.read(&mut ingress).await {
                Ok(count) if count > 0 => {
                    let mut frame = BridgeFrame::new();
                    if frame.extend_from_slice(&ingress[..count]).is_err() {
                        defmt::warn!("bridge: dropping Jetson frame len={} (overflow)", count);
                        continue;
                    }

                    ttl_to_usb.send(frame).await;

                    jetson_activity
                        .send(BridgeActivityEvent {
                            kind: BridgeActivityKind::JetsonToUsb,
                            timestamp: FirmwareInstant::from(Instant::now()),
                            bytes: count,
                        })
                        .await;
                }
                Ok(_) => {}
                Err(_) => {
                    defmt::warn!("bridge: UART read error");
                    Timer::after(Duration::from_millis(5)).await;
                }
            }
        }
    };

    embassy_futures::join::join(usb_to_uart, uart_to_usb).await;
    loop {
        core::future::pending::<()>().await;
    }
}

#[cfg(target_os = "none")]
#[embassy_executor::task]
async fn usb_task(
    usb: Peri<'static, hal::peripherals::USB>,
    dp: Peri<'static, hal::peripherals::PA12>,
    dm: Peri<'static, hal::peripherals::PA11>,
) -> ! {
    let storage = unsafe { USB_STORAGE.write(usb::UsbDeviceStorage::new()) };
    let driver = embassy_stm32::usb::Driver::new(usb, UsbIrqs, dp, dm);

    let mut composite = usb::UsbComposite::new(driver, storage, usb::UsbDeviceStrings::default());

    let usb::CdcAcmHandle {
        sender: repl_sender,
        receiver: repl_receiver,
        control: repl_control,
        ..
    } = composite
        .take_repl()
        .expect("REPL CDC interface unavailable");

    let usb::CdcAcmHandle {
        sender: bridge_sender,
        receiver: bridge_receiver,
        control: bridge_control,
        ..
    } = composite
        .take_bridge()
        .expect("bridge CDC interface unavailable");

    let mut device = composite.device;

    let repl_future = run_repl_interface(repl_sender, repl_receiver, repl_control);
    let bridge_future = run_bridge_interface(bridge_sender, bridge_receiver, bridge_control);

    join3(device.run(), repl_future, bridge_future).await;
    loop {
        core::future::pending::<()>().await;
    }
}

#[cfg(target_os = "none")]
async fn run_repl_interface<D>(
    mut sender: embassy_usb::class::cdc_acm::Sender<'static, D>,
    mut receiver: embassy_usb::class::cdc_acm::Receiver<'static, D>,
    control: embassy_usb::class::cdc_acm::ControlChanged<'static>,
) -> !
where
    D: embassy_usb::driver::Driver<'static>,
{
    let repl_rx_queue = REPL_RX_QUEUE.sender();
    let repl_tx_queue = REPL_TX_QUEUE.receiver();
    let mut ingress = [0u8; usb::MAX_PACKET_SIZE as usize];
    let mut tx_packet = [0u8; usb::MAX_PACKET_SIZE as usize];
    let mut pending_tx: Option<ReplFrame> = None;

    loop {
        embassy_futures::join::join(receiver.wait_connection(), sender.wait_connection()).await;
        wait_for_dtr(&control, &mut sender).await;
        pending_tx.take();

        defmt::info!("usb: REPL interface connected");

        loop {
            match select3(
                receiver.read_packet(&mut ingress),
                async {
                    if pending_tx.is_none() {
                        pending_tx = Some(repl_tx_queue.receive().await);
                    }

                    let frame = pending_tx
                        .as_ref()
                        .expect("pending frame missing during REPL write");
                    let len = frame.len().min(tx_packet.len());
                    tx_packet[..len].copy_from_slice(&frame.as_slice()[..len]);

                    match sender.write_packet(&tx_packet[..len]).await {
                        Ok(()) => {
                            pending_tx.take();
                            Ok(len)
                        }
                        Err(err) => Err(err),
                    }
                },
                control.control_changed(),
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
                    if !sender.dtr() {
                        defmt::warn!("usb: REPL host dropped DTR");
                        pending_tx.take();
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "none")]
async fn run_bridge_interface<D>(
    mut sender: embassy_usb::class::cdc_acm::Sender<'static, D>,
    mut receiver: embassy_usb::class::cdc_acm::Receiver<'static, D>,
    control: embassy_usb::class::cdc_acm::ControlChanged<'static>,
) -> !
where
    D: embassy_usb::driver::Driver<'static>,
{
    let usb_to_ttl = BRIDGE_QUEUE.usb_to_ttl_sender();
    let ttl_to_usb = BRIDGE_QUEUE.ttl_to_usb_receiver();
    let mut ingress = [0u8; usb::MAX_PACKET_SIZE as usize];
    let mut tx_packet = [0u8; usb::MAX_PACKET_SIZE as usize];
    let mut pending_tx: Option<BridgeFrame> = None;

    loop {
        embassy_futures::join::join(receiver.wait_connection(), sender.wait_connection()).await;
        wait_for_dtr(&control, &mut sender).await;

        defmt::info!("usb: bridge interface connected");

        loop {
            match select3(
                receiver.read_packet(&mut ingress),
                async {
                    if pending_tx.is_none() {
                        pending_tx = Some(ttl_to_usb.receive().await);
                    }

                    let frame = pending_tx
                        .as_ref()
                        .expect("pending frame missing during bridge write");
                    let len = frame.len().min(tx_packet.len());
                    tx_packet[..len].copy_from_slice(&frame.as_slice()[..len]);

                    match sender.write_packet(&tx_packet[..len]).await {
                        Ok(()) => {
                            pending_tx.take();
                            Ok(len)
                        }
                        Err(err) => Err(err),
                    }
                },
                control.control_changed(),
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
                }
                Either3::First(Err(EndpointError::Disabled)) => {
                    defmt::warn!("usb: bridge interface disabled");
                    break;
                }
                Either3::First(Err(_)) => {
                    defmt::warn!("usb: bridge read error");
                }
                Either3::Second(Ok(_)) => {}
                Either3::Second(Err(EndpointError::Disabled)) => {
                    defmt::warn!("usb: bridge write disabled");
                    break;
                }
                Either3::Second(Err(_)) => {
                    defmt::warn!("usb: bridge write error");
                }
                Either3::Third(()) => {
                    if !sender.dtr() {
                        defmt::warn!("usb: bridge host dropped DTR");
                        break;
                    }
                }
            }
        }
    }
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
