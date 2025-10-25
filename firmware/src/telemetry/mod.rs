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

use crate::straps::{
    EventId, SequenceOutcome, StrapAction, StrapLineId, StrapSequenceKind, TelemetryEventKind,
};

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
    /// Metadata emitted for command queue events.
    Command(CommandTelemetry),
    /// Summary information recorded when a strap sequence completes.
    Sequence(SequenceTelemetry),
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

/// Metadata describing queued-command telemetry events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandTelemetry {
    pub queue_depth: u8,
    pub pending_for: Option<Duration>,
}

/// Metadata describing the completion of a strap sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SequenceTelemetry {
    pub outcome: SequenceOutcome,
    pub duration: Option<Duration>,
    pub events_recorded: u8,
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

    /// Records a queued command that cannot start immediately.
    pub fn record_command_pending(
        &mut self,
        kind: StrapSequenceKind,
        queue_depth: usize,
        requested_at: Instant,
        timestamp: Instant,
    ) -> EventId {
        let wait = timestamp.saturating_duration_since(requested_at);
        let payload = TelemetryPayload::Command(CommandTelemetry {
            queue_depth: truncate_depth(queue_depth),
            pending_for: Some(wait),
        });

        let id = self.record(TelemetryEventKind::CommandPending(kind), payload, timestamp);
        log_command_event(
            CommandStage::Pending,
            kind,
            timestamp,
            queue_depth,
            Some(wait),
        );
        id
    }

    /// Records the moment a queued command begins execution.
    pub fn record_command_started(
        &mut self,
        kind: StrapSequenceKind,
        queue_depth: usize,
        requested_at: Instant,
        timestamp: Instant,
    ) -> EventId {
        let wait = timestamp.saturating_duration_since(requested_at);
        let payload = TelemetryPayload::Command(CommandTelemetry {
            queue_depth: truncate_depth(queue_depth),
            pending_for: Some(wait),
        });

        let id = self.record(TelemetryEventKind::CommandStarted(kind), payload, timestamp);
        log_command_event(
            CommandStage::Started,
            kind,
            timestamp,
            queue_depth,
            Some(wait),
        );
        id
    }

    /// Records the completion of a strap sequence run.
    pub fn record_sequence_completion(
        &mut self,
        kind: StrapSequenceKind,
        outcome: SequenceOutcome,
        started_at: Option<Instant>,
        timestamp: Instant,
        events_recorded: usize,
    ) -> EventId {
        let duration = started_at.map(|start| timestamp.saturating_duration_since(start));
        let payload = TelemetryPayload::Sequence(SequenceTelemetry {
            outcome,
            duration,
            events_recorded: truncate_count(events_recorded),
        });

        let id = self.record(
            TelemetryEventKind::SequenceComplete(kind),
            payload,
            timestamp,
        );
        log_sequence_completion(kind, outcome, timestamp, duration, events_recorded);
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
        Some(delta) => emit_log(line_label, action_label, timestamp_us, Some(delta)),
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
        defmt::info!("telemetry:straps {} {} t={}us", line, action, timestamp_us);
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum CommandStage {
    Pending,
    Started,
}

fn log_command_event(
    stage: CommandStage,
    kind: StrapSequenceKind,
    timestamp: Instant,
    queue_depth: usize,
    wait: Option<Duration>,
) {
    let stage_label = command_stage_label(stage);
    let kind_label = sequence_kind_label(kind);
    let timestamp_us = timestamp.as_micros();
    let queue_depth = u32::from(truncate_depth(queue_depth));
    let wait_us = wait.map(|value| value.as_micros());

    emit_command_log(stage_label, kind_label, timestamp_us, queue_depth, wait_us);
}

const fn truncate_depth(depth: usize) -> u8 {
    if depth > u8::MAX as usize {
        u8::MAX
    } else {
        depth as u8
    }
}

const fn truncate_count(count: usize) -> u8 {
    if count > u8::MAX as usize {
        u8::MAX
    } else {
        count as u8
    }
}

const fn command_stage_label(stage: CommandStage) -> &'static str {
    match stage {
        CommandStage::Pending => "pending",
        CommandStage::Started => "started",
    }
}

