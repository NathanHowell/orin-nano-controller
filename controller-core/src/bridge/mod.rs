//! Shared bridge activity monitor logic.
//!
//! This module tracks UART/USB bridge activity timestamps, exposes helpers for
//! orchestrator bridge waits, and records telemetry events when console traffic
//! arrives. Firmware and host targets supply the transport plumbing and simply
//! feed observed events into the monitor.

#![cfg_attr(not(test), allow(dead_code))]

use crate::orchestrator::EventId;
use crate::telemetry::{TelemetryEventKind, TelemetryInstant, TelemetryPayload, TelemetryRecorder};

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
pub struct BridgeActivityEvent<TInstant>
where
    TInstant: Copy,
{
    pub kind: BridgeActivityKind,
    pub timestamp: TInstant,
    pub bytes: usize,
}

/// Result emitted when the monitor processes an activity event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeActivityUpdate<TInstant>
where
    TInstant: Copy,
{
    /// Raw activity metadata forwarded by the bridge tasks.
    pub event: BridgeActivityEvent<TInstant>,
    /// Telemetry record identifier generated for the activity (if any).
    pub telemetry_event: Option<EventId>,
    /// Indicates whether the REC strap should be released due to console activity.
    pub release_recovery: bool,
}

/// Snapshot emitted when the USB control link disconnects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeDisconnectNotice<TInstant>
where
    TInstant: Copy,
{
    pub timestamp: TInstant,
    pub recovery_release_pending: bool,
}

/// Tracks USB/UART activity and exposes timestamps for REPL status reporting.
pub struct BridgeActivityMonitor<TInstant>
where
    TInstant: Copy,
{
    pending_recovery_release: bool,
    last_rx: Option<TInstant>,
    last_tx: Option<TInstant>,
    link_attached: bool,
}

