//! Orchestrator-facing abstractions shared between firmware and host targets.
//!
//! This module defines the common data structures and traits needed to drive a
//! strap sequence run without depending on a specific executor or queue
//! implementation. Firmware and emulator implementations provide concrete
//! queue/sequence types that satisfy these traits while reusing the shared
//! business logic housed in `controller-core`.

use core::fmt;

use crate::sequences::StrapSequenceKind;

/// Identifier used when tracking emitted telemetry events.
pub type EventId = u32;

/// Source that initiated a [`SequenceCommand`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CommandSource {
    UsbHost,
}

/// Optional flags that customize how a command is executed.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub struct CommandFlags {
    /// Placeholder for future overrides (e.g., forcing recovery paths).
    pub force_recovery: bool,
}

/// Strap sequence request waiting to be processed by the orchestrator.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SequenceCommand<TInstant = u64> {
    pub kind: StrapSequenceKind,
    pub requested_at: TInstant,
    pub source: CommandSource,
    pub flags: CommandFlags,
}

impl<TInstant> SequenceCommand<TInstant> {
    /// Constructs a new command with default flags.
    pub fn new(kind: StrapSequenceKind, requested_at: TInstant, source: CommandSource) -> Self {
        Self {
            kind,
            requested_at,
            source,
            flags: CommandFlags::default(),
        }
    }

    /// Constructs a new command with explicit flags.
    pub fn with_flags(
        kind: StrapSequenceKind,
        requested_at: TInstant,
        source: CommandSource,
        flags: CommandFlags,
    ) -> Self {
        Self {
            kind,
            requested_at,
            source,
            flags,
        }
    }
}

/// Outcome reported when a sequence completes successfully.
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

/// State machine phases for an in-flight sequence run.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SequenceState {
    Idle,
    Arming,
    Executing,
    Cooldown,
    Complete(SequenceOutcome),
    Error(SequenceError),
}

impl SequenceState {
    /// Returns `true` when the sequence can still transition to another state.
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            SequenceState::Arming | SequenceState::Executing | SequenceState::Cooldown
        )
    }

    /// Returns `true` when the state represents a terminal outcome.
    pub const fn is_terminal(self) -> bool {
        matches!(self, SequenceState::Complete(_) | SequenceState::Error(_))
    }
}

/// Failure reported when attempting an invalid state transition.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TransitionError {
    pub from: SequenceState,
    pub to: SequenceState,
}

impl TransitionError {
    /// Creates a new transition error describing the attempted states.
    pub const fn new(from: SequenceState, to: SequenceState) -> Self {
        Self { from, to }
    }
}

/// Error surfaced when a command cannot be enqueued.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CommandEnqueueError<E = ()> {
    /// Queue has reached its maximum capacity.
    QueueFull,
    /// Queue has been disconnected from its consumer.
    Disconnected,
    /// Transport-specific failure.
    Other(E),
}

impl<E> CommandEnqueueError<E> {
    /// Maps the inner error type.
    pub fn map_other<F, M>(self, mapper: M) -> CommandEnqueueError<F>
    where
        M: FnOnce(E) -> F,
    {
        match self {
            CommandEnqueueError::QueueFull => CommandEnqueueError::QueueFull,
            CommandEnqueueError::Disconnected => CommandEnqueueError::Disconnected,
            CommandEnqueueError::Other(err) => CommandEnqueueError::Other(mapper(err)),
        }
    }
}

/// Error surfaced when dequeueing from the command queue fails.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CommandDequeueError<E = ()> {
    /// Queue has been disconnected from its producers.
    Disconnected,
    /// Transport-specific failure.
    Other(E),
}

impl<E> CommandDequeueError<E> {
    /// Maps the inner error type.
    pub fn map_other<F, M>(self, mapper: M) -> CommandDequeueError<F>
    where
        M: FnOnce(E) -> F,
    {
        match self {
            CommandDequeueError::Disconnected => CommandDequeueError::Disconnected,
            CommandDequeueError::Other(err) => CommandDequeueError::Other(mapper(err)),
        }
    }
}

/// Trait implemented by producers that push commands into the orchestrator queue.
pub trait CommandQueueProducer {
    /// Monotonic timestamp type attached to commands.
    type Instant: Copy;
    /// Transport-specific error type.
    type Error;

    /// Attempts to enqueue a command without blocking.
    fn try_enqueue(
        &mut self,
        command: SequenceCommand<Self::Instant>,
    ) -> Result<(), CommandEnqueueError<Self::Error>>;

