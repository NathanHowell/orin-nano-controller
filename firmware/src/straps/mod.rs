//! Strap control surface bridging firmware tasks with `controller-core`.

pub mod orchestrator;

pub use controller_core::telemetry::TelemetryEventKind;
use controller_core::telemetry::TelemetryInstant;
use controller_core::{orchestrator as core_orch, sequences as core_seq};
#[cfg(not(target_os = "none"))]
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
#[cfg(target_os = "none")]
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use core::convert::TryFrom;
use embassy_sync::channel::{Channel, Receiver, Sender, TrySendError};
use embassy_time::{Duration as EmbassyDuration, Instant as EmbassyInstant};

pub use core_orch::{EventId, SequenceError, SequenceOutcome, SequenceState};
pub use core_seq::{
    ALL_STRAPS, SequenceTemplate, StepCompletion, StrapAction, StrapId, StrapLine,
    StrapSequenceKind, StrapStep, strap_by_id,
};

use core::ops::Add;

/// Depth of the command queue shared between producers and the orchestrator.
pub const COMMAND_QUEUE_DEPTH: usize = 4;

#[cfg(target_os = "none")]
type StrapMutex = ThreadModeRawMutex;
#[cfg(not(target_os = "none"))]
type StrapMutex = NoopRawMutex;

/// Type alias binding the shared `SequenceCommand` to Embassy's monotonic instant.
pub type SequenceCommand = core_orch::SequenceCommand<FirmwareInstant>;

/// Queue used to coordinate strap sequence commands.
#[cfg_attr(not(target_os = "none"), allow(dead_code))]
pub type CommandQueue = Channel<StrapMutex, SequenceCommand, COMMAND_QUEUE_DEPTH>;

/// Convenience sender type alias for the strap command queue.
pub type CommandSender<'a> = Sender<'a, StrapMutex, SequenceCommand, COMMAND_QUEUE_DEPTH>;

/// Convenience receiver type alias for the strap command queue.
pub type CommandReceiver<'a> = Receiver<'a, StrapMutex, SequenceCommand, COMMAND_QUEUE_DEPTH>;

/// Wrapper around `embassy_time::Instant` that implements the traits expected by controller-core.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct FirmwareInstant(pub EmbassyInstant);

impl FirmwareInstant {
    /// Converts the wrapper back into an Embassy instant.
    pub fn into_embassy(self) -> EmbassyInstant {
        self.0
    }
}

impl From<EmbassyInstant> for FirmwareInstant {
    fn from(value: EmbassyInstant) -> Self {
        Self(value)
    }
}

impl From<FirmwareInstant> for EmbassyInstant {
    fn from(value: FirmwareInstant) -> Self {
        value.0
    }
}

impl Add<core::time::Duration> for FirmwareInstant {
    type Output = Self;

    fn add(self, rhs: core::time::Duration) -> Self::Output {
        let clamped = rhs.as_micros().min(u128::from(u64::MAX));
        let micros = u64::try_from(clamped).unwrap_or(u64::MAX);
        let delta = EmbassyDuration::from_micros(micros);
        Self(self.0 + delta)
    }
}

impl TelemetryInstant for FirmwareInstant {
    fn saturating_duration_since(&self, earlier: Self) -> core::time::Duration {
        let delta = self.0.saturating_duration_since(earlier.0);
        core::time::Duration::from_micros(delta.as_micros())
    }
}

/// Runtime state for an executing strap sequence.
pub type SequenceRun = core_orch::SequenceRun<FirmwareInstant>;

/// Adapter that exposes the Embassy channel sender as a `controller-core` producer.
#[allow(dead_code)]
pub struct CommandProducer<'a> {
    sender: CommandSender<'a>,
}

#[allow(dead_code)]
impl<'a> CommandProducer<'a> {
    /// Creates a new adapter that wraps the provided sender.
    pub fn new(sender: CommandSender<'a>) -> Self {
        Self { sender }
    }

    /// Returns an immutable handle to the underlying sender.
    pub fn inner(&self) -> &CommandSender<'a> {
        &self.sender
    }

    /// Returns a mutable handle to the underlying sender.
    pub fn inner_mut(&mut self) -> &mut CommandSender<'a> {
        &mut self.sender
    }

    /// Consumes the adapter and returns the wrapped sender.
    pub fn into_inner(self) -> CommandSender<'a> {
        self.sender
    }
}

impl core_orch::CommandQueueProducer for CommandProducer<'_> {
    type Instant = FirmwareInstant;
    type Error = ();

    fn try_enqueue(
        &mut self,
        command: SequenceCommand,
    ) -> Result<(), core_orch::CommandEnqueueError<Self::Error>> {
        match self.sender.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(core_orch::CommandEnqueueError::QueueFull),
        }
    }

    fn capacity(&self) -> Option<usize> {
        Some(COMMAND_QUEUE_DEPTH)
    }
}
