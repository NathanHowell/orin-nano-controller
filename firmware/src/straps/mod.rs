//! Strap control surface bridging firmware tasks with `controller-core`.

pub mod orchestrator;

pub use controller_core::telemetry::TelemetryEventKind;
use controller_core::{orchestrator as core_orch, sequences as core_seq};
#[cfg(not(target_os = "none"))]
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
#[cfg(target_os = "none")]
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender, TrySendError};
use embassy_time::Instant;
use heapless::Vec;

pub use core_orch::{EventId, SequenceError, SequenceOutcome, SequenceState};
pub use core_seq::{
    ALL_STRAPS, SequenceTemplate, StepCompletion, StrapAction, StrapId, StrapLine,
    StrapSequenceKind, StrapStep, strap_by_id,
};

/// Depth of the command queue shared between producers and the orchestrator.
pub const COMMAND_QUEUE_DEPTH: usize = 4;

/// Maximum number of telemetry events tracked for a single sequence run.
pub const MAX_EMITTED_EVENTS: usize = 16;

#[cfg(target_os = "none")]
type StrapMutex = ThreadModeRawMutex;
#[cfg(not(target_os = "none"))]
type StrapMutex = NoopRawMutex;

/// Type alias binding the shared `SequenceCommand` to Embassy's monotonic instant.
pub type SequenceCommand = core_orch::SequenceCommand<Instant>;

/// Queue used to coordinate strap sequence commands.
pub type CommandQueue = Channel<StrapMutex, SequenceCommand, COMMAND_QUEUE_DEPTH>;

/// Convenience sender type alias for the strap command queue.
pub type CommandSender<'a> = Sender<'a, StrapMutex, SequenceCommand, COMMAND_QUEUE_DEPTH>;

/// Convenience receiver type alias for the strap command queue.
pub type CommandReceiver<'a> = Receiver<'a, StrapMutex, SequenceCommand, COMMAND_QUEUE_DEPTH>;

/// Runtime state for an executing strap sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub struct SequenceRun {
    pub command: SequenceCommand,
    pub state: SequenceState,
    pub emitted_events: Vec<EventId, MAX_EMITTED_EVENTS>,
    pub retry_count: u8,
    pub waiting_on_bridge: bool,
    pub sequence_started_at: Option<Instant>,
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
            sequence_started_at: None,
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
        self.sequence_started_at = None;
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
    #[allow(dead_code)]
    pub fn current_step_index(&self) -> Option<usize> {
        self.current_step_index
    }

    /// Returns the deadline for the in-flight step, if any.
    #[allow(dead_code)]
    pub fn step_deadline(&self) -> Option<Instant> {
        self.step_deadline
    }

    /// Returns the deadline for the active cooldown interval, if any.
    #[allow(dead_code)]
    pub fn cooldown_deadline(&self) -> Option<Instant> {
        self.cooldown_deadline
    }
}

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

impl<'a> core_orch::CommandQueueProducer for CommandProducer<'a> {
    type Instant = Instant;
    type Error = TrySendError<SequenceCommand>;

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
