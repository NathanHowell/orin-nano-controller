//! Strap control data structures.
//!
//! These types model the strap hardware surface as described in
//! `specs/001-build-orin-controller/data-model.md` and provide the shared
//! abstractions required by the future orchestrator implementation.

#![allow(dead_code)]

pub mod orchestrator;
pub mod sequences;

use core::fmt;

use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::{Duration, Instant};
use heapless::Vec;

#[cfg(not(target_os = "none"))]
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
#[cfg(target_os = "none")]
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;

#[cfg(target_os = "none")]
type StrapMutex = ThreadModeRawMutex;
#[cfg(not(target_os = "none"))]
type StrapMutex = NoopRawMutex;

/// Maximum number of steps allowed in a single strap sequence.
///
/// The longest planned template (`FaultRecovery`) insists on seven strap actions
/// (APO pre-hold, REC pre/post windows, RESET pulse, PWR cycle). Keeping a bit
/// of headroom lands on eight steps, which trims the `SequenceTemplate` footprint
/// enough for clippy's `result_large_err` lint without constraining upcoming
/// sequences.
pub const MAX_SEQUENCE_STEPS: usize = 8;

/// Maximum number of telemetry events tracked for a single sequence run.
pub const MAX_EMITTED_EVENTS: usize = 16;

/// Depth of the command queue shared between producers and the orchestrator.
pub const COMMAND_QUEUE_DEPTH: usize = 4;

/// Represents the logical strap lines that the controller can drive.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StrapLineId {
    Reset,
    Recovery,
    Power,
    Apo,
}

/// Enumerates MCU pins connected to strap control lines.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum McuPin {
    Pa2,
    Pa3,
    Pa4,
    Pa5,
}

/// Enumerates the open-drain driver channels (SN74LVC07) associated with straps.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DriverChannel {
    Channel1Y,
    Channel2Y,
    Channel3Y,
    Channel4Y,
    Channel5Y,
    Channel6Y,
}

impl DriverChannel {
    /// Human-readable identifier matching the schematic annotation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Channel1Y => "1Y",
            Self::Channel2Y => "2Y",
            Self::Channel3Y => "3Y",
            Self::Channel4Y => "4Y",
            Self::Channel5Y => "5Y",
            Self::Channel6Y => "6Y",
        }
    }
}

/// Strap drive polarity.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StrapPolarity {
    ActiveLow,
    ActiveHigh,
}

/// Logical default state for a strap output.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StrapDefaultState {
    ReleasedHigh,
    AssertedLow,
}

/// Metadata describing a single strap line.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct StrapLine {
    pub id: StrapLineId,
    pub mcu_pin: McuPin,
    pub driver_channel: DriverChannel,
    pub j14_pin: u8,
    pub polarity: StrapPolarity,
    pub default_state: StrapDefaultState,
}

impl StrapLine {
    /// Creates a new [`StrapLine`] definition.
    pub const fn new(
        id: StrapLineId,
        mcu_pin: McuPin,
        driver_channel: DriverChannel,
        j14_pin: u8,
        polarity: StrapPolarity,
        default_state: StrapDefaultState,
    ) -> Self {
        Self {
            id,
            mcu_pin,
            driver_channel,
            j14_pin,
            polarity,
            default_state,
        }
    }
}

/// Static strap lookup table keyed by [`StrapLineId`].
pub const STRAP_LINES: [StrapLine; 4] = [
    StrapLine::new(
        StrapLineId::Reset,
        McuPin::Pa4,
        DriverChannel::Channel2Y,
        8,
        StrapPolarity::ActiveLow,
        StrapDefaultState::ReleasedHigh,
    ),
    StrapLine::new(
        StrapLineId::Recovery,
        McuPin::Pa3,
        DriverChannel::Channel1Y,
        10,
        StrapPolarity::ActiveLow,
        StrapDefaultState::ReleasedHigh,
    ),
    StrapLine::new(
        StrapLineId::Power,
        McuPin::Pa2,
        DriverChannel::Channel2Y,
        12,
        StrapPolarity::ActiveLow,
        StrapDefaultState::ReleasedHigh,
    ),
    StrapLine::new(
        StrapLineId::Apo,
        McuPin::Pa5,
        DriverChannel::Channel1Y,
        5,
        StrapPolarity::ActiveLow,
        StrapDefaultState::ReleasedHigh,
    ),
];

/// Strap action performed during a sequence step.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StrapAction {
    AssertLow,
    ReleaseHigh,
}

/// Kinds of strap sequences supported by the controller.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StrapSequenceKind {
    NormalReboot,
    RecoveryEntry,
    RecoveryImmediate,
    FaultRecovery,
}

/// Encodes additional timing constraints for a strap step.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub struct TimingConstraintSet {
    /// Minimum duration the strap must remain in the requested state.
    pub min_hold: Option<Duration>,
    /// Maximum duration the strap may remain in the requested state.
    pub max_hold: Option<Duration>,
    /// Earliest time the strap action may begin relative to the template anchor.
    pub earliest_start: Option<Duration>,
    /// Latest allowed completion time relative to the template anchor.
    pub latest_completion: Option<Duration>,
}

/// Step completion conditions.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StepCompletion {
    /// Advance immediately after the requested duration elapses.
    AfterDuration,
    /// Wait for bridge activity (UART console) before advancing.
    OnBridgeActivity,
    /// Wait until a telemetry event is observed.
    OnEvent(TelemetryEventKind),
}

