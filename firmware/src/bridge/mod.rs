//! USB↔UART bridge scaffolding.
//!
//! This module defines the bounded channels shared by the USB CDC bridge tasks
//! and introduces an activity monitor placeholder so future tasks can wire the
//! recovery workflow described in `specs/001-build-orin-controller/data-model.md`.

#![allow(dead_code)]

use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::Instant;
use heapless::Vec;

use crate::straps::FirmwareInstant;
use crate::telemetry::TelemetryRecorder;
pub use controller_core::bridge::BridgeActivityKind;
use controller_core::bridge::{
    BridgeActivityEvent as CoreBridgeActivityEvent,
    BridgeActivityMonitor as CoreBridgeActivityMonitor,
    BridgeActivityUpdate as CoreBridgeActivityUpdate,
    BridgeDisconnectNotice as CoreBridgeDisconnectNotice,
};

#[cfg(not(target_os = "none"))]
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
#[cfg(target_os = "none")]
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;

#[cfg(target_os = "none")]
type BridgeMutex = ThreadModeRawMutex;
#[cfg(not(target_os = "none"))]
type BridgeMutex = NoopRawMutex;

/// Maximum payload size (bytes) for bridge frames forwarded between USB and UART.
pub const BRIDGE_FRAME_SIZE: usize = 64;

/// Depth for each bounded bridge channel.
pub const BRIDGE_QUEUE_DEPTH: usize = 4;

/// Depth for activity events observed by the bridge monitor.
pub const ACTIVITY_QUEUE_DEPTH: usize = 4;

/// Fixed-size frame exchanged between bridge tasks.
pub type BridgeFrame = Vec<u8, BRIDGE_FRAME_SIZE>;

/// Channel used to shuttle frames between producers and consumers.
pub type BridgeChannel = Channel<BridgeMutex, BridgeFrame, BRIDGE_QUEUE_DEPTH>;

/// Sender handle tied to a bridge channel.
pub type BridgeSender<'a> = Sender<'a, BridgeMutex, BridgeFrame, BRIDGE_QUEUE_DEPTH>;

/// Receiver handle tied to a bridge channel.
pub type BridgeReceiver<'a> = Receiver<'a, BridgeMutex, BridgeFrame, BRIDGE_QUEUE_DEPTH>;

/// Bridge activity event shared with the orchestrator.
pub type BridgeActivityEvent = CoreBridgeActivityEvent<FirmwareInstant>;

/// Activity update emitted after processing an event.
pub type BridgeActivityUpdate = CoreBridgeActivityUpdate<FirmwareInstant>;

/// Notice emitted when the USB control link disconnects.
pub type BridgeDisconnectNotice = CoreBridgeDisconnectNotice<FirmwareInstant>;

/// Channel used to publish bridge activity events.
pub type BridgeActivityChannel = Channel<BridgeMutex, BridgeActivityEvent, ACTIVITY_QUEUE_DEPTH>;

/// Sender for bridge activity events.
pub type BridgeActivitySender<'a> =
    Sender<'a, BridgeMutex, BridgeActivityEvent, ACTIVITY_QUEUE_DEPTH>;

/// Receiver for bridge activity events.
pub type BridgeActivityReceiver<'a> =
    Receiver<'a, BridgeMutex, BridgeActivityEvent, ACTIVITY_QUEUE_DEPTH>;

/// Bundles the bounded USB↔UART channels so tasks can share a single instance.
pub struct BridgeQueue {
    pub usb_to_ttl: BridgeChannel,
    pub ttl_to_usb: BridgeChannel,
}

impl BridgeQueue {
    /// Creates a new bridge queue with empty USB↔UART channels.
    pub const fn new() -> Self {
        Self {
            usb_to_ttl: Channel::new(),
            ttl_to_usb: Channel::new(),
        }
    }

    /// Returns a sender handle for the USB→UART path.
    pub fn usb_to_ttl_sender(&self) -> BridgeSender<'_> {
        self.usb_to_ttl.sender()
    }

    /// Returns a receiver handle for the USB→UART path.
    pub fn usb_to_ttl_receiver(&self) -> BridgeReceiver<'_> {
        self.usb_to_ttl.receiver()
    }

    /// Returns a sender handle for the UART→USB path.
    pub fn ttl_to_usb_sender(&self) -> BridgeSender<'_> {
        self.ttl_to_usb.sender()
    }

    /// Returns a receiver handle for the UART→USB path.
    pub fn ttl_to_usb_receiver(&self) -> BridgeReceiver<'_> {
        self.ttl_to_usb.receiver()
    }
}

