use super::{BRIDGE_QUEUE, USB_STORAGE};
use crate::bridge::BridgeFrame;
use crate::repl::{REPL_RX_QUEUE, REPL_TX_QUEUE, ReplFrame};
use crate::usb::{self, UsbDeviceStrings};
use embassy_futures::join::{join, join3};
use embassy_futures::select::{Either3, select3};
use embassy_stm32 as hal;
use embassy_stm32::Peri;
use embassy_usb::driver::EndpointError;

embassy_stm32::bind_interrupts!(struct UsbIrqs {
    USB_UCPD1_2 => embassy_stm32::usb::InterruptHandler<hal::peripherals::USB>;
});

#[embassy_executor::task]
pub async fn run(
    usb: Peri<'static, hal::peripherals::USB>,
    dp: Peri<'static, hal::peripherals::PA12>,
    dm: Peri<'static, hal::peripherals::PA11>,
) -> ! {
    let storage = USB_STORAGE.init(usb::UsbDeviceStorage::new());
    let driver = embassy_stm32::usb::Driver::new(usb, UsbIrqs, dp, dm);

    let mut composite = usb::UsbComposite::new(driver, storage, UsbDeviceStrings::default());

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
        join(receiver.wait_connection(), sender.wait_connection()).await;
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
        join(receiver.wait_connection(), sender.wait_connection()).await;
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