/// Telemetry event kinds relevant to strap sequencing.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TelemetryEventKind {
    StrapAsserted(StrapLineId),
    StrapReleased(StrapLineId),
    PowerStable,
    RecoveryConsoleActivity,
    CommandPending(StrapSequenceKind),
    CommandStarted(StrapSequenceKind),
    SequenceComplete(StrapSequenceKind),
    UsbDisconnect,
    Custom(u16),
}

/// Single step within a strap sequence template.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StrapStep {
    pub line: StrapLineId,
    pub action: StrapAction,
    pub hold_for: Duration,
    pub constraints: TimingConstraintSet,
    pub completion: StepCompletion,
}

impl StrapStep {
    /// Convenience constructor for a basic timing-only step.
    pub fn timed(line: StrapLineId, action: StrapAction, hold_for: Duration) -> Self {
        Self {
            line,
            action,
            hold_for,
            constraints: TimingConstraintSet::default(),
            completion: StepCompletion::AfterDuration,
        }
    }
}

/// Template describing a strap sequence used by the orchestrator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SequenceTemplate {
    pub kind: StrapSequenceKind,
    pub phases: Vec<StrapStep, MAX_SEQUENCE_STEPS>,
    pub cooldown: Duration,
    pub max_retries: Option<u8>,
}

impl SequenceTemplate {
    /// Creates an empty template for the specified sequence kind.
    pub fn new(kind: StrapSequenceKind, cooldown: Duration, max_retries: Option<u8>) -> Self {
        Self {
            kind,
            phases: Vec::new(),
            cooldown,
            max_retries,
        }
    }
}

/// Source for strap sequence commands.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CommandSource {
    UsbHost,
}

/// Flags carried alongside a strap sequence command.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub struct CommandFlags {
    pub force_recovery: bool,
    /// Optional delay before the orchestrator may start executing the command.
    pub start_after: Option<Duration>,
}

/// Command describing a request to execute a strap sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SequenceCommand {
    pub kind: StrapSequenceKind,
    pub requested_at: Instant,
    pub source: CommandSource,
    pub flags: CommandFlags,
}

impl SequenceCommand {
    /// Creates a new command for the given sequence kind.
    pub fn new(kind: StrapSequenceKind, requested_at: Instant, source: CommandSource) -> Self {
        Self {
            kind,
            requested_at,
            source,
            flags: CommandFlags::default(),
        }
    }
}

/// Identifier used when tracking emitted telemetry events.
pub type EventId = u32;

/// Sequence run state machine phases.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SequenceState {
    Idle,
    Arming,
    Executing,
    Cooldown,
    Complete(SequenceOutcome),
    Error(SequenceError),
}

/// Outcome reported when a sequence finishes successfully.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SequenceOutcome {
    Completed,
    SkippedCooldown,
}

/// Error detail captured when a sequence fails.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SequenceError {
    Busy,
    BrownOutDetected,
    BridgeTimeout,
    RetryLimitExceeded,
    ControlLinkLost,
    UnexpectedState,
    TelemetryBacklog,
}

impl fmt::Display for SequenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Runtime state for an executing strap sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SequenceRun {
    pub command: SequenceCommand,
    pub state: SequenceState,
    pub emitted_events: Vec<EventId, MAX_EMITTED_EVENTS>,
    pub retry_count: u8,
    pub waiting_on_bridge: bool,
    pub(super) current_step_index: Option<usize>,
    pub(super) step_started_at: Option<Instant>,
    pub(super) step_deadline: Option<Instant>,
    pub(super) cooldown_deadline: Option<Instant>,
}

impl SequenceRun {
    /// Creates a new [`SequenceRun`] in the arming state.
    pub fn new(command: SequenceCommand) -> Self {
        Self {
            command,
            state: SequenceState::Arming,
            emitted_events: Vec::new(),
            retry_count: 0,
            waiting_on_bridge: false,
            current_step_index: None,
            step_started_at: None,
            step_deadline: None,
            cooldown_deadline: None,
        }
    }

    /// Resets telemetry tracking for a retry attempt.
    pub fn begin_retry(&mut self) {
        self.retry_count = self.retry_count.saturating_add(1);
        self.emitted_events.clear();
        self.waiting_on_bridge = false;
        self.state = SequenceState::Arming;
        self.current_step_index = None;
        self.step_started_at = None;
        self.step_deadline = None;
        self.cooldown_deadline = None;
    }

    /// Records a telemetry event identifier associated with this run.
    pub fn track_event(&mut self, event_id: EventId) -> bool {
        self.emitted_events.push(event_id).is_ok()
    }

    /// Returns the index of the currently executing step, if any.
    pub fn current_step_index(&self) -> Option<usize> {
        self.current_step_index
    }

    /// Returns the deadline for the in-flight step, if any.
    pub fn step_deadline(&self) -> Option<Instant> {
        self.step_deadline
    }

    /// Returns the deadline for the active cooldown interval, if any.
    pub fn cooldown_deadline(&self) -> Option<Instant> {
        self.cooldown_deadline
    }
}

/// Queue used to coordinate strap sequence commands.
pub type CommandQueue = Channel<StrapMutex, SequenceCommand, COMMAND_QUEUE_DEPTH>;

/// Convenience sender type alias for the strap command queue.
pub type CommandSender<'a> = Sender<'a, StrapMutex, SequenceCommand, COMMAND_QUEUE_DEPTH>;

/// Convenience receiver type alias for the strap command queue.
pub type CommandReceiver<'a> = Receiver<'a, StrapMutex, SequenceCommand, COMMAND_QUEUE_DEPTH>;
