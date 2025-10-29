//! Telemetry event catalog and payload structures shared by firmware and host targets.
//!
//! The data model mirrors the documentation in
//! `specs/001-build-orin-controller/data-model.md`, providing strongly typed
//! event kinds that can be serialized to compact numeric codes for transport
//! over diagnostics channels. Payload enums carry the extra metadata required
//! by the REPL and evidence capture tooling while remaining `no_std`
//! compatible.

#![cfg_attr(not(test), allow(dead_code))]

use core::{convert::TryFrom, fmt, time::Duration};

use heapless::{HistoryBuf, OldestOrdered, Vec};

use crate::orchestrator::{EventId, SequenceOutcome};
use crate::sequences::{StrapAction, StrapId, StrapSequenceKind};

/// Maximum length for diagnostics note payloads.
pub const MAX_DIAGNOSTIC_NOTES: usize = 96;

/// Canonical timestamp units for telemetry records (microseconds).
pub type TimestampMicros = u64;

/// Structured diagnostics frame mirrored over host transports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticsFrame {
    pub event: TelemetryEventKind,
    pub timestamp_us: TimestampMicros,
    pub jetson_power: Option<bool>,
    pub notes: Vec<u8, MAX_DIAGNOSTIC_NOTES>,
}

impl DiagnosticsFrame {
    /// Creates a new diagnostics frame with the provided capacity.
    #[must_use]
    pub fn new(
        event: TelemetryEventKind,
        timestamp_us: TimestampMicros,
        jetson_power: Option<bool>,
    ) -> Self {
        Self {
            event,
            timestamp_us,
            jetson_power,
            notes: Vec::new(),
        }
    }
}

/// Discriminated telemetry events shared across all controller targets.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TelemetryEventKind {
    StrapAsserted(StrapId),
    StrapReleased(StrapId),
    PowerStable,
    RecoveryConsoleActivity,
    CommandPending(StrapSequenceKind),
    CommandStarted(StrapSequenceKind),
    SequenceComplete(StrapSequenceKind),
    UsbDisconnect,
    Custom(u16),
}

impl fmt::Display for TelemetryEventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TelemetryEventKind::StrapAsserted(line) => write!(f, "strap-asserted {line}"),
            TelemetryEventKind::StrapReleased(line) => write!(f, "strap-released {line}"),
            TelemetryEventKind::PowerStable => f.write_str("power-stable"),
            TelemetryEventKind::RecoveryConsoleActivity => f.write_str("recovery-console-activity"),
            TelemetryEventKind::CommandPending(kind) => write!(f, "command-pending {kind}"),
            TelemetryEventKind::CommandStarted(kind) => write!(f, "command-started {kind}"),
            TelemetryEventKind::SequenceComplete(kind) => {
                write!(f, "sequence-complete {kind}")
            }
            TelemetryEventKind::UsbDisconnect => f.write_str("usb-disconnect"),
            TelemetryEventKind::Custom(code) => write!(f, "custom({code})"),
        }
    }
}

impl TelemetryEventKind {
    const STRAP_ASSERT_BASE: u16 = 0x0000;
    const STRAP_RELEASE_BASE: u16 = 0x0004;
    const POWER_STABLE_CODE: u16 = 0x0008;
    const RECOVERY_ACTIVITY_CODE: u16 = 0x0009;
    const USB_DISCONNECT_CODE: u16 = 0x000A;
    const COMMAND_PENDING_BASE: u16 = 0x0010;
    const COMMAND_STARTED_BASE: u16 = 0x0014;
    const SEQUENCE_COMPLETE_BASE: u16 = 0x0018;

    /// Encodes the event into a compact transport-friendly discriminant.
    #[must_use]
    pub const fn to_raw(self) -> u16 {
        match self {
            TelemetryEventKind::StrapAsserted(line) => Self::STRAP_ASSERT_BASE + strap_index(line),
            TelemetryEventKind::StrapReleased(line) => Self::STRAP_RELEASE_BASE + strap_index(line),
            TelemetryEventKind::PowerStable => Self::POWER_STABLE_CODE,
            TelemetryEventKind::RecoveryConsoleActivity => Self::RECOVERY_ACTIVITY_CODE,
            TelemetryEventKind::CommandPending(kind) => {
                Self::COMMAND_PENDING_BASE + sequence_index(kind)
            }
            TelemetryEventKind::CommandStarted(kind) => {
                Self::COMMAND_STARTED_BASE + sequence_index(kind)
            }
            TelemetryEventKind::SequenceComplete(kind) => {
                Self::SEQUENCE_COMPLETE_BASE + sequence_index(kind)
            }
            TelemetryEventKind::UsbDisconnect => Self::USB_DISCONNECT_CODE,
            TelemetryEventKind::Custom(code) => code,
        }
    }

