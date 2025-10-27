//! High-level REPL command dispatcher.
//!
//! This module glues the parsed grammar tokens to orchestrator scheduling by
//! turning commands into strap sequence requests. It stays `no_std` friendly so
//! the firmware and emulator crates can share the same implementation.

use core::ops::Add;
use core::time::Duration;

use crate::orchestrator::{
    CommandFlags, CommandQueueProducer, CommandSource, ScheduleError, SequenceScheduler,
};
use crate::sequences::{StrapSequenceKind, fault::FAULT_RECOVERY_MAX_RETRIES};

use super::grammar::{self, Command, RebootCommand, RecoveryCommand};

/// Command execution successes.
#[derive(Clone, Debug, PartialEq)]
pub enum CommandOutcome<Instant> {
    Reboot(RebootAck<Instant>),
    Recovery(RecoveryAck<Instant>),
    Fault(FaultAck<Instant>),
}

/// Summary returned after queueing a reboot command.
#[derive(Clone, Debug, PartialEq)]
pub struct RebootAck<Instant> {
    pub requested_at: Instant,
    pub start_after: Option<Duration>,
}

/// Summary returned after queueing a recovery command.
#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryAck<Instant> {
    pub requested_at: Instant,
    pub sequence: StrapSequenceKind,
    pub command: RecoveryCommand,
}

/// Summary returned after queueing a fault recovery command.
#[derive(Clone, Debug, PartialEq)]
pub struct FaultAck<Instant> {
    pub requested_at: Instant,
    pub sequence: StrapSequenceKind,
    pub retry_budget: u8,
}

/// Errors surfaced while executing a command.
#[derive(Debug, PartialEq)]
pub enum CommandError<'a, E, Instant> {
    Parse(grammar::ParseError<'a>),
    Unsupported(&'static str),
    Schedule(ScheduleError<E, Instant>),
}

impl<'a, E, Instant> From<grammar::ParseError<'a>> for CommandError<'a, E, Instant> {
    fn from(error: grammar::ParseError<'a>) -> Self {
        Self::Parse(error)
    }
}

impl<'a, E, Instant> From<ScheduleError<E, Instant>> for CommandError<'a, E, Instant> {
    fn from(error: ScheduleError<E, Instant>) -> Self {
        Self::Schedule(error)
    }
}

type CommandResult<'a, S> = Result<
    CommandOutcome<<S as SequenceEnqueuer>::Instant>,
    CommandError<'a, <S as SequenceEnqueuer>::Error, <S as SequenceEnqueuer>::Instant>,
>;

type RebootResult<S> = Result<
    RebootAck<<S as SequenceEnqueuer>::Instant>,
    ScheduleError<<S as SequenceEnqueuer>::Error, <S as SequenceEnqueuer>::Instant>,
>;

type RecoveryResult<S> = Result<
    RecoveryAck<<S as SequenceEnqueuer>::Instant>,
    ScheduleError<<S as SequenceEnqueuer>::Error, <S as SequenceEnqueuer>::Instant>,
>;

type FaultResult<S> = Result<
    FaultAck<<S as SequenceEnqueuer>::Instant>,
    ScheduleError<<S as SequenceEnqueuer>::Error, <S as SequenceEnqueuer>::Instant>,
>;

/// Abstraction over orchestrator schedulers used by the dispatcher.
pub trait SequenceEnqueuer {
    type Instant: Copy + Ord + Add<Duration, Output = Self::Instant>;
    type Error;

    fn enqueue_sequence(
        &mut self,
        kind: StrapSequenceKind,
        requested_at: Self::Instant,
        source: CommandSource,
        flags: CommandFlags,
    ) -> Result<(), ScheduleError<Self::Error, Self::Instant>>;
}

