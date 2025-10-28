//! Orchestrator-facing abstractions shared between firmware and host targets.
//!
//! This module defines the common data structures and traits needed to drive a
//! strap sequence run without depending on a specific executor or queue
//! implementation. Firmware and emulator implementations provide concrete
//! queue/sequence types that satisfy these traits while reusing the shared
//! business logic housed in `controller-core`.

use core::{fmt, ops::Add, time::Duration};

use heapless::Vec;

use crate::sequences::{
    SequenceTemplate, StepCompletion, StrapAction, StrapId, StrapSequenceKind, StrapStep,
    normal_reboot_template,
};

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
    /// Optional delay before the orchestrator may start executing the command.
    pub start_after: Option<Duration>,
    /// Optional retry budget override for sequences that support retries.
    pub retry_override: Option<u8>,
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

/// Abstraction over the physical strap drivers.
pub trait StrapDriver {
    /// Applies the requested action to the strap line.
    fn apply(&mut self, line: StrapId, action: StrapAction);

    /// Releases all strap lines to their default state.
    fn release_all(&mut self);
}

/// Strap driver that performs no hardware interaction.
#[derive(Copy, Clone, Debug, Default)]
pub struct NoopStrapDriver;

impl NoopStrapDriver {
    /// Creates a new no-op strap driver.
    pub const fn new() -> Self {
        Self
    }
}

impl StrapDriver for NoopStrapDriver {
    fn apply(&mut self, _: StrapId, _: StrapAction) {}

    fn release_all(&mut self) {}
}

/// High-level orchestrator lifecycle mirrored by firmware tasks.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OrchestratorState {
    Idle,
    Arming,
    Running,
    Cooldown,
    Completed,
    Error,
}

impl OrchestratorState {
    /// Returns `true` when the orchestrator has reached a terminal state.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Error)
    }
}

/// Errors reported when accessing the active run.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ActiveRunError {
    /// No sequence is currently active.
    NoActiveRun,
}

/// Reason a command was rejected by the orchestrator.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CommandRejectionReason {
    Busy,
    MissingTemplate,
    ControlLinkLost,
}

/// Tracks the last command rejection for diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandRejection<TInstant> {
    command: SequenceCommand<TInstant>,
    reason: CommandRejectionReason,
}

impl<TInstant: Copy> CommandRejection<TInstant> {
    const fn new(command: SequenceCommand<TInstant>, reason: CommandRejectionReason) -> Self {
        Self { command, reason }
    }

    /// Builds a BUSY rejection.
    pub fn busy(command: SequenceCommand<TInstant>) -> Self {
        Self::new(command, CommandRejectionReason::Busy)
    }

    /// Builds a rejection caused by a missing template.
    pub fn missing_template(command: SequenceCommand<TInstant>) -> Self {
        Self::new(command, CommandRejectionReason::MissingTemplate)
    }

    /// Builds a rejection caused by a missing control link.
    pub fn control_link_lost(command: SequenceCommand<TInstant>) -> Self {
        Self::new(command, CommandRejectionReason::ControlLinkLost)
    }

    /// Returns the rejection reason.
    pub const fn reason(&self) -> CommandRejectionReason {
        self.reason
    }

    /// Returns the rejected command.
    pub const fn command(&self) -> &SequenceCommand<TInstant> {
        &self.command
    }

