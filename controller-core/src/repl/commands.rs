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
use crate::sequences::StrapSequenceKind;

use super::grammar::{self, Command, RebootCommand};

/// Command execution successes.
#[derive(Clone, Debug, PartialEq)]
pub enum CommandOutcome<Instant> {
    Reboot(RebootAck<Instant>),
}

/// Summary returned after queueing a reboot command.
#[derive(Clone, Debug, PartialEq)]
pub struct RebootAck<Instant> {
    pub requested_at: Instant,
    pub start_after: Option<Duration>,
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
            Command::Recovery(_) => Err(CommandError::Unsupported("recovery")),
            Command::Fault(_) => Err(CommandError::Unsupported("fault")),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::{CommandEnqueueError, CommandQueueProducer};
    use crate::orchestrator::{CommandSource, SequenceCommand};
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
        let scheduler = SequenceScheduler::new(queue);
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
}