/// Helper that owns the activity event channel and hands out producer/consumer halves.
pub struct BridgeActivityBus {
    channel: BridgeActivityChannel,
}

impl BridgeActivityBus {
    /// Creates a new, empty bridge activity bus.
    pub const fn new() -> Self {
        Self {
            channel: Channel::new(),
        }
    }

    /// Returns a sender used by bridge tasks to report activity.
    pub fn sender(&self) -> BridgeActivitySender<'_> {
        self.channel.sender()
    }

    /// Returns a receiver consumed by the activity monitor.
    pub fn receiver(&self) -> BridgeActivityReceiver<'_> {
        self.channel.receiver()
    }
}

/// Tracks USB/UART activity and exposes timestamps for REPL status reporting.
pub struct BridgeActivityMonitor<'a> {
    subscriber: BridgeActivityReceiver<'a>,
    monitor: CoreBridgeActivityMonitor<FirmwareInstant>,
}

impl<'a> BridgeActivityMonitor<'a> {
    /// Creates a new monitor around the supplied activity subscriber.
    pub fn new(subscriber: BridgeActivityReceiver<'a>) -> Self {
        Self {
            subscriber,
            monitor: CoreBridgeActivityMonitor::new(),
        }
    }

    /// Returns `true` when a recovery sequence is waiting on bridge activity.
    pub fn is_pending(&self) -> bool {
        self.monitor.is_pending()
    }

    /// Marks the monitor as waiting (or not) for activity before releasing REC.
    pub fn set_pending(&mut self, pending: bool) {
        self.monitor.set_pending(pending);
    }

    /// Returns the timestamp of the last Jetson→USB frame observed.
    pub fn last_rx(&self) -> Option<FirmwareInstant> {
        self.monitor.last_rx()
    }

    /// Returns the timestamp of the last USB→Jetson frame forwarded.
    pub fn last_tx(&self) -> Option<FirmwareInstant> {
        self.monitor.last_tx()
    }

    /// Processes a single pending activity event, if available.
    pub fn poll(&mut self, telemetry: &mut TelemetryRecorder) -> Option<BridgeActivityUpdate> {
        let event = self.subscriber.try_receive().ok()?;
        let update = self.monitor.process_event(event, telemetry.inner_mut())?;

        log_bridge_activity(&update.event, update.release_recovery);
        Some(update)
    }

    /// Marks the USB control link as attached and logs the transition.
    pub fn notify_usb_connect(&mut self, timestamp: FirmwareInstant) {
        self.monitor.notify_usb_connect();
        log_usb_connect(timestamp.into_embassy());
    }

    /// Marks the USB control link as detached, returning the resulting notice.
    pub fn notify_usb_disconnect(
        &mut self,
        timestamp: FirmwareInstant,
    ) -> Option<BridgeDisconnectNotice> {
        let notice = self.monitor.notify_usb_disconnect(timestamp)?;

        log_usb_disconnect(
            notice.recovery_release_pending,
            notice.timestamp.into_embassy(),
        );
        Some(notice)
    }
}

#[cfg(target_os = "none")]
fn log_usb_connect(timestamp: Instant) {
    defmt::info!(
        "bridge: USB control link attached t={}us",
        timestamp.as_micros()
    );
}

#[cfg(not(target_os = "none"))]
fn log_usb_connect(timestamp: Instant) {
    println!(
        "bridge: USB control link attached t={}us",
        timestamp.as_micros()
    );
}

#[cfg(target_os = "none")]
fn log_usb_disconnect(pending_recovery: bool, timestamp: Instant) {
    if pending_recovery {
        defmt::warn!(
            "bridge: USB control link lost while awaiting recovery console activity t={}us",
            timestamp.as_micros()
        );
    } else {
        defmt::warn!(
            "bridge: USB control link lost t={}us",
            timestamp.as_micros()
        );
    }
}

#[cfg(target_os = "none")]
fn log_bridge_activity(event: &BridgeActivityEvent, release: bool) {
    let timestamp = event.timestamp.into_embassy();
    match event.kind {
        BridgeActivityKind::UsbToJetson => {
            defmt::debug!(
                "bridge: usb→jetson bytes={=usize} t={}us",
                event.bytes,
                timestamp.as_micros()
            );
        }
        BridgeActivityKind::JetsonToUsb => {
            if release {
                defmt::info!(
                    "bridge: recovery console activity bytes={=usize} t={}us (releasing REC)",
                    event.bytes,
                    timestamp.as_micros()
                );
            } else {
                defmt::debug!(
                    "bridge: jetson→usb bytes={=usize} t={}us",
                    event.bytes,
                    timestamp.as_micros()
                );
            }
        }
    }
}