impl<P, const CAPACITY: usize> SequenceEnqueuer for SequenceScheduler<P, CAPACITY>
where
    P: CommandQueueProducer,
    P::Instant: Copy + Ord + Add<Duration, Output = P::Instant>,
{
    type Instant = P::Instant;
    type Error = P::Error;

    fn enqueue_sequence(
        &mut self,
        kind: StrapSequenceKind,
        requested_at: Self::Instant,
        source: CommandSource,
        flags: CommandFlags,
    ) -> Result<(), ScheduleError<Self::Error, Self::Instant>> {
        self.enqueue_with_flags(kind, requested_at, source, flags)
    }
}

/// Dispatches REPL commands into the orchestrator.
pub struct CommandExecutor<S> {
    scheduler: S,
}

impl<S> CommandExecutor<S> {
    /// Creates a new executor around the provided scheduler.
    pub const fn new(scheduler: S) -> Self {
        Self { scheduler }
    }

    /// Returns an immutable reference to the underlying scheduler.
    pub fn scheduler(&self) -> &S {
        &self.scheduler
    }

    /// Returns a mutable reference to the underlying scheduler.
    pub fn scheduler_mut(&mut self) -> &mut S {
        &mut self.scheduler
    }

    /// Consumes the executor and yields the inner scheduler.
    pub fn into_inner(self) -> S {
        self.scheduler
    }
}