    /// Decodes a raw discriminant into a telemetry event, falling back to [`Custom`].
    #[must_use]
    pub fn from_raw(code: u16) -> Self {
        match code {
            Self::POWER_STABLE_CODE => TelemetryEventKind::PowerStable,
            Self::RECOVERY_ACTIVITY_CODE => TelemetryEventKind::RecoveryConsoleActivity,
            Self::USB_DISCONNECT_CODE => TelemetryEventKind::UsbDisconnect,
            value if (Self::STRAP_ASSERT_BASE..Self::STRAP_RELEASE_BASE).contains(&value) => {
                let offset = value - Self::STRAP_ASSERT_BASE;
                strap_from_index(offset).map_or(TelemetryEventKind::Custom(value), |line| {
                    TelemetryEventKind::StrapAsserted(line)
                })
            }
            value if (Self::STRAP_RELEASE_BASE..Self::COMMAND_PENDING_BASE).contains(&value) => {
                let offset = value - Self::STRAP_RELEASE_BASE;
                strap_from_index(offset).map_or(TelemetryEventKind::Custom(value), |line| {
                    TelemetryEventKind::StrapReleased(line)
                })
            }
            value if (Self::COMMAND_PENDING_BASE..Self::COMMAND_STARTED_BASE).contains(&value) => {
                let offset = value - Self::COMMAND_PENDING_BASE;
                sequence_from_index(offset).map_or(TelemetryEventKind::Custom(value), |kind| {
                    TelemetryEventKind::CommandPending(kind)
                })
            }
            value
                if (Self::COMMAND_STARTED_BASE..Self::SEQUENCE_COMPLETE_BASE).contains(&value) =>
            {
                let offset = value - Self::COMMAND_STARTED_BASE;
                sequence_from_index(offset).map_or(TelemetryEventKind::Custom(value), |kind| {
                    TelemetryEventKind::CommandStarted(kind)
                })
            }
            value
                if (Self::SEQUENCE_COMPLETE_BASE..Self::SEQUENCE_COMPLETE_BASE + 4)
                    .contains(&value) =>
            {
                let offset = value - Self::SEQUENCE_COMPLETE_BASE;
                sequence_from_index(offset).map_or(TelemetryEventKind::Custom(value), |kind| {
                    TelemetryEventKind::SequenceComplete(kind)
                })
            }
            other => TelemetryEventKind::Custom(other),
        }
    }
}

/// Payloads carried alongside telemetry events.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TelemetryPayload {
    /// No additional metadata accompanies the event.
    None,
    /// Details describing a strap transition.
    Strap(StrapTelemetry),
    /// Metadata about queue-backed strap commands.
    Command(CommandTelemetry),
    /// Summary of a completed strap sequence.
    Sequence(SequenceTelemetry),
}

impl TelemetryPayload {
    /// Convenience constructor when no payload data is needed.
    #[must_use]
    pub const fn none() -> Self {
        TelemetryPayload::None
    }
}

/// Strap transition payload.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StrapTelemetry {
    pub line: StrapId,
    pub action: StrapAction,
    pub elapsed_since_previous: Option<Duration>,
}

impl StrapTelemetry {
    #[must_use]
    pub const fn new(
        line: StrapId,
        action: StrapAction,
        elapsed_since_previous: Option<Duration>,
    ) -> Self {
        Self {
            line,
            action,
            elapsed_since_previous,
        }
    }
}

/// Queue command metadata payload.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CommandTelemetry {
    pub queue_depth: u8,
    pub pending_for: Option<Duration>,
}

impl CommandTelemetry {
    #[must_use]
    pub const fn new(queue_depth: u8, pending_for: Option<Duration>) -> Self {
        Self {
            queue_depth,
            pending_for,
        }
    }
}