    /// Consumes the rejection and returns the command.
    pub fn into_command(self) -> SequenceCommand<TInstant> {
        self.command
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

/// Default retry budget when templates do not provide an override.
pub const DEFAULT_RETRY_BUDGET: u8 = 1;

/// Determines the retry budget for a sequence command.
pub fn retry_budget_for<TInstant>(
    command: &SequenceCommand<TInstant>,
    template: &SequenceTemplate,
) -> u8 {
    command
        .flags
        .retry_override
        .unwrap_or_else(|| template.max_retries.unwrap_or(DEFAULT_RETRY_BUDGET))
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

    /// Returns `true` when the queue reports that it currently holds no items.
    fn is_empty(&self) -> Option<bool> {
        self.len().map(|current| current == 0)
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

/// Total number of distinct [`StrapSequenceKind`] variants.
pub const SEQUENCE_KIND_COUNT: usize = 4;

/// Default timeout used when waiting for bridge activity during recovery.
pub const DEFAULT_BRIDGE_ACTIVITY_TIMEOUT: Duration = Duration::from_secs(10);

/// Configuration controlling how long the orchestrator waits for bridge activity.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BridgeHoldConfig {
    timeout: Duration,
}

impl BridgeHoldConfig {
    /// Creates a configuration with the provided timeout.
    pub const fn new(timeout: Duration) -> Self {
        Self { timeout }
    }

    /// Returns the configured timeout duration.
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Returns `true` when the timeout is non-zero.
    pub const fn has_timeout(&self) -> bool {
        !self.timeout.is_zero()
    }
}

impl Default for BridgeHoldConfig {
    fn default() -> Self {
        Self::new(DEFAULT_BRIDGE_ACTIVITY_TIMEOUT)
    }
}

/// Applies per-step timing semantics, including bridge wait configuration.
pub fn configure_step_timing<R>(
    run: &mut R,
    step: &StrapStep,
    now: R::Instant,
    bridge: &BridgeHoldConfig,
) -> Option<R::Instant>
where
    R: SequenceRunControl,
    R::Instant: Copy + Add<Duration, Output = R::Instant>,
{
    match step.completion {
        StepCompletion::AfterDuration => {
            run.set_waiting_on_bridge(false);
            let deadline = now + step.hold_duration();
            run.set_step_deadline(Some(deadline));
            Some(deadline)
        }
        StepCompletion::OnBridgeActivity => begin_bridge_wait(run, now, bridge),
        StepCompletion::OnEvent(_) => {
            run.set_waiting_on_bridge(false);
            run.set_step_deadline(None);
            None
        }
    }
}

/// Marks the active step as waiting for bridge activity.
pub fn begin_bridge_wait<R>(
    run: &mut R,
    now: R::Instant,
    config: &BridgeHoldConfig,
) -> Option<R::Instant>
where
    R: SequenceRunControl,
    R::Instant: Copy + Add<Duration, Output = R::Instant>,
{
    run.set_waiting_on_bridge(true);

    if !config.has_timeout() {
        run.set_step_deadline(None);
        None
    } else {
        let deadline = now + config.timeout();
        run.set_step_deadline(Some(deadline));
        Some(deadline)
    }
}

/// Clears bridge wait state in response to observed activity.
pub fn satisfy_bridge_wait<R>(run: &mut R)
where
    R: SequenceRunControl,
{
    if run.waiting_on_bridge() {
        run.set_waiting_on_bridge(false);
        run.set_step_deadline(None);
    }
}

/// Returns `true` when the configured bridge wait timeout has elapsed.
pub fn bridge_wait_timed_out<R>(run: &R, now: R::Instant) -> bool
where
    R: SequenceRunView,
{
    if !run.waiting_on_bridge() {
        return false;
    }

    match run.step_deadline() {
        Some(deadline) => now >= deadline,
        None => false,
    }
}

/// Maximum number of templates we expect to register for the controller.
pub const MAX_SEQUENCE_TEMPLATES: usize = SEQUENCE_KIND_COUNT;

/// Registry tracking strap sequence templates by [`StrapSequenceKind`].
#[derive(Clone)]
pub struct TemplateRegistry<const CAPACITY: usize = MAX_SEQUENCE_TEMPLATES> {
    templates: Vec<SequenceTemplate, CAPACITY>,
}

impl<const CAPACITY: usize> TemplateRegistry<CAPACITY> {
    /// Creates an empty registry.
    pub const fn new() -> Self {
        Self {
            templates: Vec::new(),
        }
    }

    /// Registers (or replaces) a template in the registry.
    pub fn register(&mut self, template: SequenceTemplate) -> Result<(), TemplateRegistryError> {
        if let Some(existing) = self
            .templates
            .iter_mut()
            .find(|existing| existing.kind == template.kind)
        {
            *existing = template;
            Ok(())
        } else {
            self.templates
                .push(template)
                .map_err(|_| TemplateRegistryError::RegistryFull)
        }
    }

    /// Looks up a template by kind.
    pub fn get(&self, kind: StrapSequenceKind) -> Option<&SequenceTemplate> {
        self.templates.iter().find(|template| template.kind == kind)
    }

    /// Returns `true` when the registry already contains a template for `kind`.
    pub fn contains(&self, kind: StrapSequenceKind) -> bool {
        self.get(kind).is_some()
    }

    /// Returns the number of registered templates.
    pub fn len(&self) -> usize {
        self.templates.len()
    }

    /// Returns `true` when no templates are stored.
    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    /// Provides an iterator over registered templates.
    pub fn iter(&self) -> core::slice::Iter<'_, SequenceTemplate> {
        self.templates.iter()
    }
}

impl<const CAPACITY: usize> Default for TemplateRegistry<CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that may occur while managing the template registry.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TemplateRegistryError {
    /// Registry has reached [`MAX_SEQUENCE_TEMPLATES`].
    RegistryFull,
}

fn sequence_kind_index(kind: StrapSequenceKind) -> usize {
    match kind {
        StrapSequenceKind::NormalReboot => 0,
        StrapSequenceKind::RecoveryEntry => 1,
        StrapSequenceKind::RecoveryImmediate => 2,
        StrapSequenceKind::FaultRecovery => 3,
    }
}

/// Tracks cooldown deadlines for each sequence kind.
#[derive(Clone, Debug)]
pub struct CooldownTracker<Instant> {
    next_allowed: [Option<Instant>; SEQUENCE_KIND_COUNT],
}

impl<Instant> CooldownTracker<Instant>
where
    Instant: Copy + Ord,
{
    /// Creates a tracker with no cooldowns enforced.
    pub const fn new() -> Self {
        Self {
            next_allowed: [None; SEQUENCE_KIND_COUNT],
        }
    }

    /// Returns the timestamp when the sequence kind may run again, if any.
    pub fn next_allowed(&self, kind: StrapSequenceKind) -> Option<Instant> {
        self.next_allowed[sequence_kind_index(kind)]
    }

    /// Returns `true` when the sequence may start at `now`.
    pub fn is_ready(&self, kind: StrapSequenceKind, now: Instant) -> bool {
        match self.next_allowed(kind) {
            Some(deadline) => now >= deadline,
            None => true,
        }
    }

    /// Clears the cooldown for the given sequence kind.
    pub fn clear(&mut self, kind: StrapSequenceKind) {
        self.next_allowed[sequence_kind_index(kind)] = None;
    }
}

impl<Instant> CooldownTracker<Instant>
where
    Instant: Copy + Ord,
{
    fn update_deadline(&mut self, kind: StrapSequenceKind, deadline: Instant) {
        let slot = &mut self.next_allowed[sequence_kind_index(kind)];
        match slot {
            Some(current) if *current >= deadline => {}
            _ => *slot = Some(deadline),
        }
    }

    /// Records a cooldown deadline starting at `start`.
    pub fn reserve_with_duration(
        &mut self,
        kind: StrapSequenceKind,
        start: Instant,
        cooldown: Duration,
    ) where
        Instant: Add<Duration, Output = Instant>,
    {
        let deadline = start + cooldown;
        self.update_deadline(kind, deadline);
    }
}

impl<Instant> Default for CooldownTracker<Instant>
where
    Instant: Copy + Ord,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that may occur while attempting to enqueue a sequence command.
#[derive(Debug, PartialEq, Eq)]
pub enum ScheduleError<E, Instant> {
    /// Underlying queue rejected the command.
    Queue(CommandEnqueueError<E>),
    /// No template is registered for the requested sequence kind.
    MissingTemplate(StrapSequenceKind),
    /// Sequence is cooling down until the provided timestamp.
    CooldownActive {
        kind: StrapSequenceKind,
        ready_at: Instant,
    },
}

impl<E, Instant> From<CommandEnqueueError<E>> for ScheduleError<E, Instant> {
    fn from(value: CommandEnqueueError<E>) -> Self {
        ScheduleError::Queue(value)
    }
}

/// High-level scheduler that enqueues strap sequences while respecting cooldowns.
pub struct SequenceScheduler<P, const CAPACITY: usize = MAX_SEQUENCE_TEMPLATES>
where
    P: CommandQueueProducer,
{
    producer: P,
    templates: TemplateRegistry<CAPACITY>,
    cooldowns: CooldownTracker<P::Instant>,
}

impl<P, const CAPACITY: usize> SequenceScheduler<P, CAPACITY>
where
    P: CommandQueueProducer,
    P::Instant: Copy + Ord + Add<Duration, Output = P::Instant>,
{
    /// Creates a scheduler that owns the provided queue producer.
    pub fn new(producer: P) -> Self {
        let mut scheduler = Self {
            producer,
            templates: TemplateRegistry::new(),
            cooldowns: CooldownTracker::new(),
        };

        scheduler
            .templates
            .register(normal_reboot_template())
            .expect("default template registration should succeed");

        scheduler
    }

    /// Accesses the underlying queue producer.
    pub fn producer(&self) -> &P {
        &self.producer
    }

    /// Mutably accesses the underlying queue producer.
    pub fn producer_mut(&mut self) -> &mut P {
        &mut self.producer
    }

    /// Returns a read-only view of the template registry.
    pub fn templates(&self) -> &TemplateRegistry<CAPACITY> {
        &self.templates
    }

    /// Returns a mutable view of the template registry.
    pub fn templates_mut(&mut self) -> &mut TemplateRegistry<CAPACITY> {
        &mut self.templates
    }

    /// Returns a read-only view of the cooldown tracker.
    pub fn cooldowns(&self) -> &CooldownTracker<P::Instant> {
        &self.cooldowns
    }

    /// Attempts to enqueue a sequence command.
    pub fn enqueue(
        &mut self,
        kind: StrapSequenceKind,
        requested_at: P::Instant,
        source: CommandSource,
    ) -> Result<(), ScheduleError<P::Error, P::Instant>> {
        self.enqueue_with_flags(kind, requested_at, source, CommandFlags::default())
    }

    /// Attempts to enqueue a sequence command with explicit flags.
    pub fn enqueue_with_flags(
        &mut self,
        kind: StrapSequenceKind,
        requested_at: P::Instant,
        source: CommandSource,
        flags: CommandFlags,
    ) -> Result<(), ScheduleError<P::Error, P::Instant>> {
        let template = self
            .templates
            .get(kind)
            .ok_or(ScheduleError::MissingTemplate(kind))?;

        if let Some(deadline) = self.cooldowns.next_allowed(kind)
            && requested_at < deadline
        {
            return Err(ScheduleError::CooldownActive {
                kind,
                ready_at: deadline,
            });
        }

        let command = SequenceCommand::with_flags(kind, requested_at, source, flags);
        self.producer
            .try_enqueue(command)
            .map_err(ScheduleError::from)?;

        self.cooldowns
            .reserve_with_duration(kind, requested_at, template.cooldown_duration());

        Ok(())
    }

    /// Updates cooldown tracking after a sequence completes.
    pub fn notify_completed(
        &mut self,
        kind: StrapSequenceKind,
        completed_at: P::Instant,
    ) -> Result<(), ScheduleError<P::Error, P::Instant>> {
        let template = self
            .templates
            .get(kind)
            .ok_or(ScheduleError::MissingTemplate(kind))?;

        self.cooldowns
            .reserve_with_duration(kind, completed_at, template.cooldown_duration());

        Ok(())
    }

    /// Clears cooldown state for the provided sequence kind.
    pub fn reset_cooldown(&mut self, kind: StrapSequenceKind) {
        self.cooldowns.clear(kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequences::{Milliseconds, StrapAction, StrapId, TimingConstraintSet};
    use heapless::Vec as HeaplessVec;

    #[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct MockInstant(u64);

    impl MockInstant {
        fn micros(value: u64) -> Self {
            Self(value)
        }

        fn value(self) -> u64 {
            self.0
        }
    }

    impl Add<Duration> for MockInstant {
        type Output = Self;

        fn add(self, rhs: Duration) -> Self::Output {
            Self(self.0 + rhs.as_micros() as u64)
        }
    }

    #[derive(Clone)]
    struct MockQueue {
        capacity: usize,
        commands: HeaplessVec<SequenceCommand<MockInstant>, 8>,
    }

    impl MockQueue {
        fn new(capacity: usize) -> Self {
            Self {
                capacity,
                commands: HeaplessVec::new(),
            }
        }

        fn commands(&self) -> &[SequenceCommand<MockInstant>] {
            &self.commands
        }
    }

    impl CommandQueueProducer for MockQueue {
        type Instant = MockInstant;
        type Error = ();

        fn try_enqueue(
            &mut self,
            command: SequenceCommand<Self::Instant>,
        ) -> Result<(), CommandEnqueueError<Self::Error>> {
            if self.commands.len() >= self.capacity {
                return Err(CommandEnqueueError::QueueFull);
            }

            self.commands
                .push(command)
                .map_err(|_| CommandEnqueueError::QueueFull)
        }

        fn capacity(&self) -> Option<usize> {
            Some(self.capacity)
        }

        fn len(&self) -> Option<usize> {
            Some(self.commands.len())
        }
    }

    #[derive(Clone)]
    struct MockRun {
        command: SequenceCommand<MockInstant>,
        state: SequenceState,
        retry_count: u8,
        waiting_on_bridge: bool,
        sequence_started_at: Option<MockInstant>,
        current_step_index: Option<usize>,
        step_deadline: Option<MockInstant>,
        cooldown_deadline: Option<MockInstant>,
        events: HeaplessVec<EventId, 8>,
    }

    impl MockRun {
        fn new() -> Self {
            Self {
                command: SequenceCommand::new(
                    StrapSequenceKind::NormalReboot,
                    MockInstant::micros(0),
                    CommandSource::UsbHost,
                ),
                state: SequenceState::Executing,
                retry_count: 0,
                waiting_on_bridge: false,
                sequence_started_at: None,
                current_step_index: Some(0),
                step_deadline: None,
                cooldown_deadline: None,
                events: HeaplessVec::new(),
            }
        }
    }

    impl SequenceRunView for MockRun {
        type Instant = MockInstant;
        type EventId = EventId;

        fn command(&self) -> &SequenceCommand<Self::Instant> {
            &self.command
        }

        fn state(&self) -> SequenceState {
            self.state
        }

        fn retry_count(&self) -> u8 {
            self.retry_count
        }

        fn waiting_on_bridge(&self) -> bool {
            self.waiting_on_bridge
        }

        fn sequence_started_at(&self) -> Option<Self::Instant> {
            self.sequence_started_at
        }

        fn current_step_index(&self) -> Option<usize> {
            self.current_step_index
        }

        fn step_deadline(&self) -> Option<Self::Instant> {
            self.step_deadline
        }

        fn cooldown_deadline(&self) -> Option<Self::Instant> {
            self.cooldown_deadline
        }

        fn emitted_events(&self) -> &[Self::EventId] {
            self.events.as_slice()
        }
    }

    impl SequenceRunControl for MockRun {
        fn set_state(&mut self, next: SequenceState) -> Result<(), TransitionError> {
            self.state = next;
            Ok(())
        }

        fn set_sequence_started_at(&mut self, instant: Option<Self::Instant>) {
            self.sequence_started_at = instant;
        }

        fn set_current_step_index(&mut self, index: Option<usize>) {
            self.current_step_index = index;
        }

        fn set_step_deadline(&mut self, deadline: Option<Self::Instant>) {
            self.step_deadline = deadline;
        }

        fn set_cooldown_deadline(&mut self, deadline: Option<Self::Instant>) {
            self.cooldown_deadline = deadline;
        }

        fn set_waiting_on_bridge(&mut self, waiting: bool) {
            self.waiting_on_bridge = waiting;
        }

        fn record_event(&mut self, event_id: Self::EventId) -> bool {
            self.events.push(event_id).is_ok()
        }

        fn clear_events(&mut self) {
            self.events.clear();
        }

        fn increment_retry(&mut self) {
            self.retry_count = self.retry_count.saturating_add(1);
        }

        fn reset_for_retry(&mut self) {
            self.retry_count = 0;
            self.state = SequenceState::Arming;
            self.waiting_on_bridge = false;
            self.sequence_started_at = None;
            self.current_step_index = None;
            self.step_deadline = None;
            self.cooldown_deadline = None;
            self.events.clear();
        }
    }

    #[test]
    fn normal_reboot_registered_by_default() {
        let queue = MockQueue::new(4);
        let scheduler = SequenceScheduler::<MockQueue>::new(queue);
        assert!(
            scheduler
                .templates()
                .contains(StrapSequenceKind::NormalReboot)
        );
    }

    #[test]
    fn enqueue_records_normal_reboot_command() {
        let queue = MockQueue::new(4);
        let mut scheduler = SequenceScheduler::<MockQueue>::new(queue);
        let now = MockInstant::micros(0);

        scheduler
            .enqueue(StrapSequenceKind::NormalReboot, now, CommandSource::UsbHost)
            .expect("enqueue should succeed");

        let commands = scheduler.producer().commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].kind, StrapSequenceKind::NormalReboot);
        assert_eq!(commands[0].requested_at, now);
    }

    #[test]
    fn enqueue_respects_cooldown() {
        let queue = MockQueue::new(4);
        let mut scheduler = SequenceScheduler::<MockQueue>::new(queue);
        let first_at = MockInstant::micros(0);

        scheduler
            .enqueue(
                StrapSequenceKind::NormalReboot,
                first_at,
                CommandSource::UsbHost,
            )
            .expect("first enqueue should succeed");

        let second_at = MockInstant::micros(500_000);
        let result = scheduler.enqueue(
            StrapSequenceKind::NormalReboot,
            second_at,
            CommandSource::UsbHost,
        );

        assert!(matches!(
            result,
            Err(ScheduleError::CooldownActive {
                kind: StrapSequenceKind::NormalReboot,
                ..
            })
        ));
    }

    #[test]
    fn notify_completed_updates_cooldown_deadline() {
        let queue = MockQueue::new(4);
        let mut scheduler = SequenceScheduler::<MockQueue>::new(queue);
        let start = MockInstant::micros(0);

        scheduler
            .enqueue(
                StrapSequenceKind::NormalReboot,
                start,
                CommandSource::UsbHost,
            )
            .expect("first enqueue should succeed");

        let completion = MockInstant::micros(2_500_000);
        scheduler
            .notify_completed(StrapSequenceKind::NormalReboot, completion)
            .expect("completion should update cooldown");

        let next_allowed = scheduler
            .cooldowns()
            .next_allowed(StrapSequenceKind::NormalReboot)
            .expect("cooldown deadline missing");

        assert_eq!(
            next_allowed,
            completion + normal_reboot_template().cooldown_duration()
        );
    }

    #[test]
    fn bridge_wait_sets_flag_and_deadline() {
        let mut run = MockRun::new();
        let config = BridgeHoldConfig::new(Duration::from_secs(5));
        let step = StrapStep::new(
            StrapId::Rec,
            StrapAction::AssertLow,
            Milliseconds::ZERO,
            TimingConstraintSet::unrestricted(),
            StepCompletion::OnBridgeActivity,
        );
        let now = MockInstant::micros(1_000);
        let deadline =
            configure_step_timing(&mut run, &step, now, &config).expect("deadline expected");

        assert!(run.waiting_on_bridge());
        assert_eq!(run.step_deadline(), Some(deadline));
        assert_eq!(deadline, now + config.timeout());
    }

    #[test]
    fn satisfy_bridge_wait_clears_state() {
        let mut run = MockRun::new();
        let config = BridgeHoldConfig::default();
        let step = StrapStep::new(
            StrapId::Rec,
            StrapAction::AssertLow,
            Milliseconds::ZERO,
            TimingConstraintSet::unrestricted(),
            StepCompletion::OnBridgeActivity,
        );

        let now = MockInstant::micros(0);
        configure_step_timing(&mut run, &step, now, &config);
        satisfy_bridge_wait(&mut run);

        assert!(!run.waiting_on_bridge());
        assert!(run.step_deadline().is_none());
    }

    #[test]
    fn bridge_wait_timeout_detects_expiry() {
        let mut run = MockRun::new();
        let config = BridgeHoldConfig::new(Duration::from_secs(2));
        let step = StrapStep::new(
            StrapId::Rec,
            StrapAction::AssertLow,
            Milliseconds::ZERO,
            TimingConstraintSet::unrestricted(),
            StepCompletion::OnBridgeActivity,
        );

        let start = MockInstant::micros(0);
        let deadline =
            configure_step_timing(&mut run, &step, start, &config).expect("deadline expected");

        assert!(!bridge_wait_timed_out(&run, start));
        assert!(bridge_wait_timed_out(&run, deadline));

        let after = MockInstant::micros(deadline.value() + 1);
        assert!(bridge_wait_timed_out(&run, after));
    }

    #[test]
    fn bridge_wait_without_timeout_skips_deadline() {
        let mut run = MockRun::new();
        let config = BridgeHoldConfig::new(Duration::from_secs(0));
        let step = StrapStep::new(
            StrapId::Rec,
            StrapAction::AssertLow,
            Milliseconds::ZERO,
            TimingConstraintSet::unrestricted(),
            StepCompletion::OnBridgeActivity,
        );

        let now = MockInstant::micros(0);
        let deadline = configure_step_timing(&mut run, &step, now, &config);

        assert!(run.waiting_on_bridge());
        assert!(deadline.is_none());
        assert!(run.step_deadline().is_none());
        assert!(!bridge_wait_timed_out(&run, MockInstant::micros(1_000_000)));
    }
}
