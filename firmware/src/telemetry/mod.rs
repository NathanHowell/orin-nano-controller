//! Telemetry ring buffer wrapper with firmware-specific logging helpers.
//!
//! The underlying ring buffer implementation lives in `controller-core`. This
//! module wraps the shared recorder so firmware can continue emitting defmt /
//! host console diagnostics whenever telemetry is recorded.

#![allow(dead_code)]

use core::{convert::TryFrom, time::Duration};

use crate::straps::{
    EventId, FirmwareInstant, SequenceOutcome, StrapAction, StrapId, StrapSequenceKind,
    TelemetryEventKind,
};
pub use controller_core::telemetry::{
    CommandTelemetry, SequenceTelemetry, TelemetryPayload,
};
use controller_core::telemetry::{
    TelemetryRecord as CoreTelemetryRecord, TelemetryRecorder as CoreTelemetryRecorder,
    TelemetryRing as CoreTelemetryRing,
};
use heapless::OldestOrdered;

/// Telemetry ring buffer type alias specialized for firmware timestamps.
pub type TelemetryRing = CoreTelemetryRing<FirmwareInstant>;

/// Telemetry record alias specialized for firmware timestamps.
pub type TelemetryRecord = CoreTelemetryRecord<FirmwareInstant>;

/// Firmware-side telemetry recorder that decorates the shared implementation
/// with logging hooks.
pub struct TelemetryRecorder {
    inner: CoreTelemetryRecorder<FirmwareInstant>,
}

impl TelemetryRecorder {
    /// Creates a new telemetry recorder with an empty history.
    pub fn new() -> Self {
        Self {
            inner: CoreTelemetryRecorder::new(),
        }
    }

    /// Returns an iterator over the recorded telemetry in chronological order.
    pub fn oldest_first(&self) -> OldestOrdered<'_, TelemetryRecord> {
        self.inner.oldest_first()
    }

    /// Returns the most recent telemetry record, if available.
    pub fn latest(&self) -> Option<&TelemetryRecord> {
        self.inner.latest()
    }

    /// Returns the number of records currently stored.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Records a strap transition and mirrors the event to firmware logs.
    pub fn record_strap_transition(
        &mut self,
        line: StrapId,
        action: StrapAction,
        timestamp: FirmwareInstant,
    ) -> EventId {
        let id = self.inner.record_strap_transition(line, action, timestamp);

        if let Some(record) = self.inner.latest().copied()
            && let TelemetryPayload::Strap(details) = record.details
        {
            log_strap_transition(
                line,
                action,
                record.timestamp,
                details.elapsed_since_previous,
            );
        }

        id
    }

    /// Records a queued command that cannot start immediately.
    pub fn record_command_pending(
        &mut self,
        kind: StrapSequenceKind,
        queue_depth: usize,
        requested_at: FirmwareInstant,
        timestamp: FirmwareInstant,
    ) -> EventId {
        let id = self
            .inner
            .record_command_pending(kind, queue_depth, requested_at, timestamp);

        if let Some(record) = self.inner.latest().copied()
            && let TelemetryPayload::Command(details) = record.details
        {
            log_command_event(CommandStage::Pending, kind, record.timestamp, details);
        }

        id
    }

    /// Records the moment a queued command begins execution.
    pub fn record_command_started(
        &mut self,
        kind: StrapSequenceKind,
        queue_depth: usize,
        requested_at: FirmwareInstant,
        timestamp: FirmwareInstant,
    ) -> EventId {
        let id = self
            .inner
            .record_command_started(kind, queue_depth, requested_at, timestamp);

        if let Some(record) = self.inner.latest().copied()
            && let TelemetryPayload::Command(details) = record.details
        {
            log_command_event(CommandStage::Started, kind, record.timestamp, details);
        }

        id
    }

    /// Records the completion of a strap sequence run.
    pub fn record_sequence_completion(
        &mut self,
        kind: StrapSequenceKind,
        outcome: SequenceOutcome,
        started_at: Option<FirmwareInstant>,
        timestamp: FirmwareInstant,
        events_recorded: usize,
    ) -> EventId {
        let id = self.inner.record_sequence_completion(
            kind,
            outcome,
            started_at,
            timestamp,
            events_recorded,
        );

        if let Some(record) = self.inner.latest().copied()
            && let TelemetryPayload::Sequence(details) = record.details
        {
            log_sequence_completion(kind, outcome, record.timestamp, details);
        }

        id
    }

    /// Records an arbitrary telemetry event with the supplied payload.
    pub fn record(
        &mut self,
        event: TelemetryEventKind,
        payload: TelemetryPayload,
        timestamp: FirmwareInstant,
    ) -> EventId {
        self.inner.record(event, payload, timestamp)
    }

    /// Provides mutable access to the shared telemetry recorder.
    pub(crate) fn inner_mut(&mut self) -> &mut CoreTelemetryRecorder<FirmwareInstant> {
        &mut self.inner
    }
}

