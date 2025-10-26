//! USB↔UART bridge scaffolding.
//!
//! This module defines the bounded channels shared by the USB CDC bridge tasks
//! and introduces an activity monitor placeholder so future tasks can wire the
//! recovery workflow described in `specs/001-build-orin-controller/data-model.md`.

#![allow(dead_code)]

use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::Instant;

use crate::straps::{EventId, TelemetryEventKind};
use crate::telemetry::{TelemetryPayload, TelemetryRecorder};

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
pub type BridgeFrame = [u8; BRIDGE_FRAME_SIZE];

/// Channel used to shuttle frames between producers and consumers.
pub type BridgeChannel = Channel<BridgeMutex, BridgeFrame, BRIDGE_QUEUE_DEPTH>;

/// Sender handle tied to a bridge channel.
pub type BridgeSender<'a> = Sender<'a, BridgeMutex, BridgeFrame, BRIDGE_QUEUE_DEPTH>;

/// Receiver handle tied to a bridge channel.
pub type BridgeReceiver<'a> = Receiver<'a, BridgeMutex, BridgeFrame, BRIDGE_QUEUE_DEPTH>;

/// Identifies the direction for a bridge activity event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeActivityKind {
    /// Bytes forwarded from the USB CDC bridge port toward the Jetson UART.
    UsbToJetson,
    /// Bytes received from the Jetson UART and forwarded to the USB host.
    JetsonToUsb,
}

/// Metadata describing a single bridge activity observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeActivityEvent {
    pub kind: BridgeActivityKind,
    pub timestamp: Instant,
    pub bytes: usize,
}

/// Result emitted when the monitor processes an activity event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeActivityUpdate {
    /// Raw activity metadata forwarded by the bridge tasks.
    pub event: BridgeActivityEvent,
    /// Telemetry record identifier generated for the activity (if any).
    pub telemetry_event: Option<EventId>,
    /// Indicates whether the REC strap should be released due to console activity.
    pub release_recovery: bool,
}

/// Channel used to publish bridge activity events.
pub type BridgeActivityChannel = Channel<BridgeMutex, BridgeActivityEvent, ACTIVITY_QUEUE_DEPTH>;

/// Sender for bridge activity events.
pub type BridgeActivitySender<'a> =
    Sender<'a, BridgeMutex, BridgeActivityEvent, ACTIVITY_QUEUE_DEPTH>;

/// Receiver for bridge activity events.
pub type BridgeActivityReceiver<'a> =
    Receiver<'a, BridgeMutex, BridgeActivityEvent, ACTIVITY_QUEUE_DEPTH>;

/// Snapshot emitted when the USB control link disconnects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeDisconnectNotice {
    pub timestamp: Instant,
    pub recovery_release_pending: bool,
}

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
    pending_recovery_release: bool,
    last_rx: Option<Instant>,
    last_tx: Option<Instant>,
    link_attached: bool,
}

impl<'a> BridgeActivityMonitor<'a> {
    /// Creates a new monitor around the supplied activity subscriber.
    pub fn new(subscriber: BridgeActivityReceiver<'a>) -> Self {
        Self {
            subscriber,
            pending_recovery_release: false,
            last_rx: None,
            last_tx: None,
            link_attached: false,
        }
    }

    /// Returns `true` when a recovery sequence is waiting on bridge activity.
    pub fn is_pending(&self) -> bool {
        self.pending_recovery_release
    }

    /// Marks the monitor as waiting (or not) for activity before releasing REC.
    pub fn set_pending(&mut self, pending: bool) {
        self.pending_recovery_release = pending;
    }

    /// Returns the timestamp of the last Jetson→USB frame observed.
    pub fn last_rx(&self) -> Option<Instant> {
        self.last_rx
    }

    /// Returns the timestamp of the last USB→Jetson frame forwarded.
    pub fn last_tx(&self) -> Option<Instant> {
        self.last_tx
    }

