//! Telemetry ring buffer and logging helpers.
//!
//! This module owns the fixed-capacity telemetry ring described in
//! `specs/001-build-orin-controller/data-model.md`. It records strap transitions
//! with timestamps, keeps elapsed timing between successive strap events, and
//! surfaces defmt/host console hooks so future tasks can emit observability data
//! without duplicating boilerplate.

#![allow(dead_code)]

use embassy_time::{Duration, Instant};
use heapless::{HistoryBuffer, OldestOrdered};

use crate::straps::{EventId, StrapAction, StrapLineId, TelemetryEventKind};

/// Total number of telemetry entries retained in memory.
pub const TELEMETRY_RING_CAPACITY: usize = 128;

/// Telemetry ring buffer type alias.
pub type TelemetryRing = HistoryBuffer<TelemetryRecord, TELEMETRY_RING_CAPACITY>;

/// Structured payloads attached to telemetry records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelemetryPayload {
    /// No additional metadata for the event.
    None,
    /// Details captured when a strap assertion or release occurs.
    Strap(StrapTelemetry),
}

impl TelemetryPayload {
    // Additional helpers will be added as more payload variants appear.
}

/// Extra metadata tracked for strap transitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrapTelemetry {
    pub line: StrapLineId,
    pub action: StrapAction,
    pub elapsed_since_previous: Option<Duration>,
}

/// Telemetry record stored in the ring buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelemetryRecord {
    pub id: EventId,
    pub timestamp: Instant,
    pub event: TelemetryEventKind,
    pub details: TelemetryPayload,
}

/// Records telemetry events into a fixed-size ring buffer and mirrors strap
/// transitions to defmt / stdout for quick inspection during bring-up.
pub struct TelemetryRecorder {
    ring: TelemetryRing,
    last_transition_at: Option<Instant>,
    next_event_id: EventId,
}

impl TelemetryRecorder {
    /// Creates a new telemetry recorder with an empty history.
    pub const fn new() -> Self {
        Self {
            ring: HistoryBuffer::new(),
            last_transition_at: None,
            next_event_id: 0,
        }
    }

    /// Returns an iterator over the recorded telemetry in chronological order.
    pub fn oldest_first(&self) -> OldestOrdered<'_, TelemetryRecord, TELEMETRY_RING_CAPACITY> {
        self.ring.oldest_ordered()
    }

    /// Returns the most recent telemetry record, if available.
    pub fn latest(&self) -> Option<&TelemetryRecord> {
        self.ring.recent()
    }

    /// Returns the number of records currently stored.
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// Records a strap transition, capturing elapsed time since the previous strap event
    /// and emitting a defmt/console log for immediate feedback.
    pub fn record_strap_transition(
        &mut self,
        line: StrapLineId,
        action: StrapAction,
        timestamp: Instant,
    ) -> EventId {
        let elapsed = self
            .last_transition_at
            .map(|previous| timestamp.saturating_duration_since(previous));
        self.last_transition_at = Some(timestamp);

        let event = match action {
            StrapAction::AssertLow => TelemetryEventKind::StrapAsserted(line),
            StrapAction::ReleaseHigh => TelemetryEventKind::StrapReleased(line),
        };

        let payload = TelemetryPayload::Strap(StrapTelemetry {
            line,
            action,
            elapsed_since_previous: elapsed,
        });

        let id = self.record(event, payload, timestamp);
        log_strap_transition(line, action, timestamp, elapsed);
        id
    }

    /// Records an arbitrary telemetry event with the supplied payload.
    pub fn record(
        &mut self,
        event: TelemetryEventKind,
        payload: TelemetryPayload,
        timestamp: Instant,
    ) -> EventId {
        let id = self.next_event_id;
        self.next_event_id = self.next_event_id.wrapping_add(1);

        self.ring.write(TelemetryRecord {
            id,
            timestamp,
            event,
            details: payload,
        });

        id
    }
}

impl Default for TelemetryRecorder {
    fn default() -> Self {
        Self::new()
    }
}

fn log_strap_transition(
    line: StrapLineId,
    action: StrapAction,
    timestamp: Instant,
    elapsed: Option<Duration>,
) {
    let line_label = strap_line_label(line);
    let action_label = strap_action_label(action);
    let timestamp_us = timestamp.as_micros();
    let elapsed_us = elapsed.map(|value| value.as_micros());

    match elapsed_us {
        Some(delta) => emit_log(
            line_label,
            action_label,
            timestamp_us,
            Some(delta),
        ),
        None => emit_log(line_label, action_label, timestamp_us, None),
    }
}

#[cfg(target_os = "none")]
fn emit_log(line: &'static str, action: &'static str, timestamp_us: u64, delta_us: Option<u64>) {
    if let Some(delta) = delta_us {
        defmt::info!(
            "telemetry:straps {} {} t={}us Δ={}us",
            line,
            action,
            timestamp_us,
            delta
        );
    } else {
        defmt::info!(
            "telemetry:straps {} {} t={}us",
            line,
            action,
            timestamp_us
        );
    }
}

#[cfg(not(target_os = "none"))]
fn emit_log(line: &'static str, action: &'static str, timestamp_us: u64, delta_us: Option<u64>) {
    if let Some(delta) = delta_us {
        println!(
            "telemetry:straps {} {} t={}us Δ={}us",
            line, action, timestamp_us, delta
        );
    } else {
        println!("telemetry:straps {} {} t={}us", line, action, timestamp_us);
    }
}

const fn strap_line_label(line: StrapLineId) -> &'static str {
    match line {
        StrapLineId::Reset => "RESET*",
        StrapLineId::Recovery => "REC*",
        StrapLineId::Power => "PWR*",
        StrapLineId::Apo => "APO",
    }
}

const fn strap_action_label(action: StrapAction) -> &'static str {
    match action {
        StrapAction::AssertLow => "assert",
        StrapAction::ReleaseHigh => "release",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::straps::{StrapAction, StrapLineId};

    fn micros(value: u64) -> Instant {
        Instant::from_micros(value)
    }

    #[test]
    fn records_elapsed_between_strap_events() {
        let mut recorder = TelemetryRecorder::new();

        let id1 = recorder.record_strap_transition(StrapLineId::Reset, StrapAction::AssertLow, micros(100));
        assert_eq!(id1, 0);

        let first = recorder.latest().copied().unwrap();
        assert_eq!(first.event, TelemetryEventKind::StrapAsserted(StrapLineId::Reset));
        match first.details {
            TelemetryPayload::Strap(details) => {
                assert_eq!(details.elapsed_since_previous, None);
            }
            _ => panic!("expected strap payload"),
        }

        let id2 = recorder.record_strap_transition(StrapLineId::Reset, StrapAction::ReleaseHigh, micros(250));
        assert_eq!(id2, 1);

        let second = recorder.latest().copied().unwrap();
        match second.details {
            TelemetryPayload::Strap(details) => {
                let elapsed = details.elapsed_since_previous.expect("missing elapsed");
                assert_eq!(elapsed.as_micros(), 150);
            }
            _ => panic!("expected strap payload"),
        }
    }
}