/// Sequence completion summary payload.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SequenceTelemetry {
    pub outcome: SequenceOutcome,
    pub duration: Option<Duration>,
    pub events_recorded: u8,
    pub fault: Option<FaultRecoveryTelemetry>,
}

impl SequenceTelemetry {
    #[must_use]
    pub const fn new(
        outcome: SequenceOutcome,
        duration: Option<Duration>,
        events_recorded: u8,
    ) -> Self {
        Self {
            outcome,
            duration,
            events_recorded,
            fault: None,
        }
    }

    /// Attaches fault recovery details to the telemetry payload.
    #[must_use]
    pub const fn with_fault(mut self, details: FaultRecoveryTelemetry) -> Self {
        self.fault = Some(details);
        self
    }
}

/// Total number of telemetry entries retained in memory.
pub const TELEMETRY_RING_CAPACITY: usize = 128;

/// Trait implemented by monotonic instant wrappers used for telemetry tracking.
pub trait TelemetryInstant: Copy {
    /// Returns the saturating duration from `earlier` to `self`.
    fn saturating_duration_since(&self, earlier: Self) -> Duration;
}

/// Telemetry record stored in the ring buffer.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TelemetryRecord<TInstant>
where
    TInstant: Copy,
{
    pub id: EventId,
    pub timestamp: TInstant,
    pub event: TelemetryEventKind,
    pub details: TelemetryPayload,
}

/// Telemetry ring buffer type alias.
pub type TelemetryRing<TInstant, const CAPACITY: usize = TELEMETRY_RING_CAPACITY> =
    HistoryBuf<TelemetryRecord<TInstant>, CAPACITY>;

/// Records telemetry events into a fixed-size ring buffer.
pub struct TelemetryRecorder<TInstant, const CAPACITY: usize = TELEMETRY_RING_CAPACITY>
where
    TInstant: Copy,
{
    ring: TelemetryRing<TInstant, CAPACITY>,
    last_transition_at: Option<TInstant>,
    next_event_id: EventId,
}

impl<TInstant, const CAPACITY: usize> TelemetryRecorder<TInstant, CAPACITY>
where
    TInstant: Copy + TelemetryInstant,
{
    /// Creates a new telemetry recorder with an empty history.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ring: HistoryBuf::new(),
            last_transition_at: None,
            next_event_id: 0,
        }
    }

    /// Returns an iterator over the recorded telemetry in chronological order.
    pub fn oldest_first(&self) -> OldestOrdered<'_, TelemetryRecord<TInstant>> {
        self.ring.oldest_ordered()
    }

    /// Returns the most recent telemetry record, if available.
    pub fn latest(&self) -> Option<&TelemetryRecord<TInstant>> {
        self.ring.recent()
    }

    /// Returns the number of records currently stored.
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// Returns `true` when no telemetry records are stored.
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// Records a strap transition and captures elapsed time since the previous transition.
    pub fn record_strap_transition(
        &mut self,
        line: StrapId,
        action: StrapAction,
        timestamp: TInstant,
    ) -> EventId {
        let elapsed = self
            .last_transition_at
            .map(|previous| timestamp.saturating_duration_since(previous));
        self.last_transition_at = Some(timestamp);

        let payload = TelemetryPayload::Strap(StrapTelemetry::new(line, action, elapsed));
        self.record(
            match action {
                StrapAction::AssertLow => TelemetryEventKind::StrapAsserted(line),
                StrapAction::ReleaseHigh => TelemetryEventKind::StrapReleased(line),
            },
            payload,
            timestamp,
        )
    }

    /// Records a queued command that cannot start immediately.
    pub fn record_command_pending(
        &mut self,
        kind: StrapSequenceKind,
        queue_depth: usize,
        requested_at: TInstant,
        timestamp: TInstant,
    ) -> EventId {
        let wait = timestamp.saturating_duration_since(requested_at);
        let payload = TelemetryPayload::Command(CommandTelemetry::new(
            truncate_depth(queue_depth),
            Some(wait),
        ));

        self.record(TelemetryEventKind::CommandPending(kind), payload, timestamp)
    }

    /// Records the moment a queued command begins execution.
    pub fn record_command_started(
        &mut self,
        kind: StrapSequenceKind,
        queue_depth: usize,
        requested_at: TInstant,
        timestamp: TInstant,
    ) -> EventId {
        let wait = timestamp.saturating_duration_since(requested_at);
        let payload = TelemetryPayload::Command(CommandTelemetry::new(
            truncate_depth(queue_depth),
            Some(wait),
        ));

        self.record(TelemetryEventKind::CommandStarted(kind), payload, timestamp)
    }

    /// Records the completion of a strap sequence run.
    pub fn record_sequence_completion(
        &mut self,
        kind: StrapSequenceKind,
        outcome: SequenceOutcome,
        started_at: Option<TInstant>,
        timestamp: TInstant,
        events_recorded: usize,
    ) -> EventId {
        let duration = started_at.map(|start| timestamp.saturating_duration_since(start));
        let payload = TelemetryPayload::Sequence(SequenceTelemetry::new(
            outcome,
            duration,
            truncate_count(events_recorded),
        ));

        self.record(
            TelemetryEventKind::SequenceComplete(kind),
            payload,
            timestamp,
        )
    }
}