const fn sequence_kind_label(kind: StrapSequenceKind) -> &'static str {
    match kind {
        StrapSequenceKind::NormalReboot => "NormalReboot",
        StrapSequenceKind::RecoveryEntry => "RecoveryEntry",
        StrapSequenceKind::RecoveryImmediate => "RecoveryImmediate",
        StrapSequenceKind::FaultRecovery => "FaultRecovery",
    }
}

const fn sequence_outcome_label(outcome: SequenceOutcome) -> &'static str {
    match outcome {
        SequenceOutcome::Completed => "Completed",
        SequenceOutcome::SkippedCooldown => "SkippedCooldown",
    }
}

#[cfg(target_os = "none")]
fn emit_command_log(
    stage: &'static str,
    kind: &'static str,
    timestamp_us: u64,
    queue_depth: u32,
    wait_us: Option<u64>,
) {
    if let Some(wait) = wait_us {
        defmt::info!(
            "telemetry:command {} kind={} depth={} t={}us wait={}us",
            stage,
            kind,
            queue_depth,
            timestamp_us,
            wait
        );
    } else {
        defmt::info!(
            "telemetry:command {} kind={} depth={} t={}us",
            stage,
            kind,
            queue_depth,
            timestamp_us
        );
    }
}

#[cfg(not(target_os = "none"))]
fn emit_command_log(
    stage: &'static str,
    kind: &'static str,
    timestamp_us: u64,
    queue_depth: u32,
    wait_us: Option<u64>,
) {
    if let Some(wait) = wait_us {
        println!(
            "telemetry:command {} kind={} depth={} t={}us wait={}us",
            stage, kind, queue_depth, timestamp_us, wait
        );
    } else {
        println!(
            "telemetry:command {} kind={} depth={} t={}us",
            stage, kind, queue_depth, timestamp_us
        );
    }
}

fn log_sequence_completion(
    kind: StrapSequenceKind,
    outcome: SequenceOutcome,
    timestamp: Instant,
    duration: Option<Duration>,
    events_recorded: usize,
) {
    let kind_label = sequence_kind_label(kind);
    let outcome_label = sequence_outcome_label(outcome);
    let timestamp_us = timestamp.as_micros();
    let duration_us = duration.map(|value| value.as_micros());
    let events = u32::from(truncate_count(events_recorded));

    emit_sequence_log(kind_label, outcome_label, timestamp_us, duration_us, events);
}

#[cfg(target_os = "none")]
fn emit_sequence_log(
    kind: &'static str,
    outcome: &'static str,
    timestamp_us: u64,
    duration_us: Option<u64>,
    events_recorded: u32,
) {
    if let Some(duration) = duration_us {
        defmt::info!(
            "telemetry:sequence complete kind={} outcome={} t={}us duration={}us events={}",
            kind,
            outcome,
            timestamp_us,
            duration,
            events_recorded
        );
    } else {
        defmt::info!(
            "telemetry:sequence complete kind={} outcome={} t={}us events={}",
            kind,
            outcome,
            timestamp_us,
            events_recorded
        );
    }
}