    /// Returns the queue capacity if it is known at compile- or run-time.
    fn capacity(&self) -> Option<usize> {
        None
    }

    /// Returns the current queue depth if it can be observed.
    fn len(&self) -> Option<usize> {
        None
    }

    /// Convenience helper that reports remaining slots when both capacity and
    /// length information are available.
    fn remaining(&self) -> Option<usize> {
        match (self.capacity(), self.len()) {
            (Some(capacity), Some(len)) => Some(capacity.saturating_sub(len)),
            _ => None,
        }
    }

    /// Returns `true` when the queue reports that it is currently full.
    fn is_full(&self) -> Option<bool> {
        self.remaining().map(|slot_count| slot_count == 0)
    }
}

/// Trait implemented by consumers that pop commands from the orchestrator queue.
pub trait CommandQueueConsumer {
    /// Monotonic timestamp type attached to commands.
    type Instant: Copy;
    /// Transport-specific error type.
    type Error;

    /// Attempts to dequeue a command without blocking.
    ///
    /// Returns `Ok(Some(command))` when a command was available, `Ok(None)` when
    /// the queue is currently empty, or an error when the underlying transport
    /// has been disconnected or failed.
    fn try_dequeue(
        &mut self,
    ) -> Result<Option<SequenceCommand<Self::Instant>>, CommandDequeueError<Self::Error>>;
}

/// Read-only view of an active [`SequenceRun`](SequenceRunControl) state machine.
pub trait SequenceRunView {
    /// Monotonic timestamp type used to track deadlines.
    type Instant: Copy + Ord;
    /// Identifier type used when recording associated telemetry.
    type EventId: Copy;

    /// Returns the command that spawned this run.
    fn command(&self) -> &SequenceCommand<Self::Instant>;

    /// Reports the current state of the run.
    fn state(&self) -> SequenceState;

    /// Returns the number of retry attempts that have occurred.
    fn retry_count(&self) -> u8;

    /// Returns `true` when the run is waiting for bridge activity.
    fn waiting_on_bridge(&self) -> bool;

    /// Provides the sequence start timestamp, if the sequence has begun.
    fn sequence_started_at(&self) -> Option<Self::Instant>;

    /// Returns the index of the active step, if any.
    fn current_step_index(&self) -> Option<usize>;

    /// Provides the deadline for the active step, if any.
    fn step_deadline(&self) -> Option<Self::Instant>;

    /// Provides the deadline for the cooldown period, if any.
    fn cooldown_deadline(&self) -> Option<Self::Instant>;

    /// Lists the telemetry event identifiers that have been emitted so far.
    fn emitted_events(&self) -> &[Self::EventId];

    /// Returns `true` when the run has transitioned into a terminal state.
    fn is_terminal(&self) -> bool {
        self.state().is_terminal()
    }

    /// Returns `true` when the run is actively executing strap steps.
    fn is_executing(&self) -> bool {
        matches!(self.state(), SequenceState::Executing)
    }
}

/// Mutable control surface for a [`SequenceRunView`].
pub trait SequenceRunControl: SequenceRunView {
    /// Updates the state machine, validating the requested transition.
    fn set_state(&mut self, next: SequenceState) -> Result<(), TransitionError>;

    /// Updates the sequence start timestamp.
    fn set_sequence_started_at(&mut self, instant: Option<Self::Instant>);

    /// Updates the current step index.
    fn set_current_step_index(&mut self, index: Option<usize>);

    /// Updates the active step deadline.
    fn set_step_deadline(&mut self, deadline: Option<Self::Instant>);

    /// Updates the active cooldown deadline.
    fn set_cooldown_deadline(&mut self, deadline: Option<Self::Instant>);

    /// Sets whether the run is waiting on bridge activity.
    fn set_waiting_on_bridge(&mut self, waiting: bool);

    /// Records a telemetry event identifier for this run.
    ///
    /// Returns `true` when the event was stored (some implementations may have
    /// bounded capacity).
    fn record_event(&mut self, event_id: Self::EventId) -> bool;

    /// Clears tracked telemetry event identifiers.
    fn clear_events(&mut self);

    /// Increments the retry counter, saturating on overflow.
    fn increment_retry(&mut self);

    /// Resets internal bookkeeping so the run can execute again.
    fn reset_for_retry(&mut self);
}

/// Blanket implementation tying the view and control traits together.
pub trait SequenceRunStateMachine: SequenceRunControl {}

impl<T> SequenceRunStateMachine for T where T: SequenceRunControl {}