impl<TInstant, const CAPACITY: usize> Default for TelemetryRecorder<TInstant, CAPACITY>
where
    TInstant: Copy + TelemetryInstant,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<TInstant, const CAPACITY: usize> TelemetryRecorder<TInstant, CAPACITY>
where
    TInstant: Copy + TelemetryInstant,
{
    /// Records an arbitrary telemetry event with the supplied payload.
    pub fn record(
        &mut self,
        event: TelemetryEventKind,
        payload: TelemetryPayload,
        timestamp: TInstant,
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

fn truncate_depth(depth: usize) -> u8 {
    match u8::try_from(depth) {
        Ok(value) => value,
        Err(_) => u8::MAX,
    }
}

fn truncate_count(count: usize) -> u8 {
    match u8::try_from(count) {
        Ok(value) => value,
        Err(_) => u8::MAX,
    }
}

/// Encoded details describing a fault recovery attempt.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FaultRecoveryTelemetry {
    pub reason: FaultRecoveryReason,
    pub retries: u8,
}

impl FaultRecoveryTelemetry {
    /// Creates a new fault recovery telemetry payload.
    #[must_use]
    pub const fn new(reason: FaultRecoveryReason, retries: u8) -> Self {
        Self { reason, retries }
    }
}

/// Reason codes recorded when fault recovery runs.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FaultRecoveryReason {
    /// Operator invoked the manual fault recovery command.
    ManualRequest,
    /// Automated recovery triggered after detecting a brown-out.
    BrownOutDetected,
    /// Control USB link dropped during fault handling.
    ControlLinkLost,
    /// Console watchdog timed out waiting for Jetson UART activity.
    ConsoleWatchdogTimeout,
    /// Implementation-specific extension.
    Custom(u8),
}

impl FaultRecoveryReason {
    const MANUAL_REQUEST_CODE: u8 = 0x00;
    const BROWN_OUT_CODE: u8 = 0x01;
    const CONTROL_LINK_LOST_CODE: u8 = 0x02;
    const CONSOLE_WATCHDOG_CODE: u8 = 0x03;

    /// Encodes the reason into a compact numeric discriminant.
    #[must_use]
    pub const fn to_raw(self) -> u8 {
        match self {
            FaultRecoveryReason::ManualRequest => Self::MANUAL_REQUEST_CODE,
            FaultRecoveryReason::BrownOutDetected => Self::BROWN_OUT_CODE,
            FaultRecoveryReason::ControlLinkLost => Self::CONTROL_LINK_LOST_CODE,
            FaultRecoveryReason::ConsoleWatchdogTimeout => Self::CONSOLE_WATCHDOG_CODE,
            FaultRecoveryReason::Custom(code) => code,
        }
    }

    /// Decodes a compact numeric discriminant into a fault recovery reason.
    #[must_use]
    pub const fn from_raw(code: u8) -> Self {
        match code {
            Self::MANUAL_REQUEST_CODE => FaultRecoveryReason::ManualRequest,
            Self::BROWN_OUT_CODE => FaultRecoveryReason::BrownOutDetected,
            Self::CONTROL_LINK_LOST_CODE => FaultRecoveryReason::ControlLinkLost,
            Self::CONSOLE_WATCHDOG_CODE => FaultRecoveryReason::ConsoleWatchdogTimeout,
            other => FaultRecoveryReason::Custom(other),
        }
    }

