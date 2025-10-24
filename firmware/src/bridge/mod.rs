//! USB↔UART bridge scaffolding.
//!
//! This module defines the bounded channels shared by the USB CDC bridge tasks
//! and introduces an activity monitor placeholder so future tasks can wire the
//! recovery workflow described in `specs/001-build-orin-controller/data-model.md`.

#![allow(dead_code)]

use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::Instant;

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
    pending_recovery_release: bool,
    last_rx: Option<Instant>,
    last_tx: Option<Instant>,
}

impl<'a> BridgeActivityMonitor<'a> {
    /// Creates a new monitor around the supplied activity subscriber.
    pub fn new(subscriber: BridgeActivityReceiver<'a>) -> Self {
        Self {
            subscriber,
            pending_recovery_release: false,
            last_rx: None,
            last_tx: None,
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
    pub fn poll(&mut self) -> Option<BridgeActivityEvent> {
        self.subscriber.try_receive().ok().map(|event| {
            self.track(&event);
            event
        })
    }

    fn track(&mut self, event: &BridgeActivityEvent) {
        match event.kind {
            BridgeActivityKind::UsbToJetson => self.last_tx = Some(event.timestamp),
            BridgeActivityKind::JetsonToUsb => self.last_rx = Some(event.timestamp),
        }
    }
}