#[cfg(not(target_os = "none"))]
fn emit_sequence_log(
    kind: &'static str,
    outcome: &'static str,
    timestamp_us: u64,
    duration_us: Option<u64>,
    events_recorded: u32,
) {
    if let Some(duration) = duration_us {
        println!(
            "telemetry:sequence complete kind={} outcome={} t={}us duration={}us events={}",
            kind, outcome, timestamp_us, duration, events_recorded
        );
    } else {
        println!(
            "telemetry:sequence complete kind={} outcome={} t={}us events={}",
            kind, outcome, timestamp_us, events_recorded
        );
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;
    use crate::straps::{SequenceOutcome, StrapAction, StrapLineId, StrapSequenceKind};

    fn micros(value: u64) -> Instant {
        Instant::from_micros(value)
    }

    #[test]
    fn records_elapsed_between_strap_events() {
        let mut recorder = TelemetryRecorder::new();

        let id1 = recorder.record_strap_transition(
            StrapLineId::Reset,
            StrapAction::AssertLow,
            micros(100),
        );
        assert_eq!(id1, 0);

        let first = recorder.latest().copied().unwrap();
        assert_eq!(
            first.event,
            TelemetryEventKind::StrapAsserted(StrapLineId::Reset)
        );
        match first.details {
            TelemetryPayload::Strap(details) => {
                assert_eq!(details.elapsed_since_previous, None);
            }
            _ => panic!("expected strap payload"),
        }

        let id2 = recorder.record_strap_transition(
            StrapLineId::Reset,
            StrapAction::ReleaseHigh,
            micros(250),
        );
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

    #[test]
    fn records_command_pending_event() {
        let mut recorder = TelemetryRecorder::new();
        let requested_at = micros(100);
        let timestamp = micros(220);

        let id = recorder.record_command_pending(
            StrapSequenceKind::NormalReboot,
            2,
            requested_at,
            timestamp,
        );
        assert_eq!(id, 0);

        let record = recorder.latest().copied().unwrap();
        assert_eq!(
            record.event,
            TelemetryEventKind::CommandPending(StrapSequenceKind::NormalReboot)
        );

        match record.details {
            TelemetryPayload::Command(details) => {
                assert_eq!(details.queue_depth, 2);
                let wait = details.pending_for.expect("missing pending duration");
                assert_eq!(wait.as_micros(), 120);
            }
            _ => panic!("expected command payload"),
        }
    }

    #[test]
    fn records_command_started_event_with_truncated_depth() {
        let mut recorder = TelemetryRecorder::new();
        // Seed an initial event to bump the ID counter.
        recorder.record_command_pending(
            StrapSequenceKind::RecoveryEntry,
            1,
            micros(50),
            micros(60),
        );

        let requested_at = micros(90);
        let start_time = micros(500);
        let id = recorder.record_command_started(
            StrapSequenceKind::FaultRecovery,
            300,
            requested_at,
            start_time,
        );
        assert_eq!(id, 1);

        let record = recorder.latest().copied().unwrap();
        assert_eq!(
            record.event,
            TelemetryEventKind::CommandStarted(StrapSequenceKind::FaultRecovery)
        );

        match record.details {
            TelemetryPayload::Command(details) => {
                assert_eq!(details.queue_depth, u8::MAX);
                let wait = details.pending_for.expect("missing wait for start");
                assert_eq!(wait.as_micros(), 410);
            }
            _ => panic!("expected command payload"),
        }
    }

    #[test]
    fn records_sequence_completion_with_duration() {
        let mut recorder = TelemetryRecorder::new();
        let started_at = micros(100);
        let completed_at = micros(1_300);

        let id = recorder.record_sequence_completion(
            StrapSequenceKind::NormalReboot,
            SequenceOutcome::Completed,
            Some(started_at),
            completed_at,
            3,
        );
        assert_eq!(id, 0);

        let record = recorder.latest().copied().unwrap();
        assert_eq!(
            record.event,
            TelemetryEventKind::SequenceComplete(StrapSequenceKind::NormalReboot)
        );

        match record.details {
            TelemetryPayload::Sequence(details) => {
                assert_eq!(details.outcome, SequenceOutcome::Completed);
                let duration = details.duration.expect("missing sequence duration");
                assert_eq!(duration.as_micros(), 1_200);
                assert_eq!(details.events_recorded, 3);
            }
            _ => panic!("expected sequence payload"),
        }
    }

    #[test]
    fn records_sequence_completion_without_start_timestamp() {
        let mut recorder = TelemetryRecorder::new();
        let completed_at = micros(2_000);

        recorder.record_sequence_completion(
            StrapSequenceKind::NormalReboot,
            SequenceOutcome::SkippedCooldown,
            None,
            completed_at,
            usize::MAX,
        );

        let record = recorder.latest().copied().unwrap();
        match record.details {
            TelemetryPayload::Sequence(details) => {
                assert_eq!(details.outcome, SequenceOutcome::SkippedCooldown);
                assert!(details.duration.is_none());
                assert_eq!(details.events_recorded, u8::MAX);
            }
            _ => panic!("expected sequence payload"),
        }
    }
}
