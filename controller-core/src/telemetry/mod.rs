//! Telemetry event catalog and payload structures shared by firmware and host targets.
//!
//! The data model mirrors the documentation in
//! `specs/001-build-orin-controller/data-model.md`, providing strongly typed
//! event kinds that can be serialized to compact numeric codes for transport
//! over diagnostics channels. Payload enums carry the extra metadata required
//! by the REPL and evidence capture tooling while remaining `no_std`
//! compatible.

#![cfg_attr(not(test), allow(dead_code))]

use core::time::Duration;

use heapless::Vec;

use crate::orchestrator::SequenceOutcome;
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
    pub const fn with_fault(mut self, details: FaultRecoveryTelemetry) -> Self {
        self.fault = Some(details);
        self
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
}
