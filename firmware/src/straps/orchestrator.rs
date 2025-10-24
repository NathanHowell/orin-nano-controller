//! Strap orchestrator skeleton.
//!
//! This module wires together the strap command queue, sequence templates, and
//! the `SequenceRun` state machine. The skeleton provides just enough structure
//! for future tasks to add real hardware control, telemetry, and error
//! handling logic.

#![allow(dead_code)]

use crate::telemetry::TelemetryRecorder;
use embassy_time::{Duration, Instant, Timer};
use heapless::{Deque, Vec};

use super::{
    CommandReceiver, EventId, SequenceCommand, SequenceError, SequenceOutcome, SequenceRun,
    SequenceState, SequenceTemplate, StrapSequenceKind, COMMAND_QUEUE_DEPTH,
};

/// Total number of sequence templates expected for this controller.
pub const MAX_SEQUENCE_TEMPLATES: usize = 4;

/// Registry tracking strap sequence templates by [`StrapSequenceKind`].
#[derive(Default)]
pub struct TemplateRegistry {
    templates: Vec<SequenceTemplate, MAX_SEQUENCE_TEMPLATES>,
}

impl TemplateRegistry {
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

    /// Looks up a mutable template reference by kind.
    pub fn get_mut(&mut self, kind: StrapSequenceKind) -> Option<&mut SequenceTemplate> {
        self.templates
            .iter_mut()
            .find(|template| template.kind == kind)
    }

    /// Returns `true` when a template exists for the given kind.
    pub fn contains(&self, kind: StrapSequenceKind) -> bool {
        self.get(kind).is_some()
    }

    /// Returns the number of registered templates.
    pub fn len(&self) -> usize {
        self.templates.len()
    }

    /// Returns an iterator over registered templates.
    pub fn iter(&self) -> core::slice::Iter<'_, SequenceTemplate> {
        self.templates.iter()
    }
}

/// Errors that may occur while managing the template registry.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TemplateRegistryError {
    /// Registry has reached [`MAX_SEQUENCE_TEMPLATES`].
    RegistryFull,
}

#[derive(Clone, Debug)]
struct QueuedCommand {
    command: SequenceCommand,
    pending_event: Option<EventId>,
}

/// High-level orchestrator states mirroring the data-model FSM.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OrchestratorState {
    /// No active sequence run; waiting on the command queue.
    Idle,
    /// Performing pre-sequence checks and strap preparation.
    Arming,
    /// Actively executing strap steps.
    Running,
    /// Enforcing the post-sequence cooldown.
    Cooldown,
    /// Sequence finished successfully and awaits cleanup.
    Completed,
    /// Sequence terminated with an error.
    Error,
}

impl OrchestratorState {
    /// Returns `true` when the state is terminal.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Error)
    }
}

/// Reason a command was rejected by the orchestrator.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CommandRejectionReason {
    Busy,
    MissingTemplate,
}

/// Command rejection detail for the last failed enqueue attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandRejection {
    command: SequenceCommand,
    reason: CommandRejectionReason,
}

impl CommandRejection {
    fn new(command: SequenceCommand, reason: CommandRejectionReason) -> Self {
        Self { command, reason }
    }

    /// Builds a BUSY rejection.
    pub fn busy(command: SequenceCommand) -> Self {
        Self::new(command, CommandRejectionReason::Busy)
    }

    /// Builds a rejection caused by a missing template.
    pub fn missing_template(command: SequenceCommand) -> Self {
        Self::new(command, CommandRejectionReason::MissingTemplate)
    }

    /// Returns the rejection reason.
    pub const fn reason(&self) -> CommandRejectionReason {
        self.reason
    }

    /// Returns the command that was rejected.
    pub fn command(&self) -> &SequenceCommand {
        &self.command
    }

    /// Unwraps the rejected command.
    pub fn into_command(self) -> SequenceCommand {
        self.command
    }
}

/// Error returned when attempting to transition the active sequence.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TransitionError {
    /// No sequence is currently active.
    NoActiveRun,
}

/// Coordinates strap sequencing based on queued commands.
pub struct StrapOrchestrator<'a> {
    command_rx: CommandReceiver<'a>,
    templates: TemplateRegistry,
    active_run: Option<SequenceRun>,
    last_rejection: Option<CommandRejection>,
    pending_commands: Deque<QueuedCommand, COMMAND_QUEUE_DEPTH>,
}

impl<'a> StrapOrchestrator<'a> {
    /// Creates a new orchestrator with an empty template registry.
    pub fn new(command_rx: CommandReceiver<'a>) -> Self {
        Self {
            command_rx,
            templates: TemplateRegistry::new(),
            active_run: None,
            last_rejection: None,
            pending_commands: Deque::new(),
        }
    }

    /// Creates a new orchestrator seeded with templates.
    pub fn with_templates(command_rx: CommandReceiver<'a>, templates: TemplateRegistry) -> Self {
        Self {
            command_rx,
            templates,
            active_run: None,
            last_rejection: None,
            pending_commands: Deque::new(),
        }
    }

    /// Returns the current orchestrator state.
    pub fn state(&self) -> OrchestratorState {
        match &self.active_run {
            None => OrchestratorState::Idle,
            Some(run) => match run.state {
                SequenceState::Idle => OrchestratorState::Idle,
                SequenceState::Arming => OrchestratorState::Arming,
                SequenceState::Executing => OrchestratorState::Running,
                SequenceState::Cooldown => OrchestratorState::Cooldown,
                SequenceState::Complete(_) => OrchestratorState::Completed,
                SequenceState::Error(_) => OrchestratorState::Error,
            },
        }
    }