impl<TInstant> BridgeActivityMonitor<TInstant>
where
    TInstant: Copy,
{
    /// Creates a new monitor with no observed activity.
    #[must_use]
    pub const fn new() -> Self {
        Self {
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
    pub fn last_rx(&self) -> Option<TInstant> {
        self.last_rx
    }

    /// Returns the timestamp of the last USB→Jetson frame forwarded.
    pub fn last_tx(&self) -> Option<TInstant> {
        self.last_tx
    }

    /// Processes a single activity event and produces a bridge update.
    pub fn process_event(
        &mut self,
        event: BridgeActivityEvent<TInstant>,
        telemetry: &mut TelemetryRecorder<TInstant>,
    ) -> Option<BridgeActivityUpdate<TInstant>>
    where
        TInstant: TelemetryInstant,
    {
        if event.bytes == 0 {
            return None;
        }

        self.track(&event);
        let release = self.handle_recovery_release(&event);
        let telemetry_event = Self::record_telemetry(telemetry, &event);

        Some(BridgeActivityUpdate {
            event,
            telemetry_event,
            release_recovery: release,
        })
    }

    /// Marks the USB control link as attached.
    pub fn notify_usb_connect(&mut self) {
        self.link_attached = true;
    }

    /// Marks the USB control link as detached, returning the resulting notice.
    pub fn notify_usb_disconnect(
        &mut self,
        timestamp: TInstant,
    ) -> Option<BridgeDisconnectNotice<TInstant>> {
        if !self.link_attached {
            return None;
        }

        self.link_attached = false;
        let pending = self.pending_recovery_release;
        self.pending_recovery_release = false;

        Some(BridgeDisconnectNotice {
            timestamp,
            recovery_release_pending: pending,
        })
    }

    fn track(&mut self, event: &BridgeActivityEvent<TInstant>) {
        match event.kind {
            BridgeActivityKind::UsbToJetson => self.last_tx = Some(event.timestamp),
            BridgeActivityKind::JetsonToUsb => self.last_rx = Some(event.timestamp),
        }
    }

    fn handle_recovery_release(&mut self, event: &BridgeActivityEvent<TInstant>) -> bool {
        if event.kind != BridgeActivityKind::JetsonToUsb || !self.pending_recovery_release {
            return false;
        }

        self.pending_recovery_release = false;
        true
    }

    fn record_telemetry(
        telemetry: &mut TelemetryRecorder<TInstant>,
        event: &BridgeActivityEvent<TInstant>,
    ) -> Option<EventId>
    where
        TInstant: TelemetryInstant,
    {
        match event.kind {
            BridgeActivityKind::JetsonToUsb => Some(telemetry.record(
                TelemetryEventKind::RecoveryConsoleActivity,
                TelemetryPayload::none(),
                event.timestamp,
            )),
            BridgeActivityKind::UsbToJetson => None,
        }
    }
}

impl<TInstant> Default for BridgeActivityMonitor<TInstant>
where
    TInstant: Copy,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;

    #[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
    struct MicrosInstant(u64);

    impl MicrosInstant {
        fn from_micros(value: u64) -> Self {
            Self(value)
        }
    }

    impl TelemetryInstant for MicrosInstant {
        fn saturating_duration_since(&self, earlier: Self) -> Duration {
            let micros = self.0.saturating_sub(earlier.0);
            Duration::from_micros(micros)
        }
    }

    #[test]
    fn jetson_activity_releases_pending_recovery_and_records_telemetry() {
        let mut monitor = BridgeActivityMonitor::<MicrosInstant>::new();
        let mut telemetry = TelemetryRecorder::<MicrosInstant>::new();

        monitor.set_pending(true);

        let update = monitor
            .process_event(
                BridgeActivityEvent {
                    kind: BridgeActivityKind::JetsonToUsb,
                    timestamp: MicrosInstant::from_micros(5_000),
                    bytes: 17,
                },
                &mut telemetry,
            )
            .expect("activity update missing");

        assert!(update.release_recovery);
        assert!(!monitor.is_pending());
        let record = telemetry.latest().expect("telemetry missing");
        assert_eq!(record.event, TelemetryEventKind::RecoveryConsoleActivity);
        assert_eq!(record.timestamp, MicrosInstant::from_micros(5_000));
        assert_eq!(update.telemetry_event, Some(record.id));
    }

    #[test]
    fn usb_to_jetson_activity_updates_tx_timestamp_only() {
        let mut monitor = BridgeActivityMonitor::<MicrosInstant>::new();
        let mut telemetry = TelemetryRecorder::<MicrosInstant>::new();

        let update = monitor
            .process_event(
                BridgeActivityEvent {
                    kind: BridgeActivityKind::UsbToJetson,
                    timestamp: MicrosInstant::from_micros(10_000),
                    bytes: 8,
                },
                &mut telemetry,
            )
            .expect("activity update missing");

        assert!(!update.release_recovery);
        assert_eq!(monitor.last_tx(), Some(MicrosInstant::from_micros(10_000)));
        assert_eq!(monitor.last_rx(), None);
        assert!(update.telemetry_event.is_none());
        assert_eq!(telemetry.len(), 0);
    }

    #[test]
    fn zero_length_frames_are_ignored() {
        let mut monitor = BridgeActivityMonitor::<MicrosInstant>::new();
        let mut telemetry = TelemetryRecorder::<MicrosInstant>::new();

        monitor.set_pending(true);

        assert!(
            monitor
                .process_event(
                    BridgeActivityEvent {
                        kind: BridgeActivityKind::JetsonToUsb,
                        timestamp: MicrosInstant::from_micros(15_000),
                        bytes: 0,
                    },
                    &mut telemetry,
                )
                .is_none()
        );
        assert!(monitor.last_rx().is_none());
        assert!(monitor.last_tx().is_none());
        assert!(monitor.is_pending());
        assert_eq!(telemetry.len(), 0);
    }

    #[test]
    fn disconnect_clears_pending_flag_when_link_attached() {
        let mut monitor = BridgeActivityMonitor::<MicrosInstant>::new();
        monitor.notify_usb_connect();
        monitor.set_pending(true);

        let notice = monitor
            .notify_usb_disconnect(MicrosInstant::from_micros(20_000))
            .expect("disconnect notice missing");

        assert!(notice.recovery_release_pending);
        assert_eq!(notice.timestamp, MicrosInstant::from_micros(20_000));
        assert!(!monitor.is_pending());
        assert!(
            monitor
                .notify_usb_disconnect(MicrosInstant::from_micros(25_000))
                .is_none()
        );
    }
}
