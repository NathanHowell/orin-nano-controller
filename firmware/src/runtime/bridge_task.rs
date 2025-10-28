use embassy_futures::join::join;
use embassy_stm32 as hal;
use embassy_stm32::Peri;
use embassy_stm32::usart::{BufferedUart, Config as UartConfig, DataBits, Parity, StopBits};
use embassy_time::{Duration, Instant, Timer};
use embedded_io_async::{Read, Write};

use crate::bridge::{
    BRIDGE_FRAME_SIZE, BRIDGE_QUEUE_DEPTH, BridgeActivityBus, BridgeActivityEvent,
    BridgeActivityKind, BridgeFrame, BridgeQueue,
};
use crate::straps::FirmwareInstant;

const BRIDGE_UART_BUFFER_SIZE: usize = BRIDGE_FRAME_SIZE * BRIDGE_QUEUE_DEPTH;
const JETSON_UART_BAUD: u32 = 115_200;

static mut UART_TX_BUFFER: [u8; BRIDGE_UART_BUFFER_SIZE] = [0; BRIDGE_UART_BUFFER_SIZE];
static mut UART_RX_BUFFER: [u8; BRIDGE_UART_BUFFER_SIZE] = [0; BRIDGE_UART_BUFFER_SIZE];

embassy_stm32::bind_interrupts!(struct UartIrqs {
    USART3_4_5_6_LPUART1 => embassy_stm32::usart::BufferedInterruptHandler<hal::peripherals::USART5>;
});

#[embassy_executor::task]
pub async fn run(
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
        let mut ingress = [0u8; BRIDGE_FRAME_SIZE];
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

    join(usb_to_uart, uart_to_usb).await;
    loop {
        core::future::pending::<()>().await;
    }
}