    /// Returns a reference to the template registry.
    pub fn templates(&self) -> &TemplateRegistry {
        &self.templates
    }

    /// Returns a mutable reference to the template registry.
    pub fn templates_mut(&mut self) -> &mut TemplateRegistry {
        &mut self.templates
    }

    /// Returns the currently active sequence run, if present.
    pub fn active_run(&self) -> Option<&SequenceRun> {
        self.active_run.as_ref()
    }

    /// Returns a mutable handle to the active sequence run.
    pub fn active_run_mut(&mut self) -> Option<&mut SequenceRun> {
        self.active_run.as_mut()
    }

    /// Returns the sequence template for the active command, if available.
    pub fn active_template(&self) -> Option<&SequenceTemplate> {
        self.active_run
            .as_ref()
            .and_then(|run| self.templates.get(run.command.kind))
    }

    /// Returns the last command rejection, if any.
    pub fn last_rejection(&self) -> Option<&CommandRejection> {
        self.last_rejection.as_ref()
    }

    /// Clears and returns the last command rejection.
    pub fn take_last_rejection(&mut self) -> Option<CommandRejection> {
        self.last_rejection.take()
    }

    /// Begins executing a new sequence command.
    pub fn begin_run(&mut self, command: SequenceCommand) -> Result<(), CommandRejection> {
        if self.active_run.is_some() {
            return Err(CommandRejection::busy(command));
        }

        if !self.templates.contains(command.kind) {
            return Err(CommandRejection::missing_template(command));
        }

        self.active_run = Some(SequenceRun::new(command));
        Ok(())
    }

    /// Finishes the active run and returns to idle.
    pub fn finish_run(&mut self) {
        self.active_run = None;
    }

    /// Marks the active run as completed.
    pub fn complete_run(&mut self, outcome: SequenceOutcome) -> Result<(), TransitionError> {
        if let Some(run) = self.active_run.as_mut() {
            run.state = SequenceState::Complete(outcome);
            Ok(())
        } else {
            Err(TransitionError::NoActiveRun)
        }
    }

    /// Marks the active run as failed.
    pub fn fail_run(&mut self, error: SequenceError) -> Result<(), TransitionError> {
        if let Some(run) = self.active_run.as_mut() {
            run.state = SequenceState::Error(error);
            Ok(())
        } else {
            Err(TransitionError::NoActiveRun)
        }
    }

    /// Updates the state of the active sequence run.
    pub fn transition_to(&mut self, next: SequenceState) -> Result<(), TransitionError> {
        if let Some(run) = self.active_run.as_mut() {
            run.state = next;
            Ok(())
        } else {
            Err(TransitionError::NoActiveRun)
        }
    }

    /// Handles the intake of commands and basic lifecycle management.
    pub async fn run(mut self, telemetry: &mut TelemetryRecorder) -> ! {
        loop {
            if self.active_run.is_some() {
                self.collect_pending_commands(telemetry);
                self.process_active_run().await;
                continue;
            }

            if let Some(queued) = self.pending_commands.pop_front() {
                match self.start_queued_command(queued, telemetry) {
                    Ok(()) => self.last_rejection = None,
                    Err(rejection) => self.last_rejection = Some(rejection),
                }
                continue;
            }

            let command = self.command_rx.receive().await;
            let queued = QueuedCommand {
                command,
                pending_event: None,
            };

            match self.start_queued_command(queued, telemetry) {
                Ok(()) => self.last_rejection = None,
                Err(rejection) => self.last_rejection = Some(rejection),
            }
        }
    }

    async fn process_active_run(&mut self) {
        if self.state().is_terminal() {
            self.finish_run();
            return;
        }

        // Yield to allow other tasks to interact with the active run, e.g.,
        // advancing strap steps or performing safety checks.
        Timer::after(Duration::from_millis(1)).await;
    }

    fn collect_pending_commands(&mut self, telemetry: &mut TelemetryRecorder) {
        while let Ok(command) = self.command_rx.try_receive() {
            if self.pending_commands.is_full() {
                self.last_rejection = Some(CommandRejection::busy(command));
                continue;
            }

            let timestamp = Instant::now();
            let queue_depth = self.pending_commands.len();
            let event_id = telemetry.record_command_pending(
                command.kind,
                queue_depth,
                command.requested_at,
                timestamp,
            );

            let queued = QueuedCommand {
                command,
                pending_event: Some(event_id),
            };

            debug_assert!(
                self.pending_commands.push_back(queued).is_ok(),
                "pending command queue overflow"
            );
        }
    }

    fn start_queued_command(
        &mut self,
        queued: QueuedCommand,
        telemetry: &mut TelemetryRecorder,
    ) -> Result<(), CommandRejection> {
        let pending_event = queued.pending_event;
        let command = queued.command;

        match self.begin_run(command) {
            Ok(()) => {
                if let Some(run) = self.active_run.as_mut() {
                    if let Some(event_id) = pending_event {
                        let _ = run.track_event(event_id);
                    }

                    let start_event = telemetry.record_command_started(
                        run.command.kind,
                        self.pending_commands.len(),
                        run.command.requested_at,
                        Instant::now(),
                    );
                    let _ = run.track_event(start_event);
                }

                Ok(())
            }
            Err(rejection) => {
                if rejection.reason() == CommandRejectionReason::Busy {
                    let queued = QueuedCommand {
                        command: rejection.command().clone(),
                        pending_event,
                    };

                    // If reinsertion fails the queue is full; drop the request after flagging busy.
                    if self.pending_commands.push_front(queued).is_err() {
                        self.last_rejection =
                            Some(CommandRejection::busy(rejection.command().clone()));
                    }
                }

                Err(rejection)
            }
        }
    }
}