impl<S> CommandExecutor<S>
where
    S: SequenceEnqueuer,
{
    /// Parses and executes a REPL command.
    pub fn execute<'a>(
        &mut self,
        line: &'a str,
        now: S::Instant,
        source: CommandSource,
    ) -> CommandResult<'a, S>
    where
        S::Instant: Copy,
    {
        let command = grammar::parse(line)?;
        self.dispatch(command, now, source)
    }

    fn dispatch<'a>(
        &mut self,
        command: Command<'a>,
        now: S::Instant,
        source: CommandSource,
    ) -> CommandResult<'a, S> {
        match command {
            Command::Reboot(action) => self
                .handle_reboot(action, now, source)
                .map(CommandOutcome::Reboot)
                .map_err(CommandError::Schedule),
            Command::Recovery(action) => self
                .handle_recovery(action, now, source)
                .map(CommandOutcome::Recovery)
                .map_err(CommandError::Schedule),
            Command::Fault(action) => {
                let retries = match action.retries {
                    Some(0) => return Err(CommandError::Unsupported("fault retries must be 1-3")),
                    Some(value) if value > FAULT_RECOVERY_MAX_RETRIES => {
                        return Err(CommandError::Unsupported("fault retries must be 1-3"));
                    }
                    other => other,
                };

                self.handle_fault(retries, now, source)
                    .map(CommandOutcome::Fault)
                    .map_err(CommandError::Schedule)
            }
            Command::Status => Err(CommandError::Unsupported("status")),
            Command::Help(_) => Err(CommandError::Unsupported("help")),
        }
    }

    fn handle_reboot(
        &mut self,
        action: RebootCommand,
        now: S::Instant,
        source: CommandSource,
    ) -> RebootResult<S> {
        let mut flags = CommandFlags::default();
        let start_after = match action {
            RebootCommand::Now => None,
            RebootCommand::Delay(duration) if duration.is_zero() => None,
            RebootCommand::Delay(duration) => {
                flags.start_after = Some(duration);
                Some(duration)
            }
        };

        self.scheduler
            .enqueue_sequence(StrapSequenceKind::NormalReboot, now, source, flags)?;

        Ok(RebootAck {
            requested_at: now,
            start_after,
        })
    }

    fn handle_recovery(
        &mut self,
        action: RecoveryCommand,
        now: S::Instant,
        source: CommandSource,
    ) -> RecoveryResult<S> {
        let (sequence, flags) = match action {
            RecoveryCommand::Enter => (StrapSequenceKind::RecoveryEntry, CommandFlags::default()),
            RecoveryCommand::Exit => (StrapSequenceKind::NormalReboot, CommandFlags::default()),
            RecoveryCommand::Now => {
                let flags = CommandFlags {
                    force_recovery: true,
                    ..CommandFlags::default()
                };
                (StrapSequenceKind::RecoveryImmediate, flags)
            }
        };

        self.scheduler
            .enqueue_sequence(sequence, now, source, flags)?;

        Ok(RecoveryAck {
            requested_at: now,
            sequence,
            command: action,
        })
    }

    fn handle_fault(
        &mut self,
        retry_override: Option<u8>,
        now: S::Instant,
        source: CommandSource,
    ) -> FaultResult<S> {
        let retry_budget = match retry_override {
            Some(value) => value,
            None => FAULT_RECOVERY_MAX_RETRIES,
        };

        let mut flags = CommandFlags::default();
        if retry_override.is_some() {
            flags.retry_override = Some(retry_budget);
        }

        self.scheduler
            .enqueue_sequence(StrapSequenceKind::FaultRecovery, now, source, flags)?;

        Ok(FaultAck {
            requested_at: now,
            sequence: StrapSequenceKind::FaultRecovery,
            retry_budget,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::{CommandEnqueueError, CommandQueueProducer};
    use crate::orchestrator::{CommandSource, SequenceCommand};
    use crate::sequences::{
        fault_recovery_template, recovery_entry_template, recovery_immediate_template,
    };
    use core::ops::Add;
    use core::time::Duration;
    use heapless::Vec as HeaplessVec;

    #[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct MockInstant(u64);

    impl MockInstant {
        fn micros(value: u64) -> Self {
            Self(value)
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

    fn executor_with_capacity(capacity: usize) -> CommandExecutor<SequenceScheduler<MockQueue>> {
        let queue = MockQueue::new(capacity);
        let mut scheduler = SequenceScheduler::new(queue);
        {
            let templates = scheduler.templates_mut();
            templates
                .register(recovery_entry_template())
                .expect("register RecoveryEntry template");
            templates
                .register(recovery_immediate_template())
                .expect("register RecoveryImmediate template");
            templates
                .register(fault_recovery_template())
                .expect("register FaultRecovery template");
        }
        CommandExecutor::new(scheduler)
    }

    #[test]
    fn reboot_now_enqueues_immediately() {
        let mut executor = executor_with_capacity(4);
        let now = MockInstant::micros(1_000);

        let outcome = executor
            .execute("reboot now", now, CommandSource::UsbHost)
            .expect("dispatch should succeed");

        assert_eq!(
            outcome,
            CommandOutcome::Reboot(RebootAck {
                requested_at: now,
                start_after: None,
            })
        );

        let commands = executor.scheduler().producer().commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].kind, StrapSequenceKind::NormalReboot);
        assert_eq!(commands[0].requested_at, now);
        assert_eq!(commands[0].flags.start_after, None);
    }

    #[test]
    fn reboot_delay_sets_start_after() {
        let mut executor = executor_with_capacity(4);
        let now = MockInstant::micros(2_000);

        let outcome = executor
            .execute("reboot delay 250ms", now, CommandSource::UsbHost)
            .expect("dispatch should succeed");

        assert_eq!(
            outcome,
            CommandOutcome::Reboot(RebootAck {
                requested_at: now,
                start_after: Some(Duration::from_millis(250)),
            })
        );

        let commands = executor.scheduler().producer().commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(
            commands[0].flags.start_after,
            Some(Duration::from_millis(250))
        );
    }

    #[test]
    fn unsupported_command_is_reported() {
        let mut executor = executor_with_capacity(4);
        let now = MockInstant::micros(0);
        let error = executor
            .execute("status", now, CommandSource::UsbHost)
            .expect_err("status should be unsupported");
        assert_eq!(error, CommandError::Unsupported("status"));
    }

    #[test]
    fn parse_error_is_returned() {
        let mut executor = executor_with_capacity(4);
        let now = MockInstant::micros(0);
        let error = executor
            .execute("reboot later please", now, CommandSource::UsbHost)
            .expect_err("parse should fail");
        assert!(matches!(error, CommandError::Parse(_)));
    }

    #[test]
    fn queue_full_surfaces_schedule_error() {
        let mut executor = executor_with_capacity(0);
        let now = MockInstant::micros(0);
        let error = executor
            .execute("reboot now", now, CommandSource::UsbHost)
            .expect_err("queue full should error");
        assert!(matches!(
            error,
            CommandError::Schedule(ScheduleError::Queue(CommandEnqueueError::QueueFull))
        ));
    }

    #[test]
    fn recovery_enter_enqueues_recovery_entry() {
        let mut executor = executor_with_capacity(4);
        let now = MockInstant::micros(5_000);

        let outcome = executor
            .execute("recovery enter", now, CommandSource::UsbHost)
            .expect("dispatch should succeed");

        assert_eq!(
            outcome,
            CommandOutcome::Recovery(RecoveryAck {
                requested_at: now,
                sequence: StrapSequenceKind::RecoveryEntry,
                command: RecoveryCommand::Enter,
            })
        );

        let commands = executor.scheduler().producer().commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].kind, StrapSequenceKind::RecoveryEntry);
    }

    #[test]
    fn recovery_now_enqueues_recovery_immediate() {
        let mut executor = executor_with_capacity(4);
        let now = MockInstant::micros(6_000);

        let outcome = executor
            .execute("recovery now", now, CommandSource::UsbHost)
            .expect("dispatch should succeed");

        assert_eq!(
            outcome,
            CommandOutcome::Recovery(RecoveryAck {
                requested_at: now,
                sequence: StrapSequenceKind::RecoveryImmediate,
                command: RecoveryCommand::Now,
            })
        );

        let commands = executor.scheduler().producer().commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].kind, StrapSequenceKind::RecoveryImmediate);
    }

    #[test]
    fn recovery_exit_reuses_normal_reboot_sequence() {
        let mut executor = executor_with_capacity(4);
        let now = MockInstant::micros(7_000);

        let outcome = executor
            .execute("recovery exit", now, CommandSource::UsbHost)
            .expect("dispatch should succeed");

        assert_eq!(
            outcome,
            CommandOutcome::Recovery(RecoveryAck {
                requested_at: now,
                sequence: StrapSequenceKind::NormalReboot,
                command: RecoveryCommand::Exit,
            })
        );

        let commands = executor.scheduler().producer().commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].kind, StrapSequenceKind::NormalReboot);
    }

    #[test]
    fn fault_recover_enqueues_with_default_budget() {
        let mut executor = executor_with_capacity(4);
        let now = MockInstant::micros(8_000);

        let outcome = executor
            .execute("fault recover", now, CommandSource::UsbHost)
            .expect("dispatch should succeed");

        assert_eq!(
            outcome,
            CommandOutcome::Fault(FaultAck {
                requested_at: now,
                sequence: StrapSequenceKind::FaultRecovery,
                retry_budget: FAULT_RECOVERY_MAX_RETRIES,
            })
        );

        let commands = executor.scheduler().producer().commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].kind, StrapSequenceKind::FaultRecovery);
        assert_eq!(commands[0].flags.retry_override, None);
    }

    #[test]
    fn fault_recover_accepts_retry_override() {
        let mut executor = executor_with_capacity(4);
        let now = MockInstant::micros(9_000);

        let outcome = executor
            .execute("fault recover retries=2", now, CommandSource::UsbHost)
            .expect("dispatch should succeed");

        assert_eq!(
            outcome,
            CommandOutcome::Fault(FaultAck {
                requested_at: now,
                sequence: StrapSequenceKind::FaultRecovery,
                retry_budget: 2,
            })
        );

        let commands = executor.scheduler().producer().commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].kind, StrapSequenceKind::FaultRecovery);
        assert_eq!(commands[0].flags.retry_override, Some(2));
    }

    #[test]
    fn fault_recover_rejects_out_of_range_retry_override() {
        let mut executor = executor_with_capacity(4);
        let now = MockInstant::micros(10_000);

        let error = executor
            .execute("fault recover retries=5", now, CommandSource::UsbHost)
            .expect_err("retry override exceeding template should fail");

        assert_eq!(
            error,
            CommandError::Unsupported("fault retries must be 1-3")
        );
    }
}