impl Default for TelemetryRecorder {
    fn default() -> Self {
        Self::new()
    }
}

fn log_strap_transition(
    line: StrapId,
    action: StrapAction,
    timestamp: FirmwareInstant,
    elapsed: Option<Duration>,
) {
    let line_label = strap_line_label(line);
    let action_label = strap_action_label(action);
    let timestamp_us = timestamp.into_embassy().as_micros();
    let elapsed_us = elapsed.map(duration_to_micros);

    emit_strap_log(line_label, action_label, timestamp_us, elapsed_us);
}

#[cfg(target_os = "none")]
fn emit_strap_log(
    line: &'static str,
    action: &'static str,
    timestamp_us: u64,
    delta_us: Option<u64>,
) {
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
fn emit_strap_log(
    line: &'static str,
    action: &'static str,
    timestamp_us: u64,
    delta_us: Option<u64>,
) {
    if let Some(delta) = delta_us {
        println!(
            "telemetry:straps {line} {action} t={timestamp_us}us Δ={delta}us"
        );
    } else {
        println!("telemetry:straps {line} {action} t={timestamp_us}us");
    }
}

fn strap_line_label(line: StrapId) -> &'static str {
    match line {
        StrapId::Reset => "RESET*",
        StrapId::Rec => "REC*",
        StrapId::Pwr => "PWR*",
        StrapId::Apo => "APO",
    }
}

fn strap_action_label(action: StrapAction) -> &'static str {
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
    timestamp: FirmwareInstant,
    details: CommandTelemetry,
) {
    let stage_label = command_stage_label(stage);
    let kind_label = sequence_kind_label(kind);
    let timestamp_us = timestamp.into_embassy().as_micros();
    let queue_depth = u32::from(details.queue_depth);
    let wait_us = details.pending_for.map(duration_to_micros);

    emit_command_log(stage_label, kind_label, timestamp_us, queue_depth, wait_us);
}

fn command_stage_label(stage: CommandStage) -> &'static str {
    match stage {
        CommandStage::Pending => "pending",
        CommandStage::Started => "started",
    }
}

fn sequence_kind_label(kind: StrapSequenceKind) -> &'static str {
    match kind {
        StrapSequenceKind::NormalReboot => "NormalReboot",
        StrapSequenceKind::RecoveryEntry => "RecoveryEntry",
        StrapSequenceKind::RecoveryImmediate => "RecoveryImmediate",
        StrapSequenceKind::FaultRecovery => "FaultRecovery",
    }
}

fn log_sequence_completion(
    kind: StrapSequenceKind,
    outcome: SequenceOutcome,
    timestamp: FirmwareInstant,
    details: SequenceTelemetry,
) {
    let kind_label = sequence_kind_label(kind);
    let outcome_label = sequence_outcome_label(outcome);
    let timestamp_us = timestamp.into_embassy().as_micros();
    let duration_us = details.duration.map(duration_to_micros);
    let events = u32::from(details.events_recorded);

    emit_sequence_log(kind_label, outcome_label, timestamp_us, duration_us, events);
}

fn sequence_outcome_label(outcome: SequenceOutcome) -> &'static str {
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
            "telemetry:command {stage} kind={kind} depth={queue_depth} t={timestamp_us}us wait={wait}us"
        );
    } else {
        println!(
            "telemetry:command {stage} kind={kind} depth={queue_depth} t={timestamp_us}us"
        );
    }
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
            "telemetry:sequence complete kind={kind} outcome={outcome} t={timestamp_us}us duration={duration}us events={events_recorded}"
        );
    } else {
        println!(
            "telemetry:sequence complete kind={kind} outcome={outcome} t={timestamp_us}us events={events_recorded}"
        );
    }
}

fn duration_to_micros(duration: Duration) -> u64 {
    let clamped = duration.as_micros().min(u128::from(u64::MAX));
    u64::try_from(clamped).unwrap_or(u64::MAX)
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    fn micros(value: u64) -> FirmwareInstant {
        FirmwareInstant::from(embassy_time::Instant::from_micros(value))
    }

    #[test]
    fn records_elapsed_between_strap_events() {
        let mut recorder = TelemetryRecorder::new();

        let id1 =
            recorder.record_strap_transition(StrapId::Reset, StrapAction::AssertLow, micros(100));
        assert_eq!(id1, 0);

        let first = recorder.latest().copied().unwrap();
        assert_eq!(
            first.event,
            TelemetryEventKind::StrapAsserted(StrapId::Reset)
        );
        match first.details {
            TelemetryPayload::Strap(details) => {
                assert_eq!(details.elapsed_since_previous, None);
            }
            _ => panic!("expected strap payload"),
        }

        let id2 =
            recorder.record_strap_transition(StrapId::Reset, StrapAction::ReleaseHigh, micros(250));
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