    /// Processes a single pending activity event, if available.
    pub fn poll(&mut self, telemetry: &mut TelemetryRecorder) -> Option<BridgeActivityUpdate> {
        let event = self.subscriber.try_receive().ok()?;

        if event.bytes == 0 {
            return None;
        }

        self.track(&event);
        let release = self.handle_recovery_release(&event);
        let telemetry_event = self.record_telemetry(telemetry, &event);
        log_bridge_activity(&event, release);

        Some(BridgeActivityUpdate {
            event,
            telemetry_event,
            release_recovery: release,
        })
    }

    /// Marks the USB control link as attached and logs the transition.
    pub fn notify_usb_connect(&mut self, timestamp: Instant) {
        if self.link_attached {
            return;
        }

        self.link_attached = true;
        log_usb_connect(timestamp);
    }

    /// Marks the USB control link as detached, returning the resulting notice.
    pub fn notify_usb_disconnect(&mut self, timestamp: Instant) -> Option<BridgeDisconnectNotice> {
        if !self.link_attached {
            return None;
        }

        self.link_attached = false;
        let pending = self.pending_recovery_release;
        self.pending_recovery_release = false;

        log_usb_disconnect(pending, timestamp);
        Some(BridgeDisconnectNotice {
            timestamp,
            recovery_release_pending: pending,
        })
    }

    fn track(&mut self, event: &BridgeActivityEvent) {
        match event.kind {
            BridgeActivityKind::UsbToJetson => self.last_tx = Some(event.timestamp),
            BridgeActivityKind::JetsonToUsb => self.last_rx = Some(event.timestamp),
        }
    }

    fn handle_recovery_release(&mut self, event: &BridgeActivityEvent) -> bool {
        if event.kind != BridgeActivityKind::JetsonToUsb || !self.pending_recovery_release {
            return false;
        }

        self.pending_recovery_release = false;
        true
    }

    fn record_telemetry(
        &mut self,
        telemetry: &mut TelemetryRecorder,
        event: &BridgeActivityEvent,
    ) -> Option<EventId> {
        match event.kind {
            BridgeActivityKind::JetsonToUsb => Some(telemetry.record(
                TelemetryEventKind::RecoveryConsoleActivity,
                TelemetryPayload::None,
                event.timestamp,
            )),
            BridgeActivityKind::UsbToJetson => None,
        }
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
    match event.kind {
        BridgeActivityKind::UsbToJetson => {
            defmt::debug!(
                "bridge: usb→jetson bytes={=usize} t={}us",
                event.bytes,
                event.timestamp.as_micros()
            );
        }
        BridgeActivityKind::JetsonToUsb => {
            if release {
                defmt::info!(
                    "bridge: recovery console activity bytes={=usize} t={}us (releasing REC)",
                    event.bytes,
                    event.timestamp.as_micros()
                );
            } else {
                defmt::debug!(
                    "bridge: jetson→usb bytes={=usize} t={}us",
                    event.bytes,
                    event.timestamp.as_micros()
                );
            }
        }
    }
}

#[cfg(not(target_os = "none"))]
fn log_bridge_activity(event: &BridgeActivityEvent, release: bool) {
    match event.kind {
        BridgeActivityKind::UsbToJetson => {
            println!(
                "bridge: usb→jetson bytes={} t={}us",
                event.bytes,
                event.timestamp.as_micros()
            );
        }
        BridgeActivityKind::JetsonToUsb => {
            if release {
                println!(
                    "bridge: recovery console activity bytes={} t={}us (releasing REC)",
                    event.bytes,
                    event.timestamp.as_micros()
                );
            } else {
                println!(
                    "bridge: jetson→usb bytes={} t={}us",
                    event.bytes,
                    event.timestamp.as_micros()
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
                timestamp: Instant::from_micros(5_000),
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
        assert_eq!(record.timestamp, Instant::from_micros(5_000));
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
                timestamp: Instant::from_micros(10_000),
                bytes: 8,
            })
            .expect("send should succeed");

        let update = monitor
            .poll(&mut telemetry)
            .expect("activity update missing");

        assert!(!update.release_recovery);
        assert_eq!(monitor.last_tx(), Some(Instant::from_micros(10_000)));
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
                timestamp: Instant::from_micros(15_000),
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