    /// Returns `true` when the reason was decoded from an unknown code.
    #[must_use]
    pub const fn is_custom(self) -> bool {
        matches!(self, FaultRecoveryReason::Custom(_))
    }
}

const fn strap_index(line: StrapId) -> u16 {
    match line {
        StrapId::Reset => 0,
        StrapId::Rec => 1,
        StrapId::Pwr => 2,
        StrapId::Apo => 3,
    }
}

fn strap_from_index(index: u16) -> Option<StrapId> {
    match index {
        0 => Some(StrapId::Reset),
        1 => Some(StrapId::Rec),
        2 => Some(StrapId::Pwr),
        3 => Some(StrapId::Apo),
        _ => None,
    }
}

const fn sequence_index(kind: StrapSequenceKind) -> u16 {
    match kind {
        StrapSequenceKind::NormalReboot => 0,
        StrapSequenceKind::RecoveryEntry => 1,
        StrapSequenceKind::RecoveryImmediate => 2,
        StrapSequenceKind::FaultRecovery => 3,
    }
}

fn sequence_from_index(index: u16) -> Option<StrapSequenceKind> {
    match index {
        0 => Some(StrapSequenceKind::NormalReboot),
        1 => Some(StrapSequenceKind::RecoveryEntry),
        2 => Some(StrapSequenceKind::RecoveryImmediate),
        3 => Some(StrapSequenceKind::FaultRecovery),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn fault_recovery_reason_round_trip() {
        let fixtures = [
            (FaultRecoveryReason::ManualRequest, 0x00),
            (FaultRecoveryReason::BrownOutDetected, 0x01),
            (FaultRecoveryReason::ControlLinkLost, 0x02),
            (FaultRecoveryReason::ConsoleWatchdogTimeout, 0x03),
            (FaultRecoveryReason::Custom(0xA5), 0xA5),
        ];

        for (reason, code) in fixtures {
            assert_eq!(reason.to_raw(), code);
            match reason {
                FaultRecoveryReason::Custom(_) => {
                    let decoded = FaultRecoveryReason::from_raw(code);
                    assert!(decoded.is_custom());
                    assert_eq!(decoded.to_raw(), code);
                }
                _ => assert_eq!(FaultRecoveryReason::from_raw(code), reason),
            }
        }
    }

    #[test]
    fn sequence_telemetry_attaches_fault_details() {
        let base = SequenceTelemetry::new(SequenceOutcome::Completed, None, 2);
        assert!(base.fault.is_none());

        let details =
            FaultRecoveryTelemetry::new(FaultRecoveryReason::ManualRequest, /* retries */ 1);
        let telemetry = base.with_fault(details);

        assert_eq!(telemetry.fault, Some(details));
        assert_eq!(telemetry.events_recorded, 2);
    }

    #[test]
    fn records_elapsed_between_strap_events() {
        let mut recorder = TelemetryRecorder::<MicrosInstant>::new();

        let id1 = recorder.record_strap_transition(
            StrapId::Reset,
            StrapAction::AssertLow,
            MicrosInstant::from_micros(100),
        );
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

        let id2 = recorder.record_strap_transition(
            StrapId::Reset,
            StrapAction::ReleaseHigh,
            MicrosInstant::from_micros(250),
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
        let mut recorder = TelemetryRecorder::<MicrosInstant>::new();
        let requested_at = MicrosInstant::from_micros(100);
        let timestamp = MicrosInstant::from_micros(220);

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
        let mut recorder = TelemetryRecorder::<MicrosInstant>::new();
        recorder.record_command_pending(
            StrapSequenceKind::RecoveryEntry,
            1,
            MicrosInstant::from_micros(50),
            MicrosInstant::from_micros(60),
        );

        let requested_at = MicrosInstant::from_micros(90);
        let start_time = MicrosInstant::from_micros(500);
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
        let mut recorder = TelemetryRecorder::<MicrosInstant>::new();
        let started_at = MicrosInstant::from_micros(100);
        let completed_at = MicrosInstant::from_micros(1_300);

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
        let mut recorder = TelemetryRecorder::<MicrosInstant>::new();
        let completed_at = MicrosInstant::from_micros(2_000);

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