#[cfg(not(target_os = "none"))]
fn log_bridge_activity(event: &BridgeActivityEvent, release: bool) {
    let timestamp = event.timestamp.into_embassy();
    match event.kind {
        BridgeActivityKind::UsbToJetson => {
            println!(
                "bridge: usb→jetson bytes={} t={}us",
                event.bytes,
                timestamp.as_micros()
            );
        }
        BridgeActivityKind::JetsonToUsb => {
            if release {
                println!(
                    "bridge: recovery console activity bytes={} t={}us (releasing REC)",
                    event.bytes,
                    timestamp.as_micros()
                );
            } else {
                println!(
                    "bridge: jetson→usb bytes={} t={}us",
                    event.bytes,
                    timestamp.as_micros()
                );
            }
        }
    }
}

#[cfg(not(target_os = "none"))]
fn log_usb_disconnect(pending_recovery: bool, timestamp: Instant) {
    if pending_recovery {
        println!(
            "bridge: USB control link lost while awaiting recovery console activity t={}us",
            timestamp.as_micros()
        );
    } else {
        println!(
            "bridge: USB control link lost t={}us",
            timestamp.as_micros()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::straps::TelemetryEventKind;

    #[test]
    fn jetson_activity_releases_pending_recovery_and_records_telemetry() {
        let bus = BridgeActivityBus::new();
        let sender = bus.sender();
        let subscriber = bus.receiver();
        let mut monitor = BridgeActivityMonitor::new(subscriber);
        let mut telemetry = TelemetryRecorder::new();

        monitor.set_pending(true);

        sender
            .try_send(BridgeActivityEvent {
                kind: BridgeActivityKind::JetsonToUsb,
                timestamp: FirmwareInstant::from(Instant::from_micros(5_000)),
                bytes: 17,
            })
            .expect("send should succeed");

        let update = monitor
            .poll(&mut telemetry)
            .expect("activity update missing");

        assert!(update.release_recovery);
        assert!(!monitor.is_pending());
        let record = telemetry.latest().expect("telemetry missing");
        assert_eq!(record.event, TelemetryEventKind::RecoveryConsoleActivity);
        assert_eq!(
            record.timestamp,
            FirmwareInstant::from(Instant::from_micros(5_000))
        );
        assert_eq!(update.telemetry_event, Some(record.id));
    }

    #[test]
    fn usb_to_jetson_activity_updates_tx_timestamp_only() {
        let bus = BridgeActivityBus::new();
        let sender = bus.sender();
        let subscriber = bus.receiver();
        let mut monitor = BridgeActivityMonitor::new(subscriber);
        let mut telemetry = TelemetryRecorder::new();

        sender
            .try_send(BridgeActivityEvent {
                kind: BridgeActivityKind::UsbToJetson,
                timestamp: FirmwareInstant::from(Instant::from_micros(10_000)),
                bytes: 8,
            })
            .expect("send should succeed");

        let update = monitor
            .poll(&mut telemetry)
            .expect("activity update missing");

        assert!(!update.release_recovery);
        assert_eq!(
            monitor.last_tx(),
            Some(FirmwareInstant::from(Instant::from_micros(10_000)))
        );
        assert_eq!(monitor.last_rx(), None);
        assert!(update.telemetry_event.is_none());
        assert_eq!(telemetry.len(), 0);
    }

    #[test]
    fn zero_length_frames_are_ignored() {
        let bus = BridgeActivityBus::new();
        let sender = bus.sender();
        let subscriber = bus.receiver();
        let mut monitor = BridgeActivityMonitor::new(subscriber);
        let mut telemetry = TelemetryRecorder::new();

        monitor.set_pending(true);

        sender
            .try_send(BridgeActivityEvent {
                kind: BridgeActivityKind::JetsonToUsb,
                timestamp: FirmwareInstant::from(Instant::from_micros(15_000)),
                bytes: 0,
            })
            .expect("send should succeed");

        assert!(monitor.poll(&mut telemetry).is_none());
        assert!(monitor.last_rx().is_none());
        assert!(monitor.last_tx().is_none());
        assert!(monitor.is_pending());
        assert_eq!(telemetry.len(), 0);
    }
}
