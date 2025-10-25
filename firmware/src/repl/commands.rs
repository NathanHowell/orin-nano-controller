//! Command implementations for the operator REPL.
//!
//! Each handler validates arguments and enqueues the appropriate strap sequence
//! requests for the orchestrator while tracking scheduling metadata.

use embassy_sync::channel::TrySendError;
use embassy_time::{Duration, Instant};

use crate::straps::{CommandSender, CommandSource, SequenceCommand, StrapSequenceKind};

use super::{CommandDispatcher, CommandIntent, CommandKind, DispatchError, ReplLink};

/// Executes REPL commands by enqueuing strap sequences.
pub struct CommandExecutor<'a, C>
where
    C: Fn() -> Instant,
{
    link: ReplLink,
    sender: CommandSender<'a>,
    clock: C,
}

impl<'a> CommandExecutor<'a, fn() -> Instant> {
    /// Creates an executor bound to the given link using [`Instant::now`] for timestamps.
    pub fn new(link: ReplLink, sender: CommandSender<'a>) -> Self {
        Self::with_clock(link, sender, Instant::now)
    }
}

impl<'a, C> CommandExecutor<'a, C>
where
    C: Fn() -> Instant,
{
    /// Creates an executor with a custom clock (primarily used for tests).
    pub fn with_clock(link: ReplLink, sender: CommandSender<'a>, clock: C) -> Self {
        Self {
            link,
            sender,
            clock,
        }
    }

    fn now(&self) -> Instant {
        (self.clock)()
    }

    fn enqueue(&self, command: SequenceCommand) -> Result<(), DispatchError> {
        match self.sender.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(DispatchError::Busy),
        }
    }

    fn require_local(&self) -> Result<(), DispatchError> {
        if self.link == ReplLink::UsbCdc0 {
            Ok(())
        } else {
            Err(DispatchError::Unsupported)
        }
    }

    fn handle_reboot<'line>(&self, intent: CommandIntent<'line>) -> Result<(), DispatchError> {
        self.require_local()?;

        let args = parse_reboot_args(intent.remainder)?;
        let mut command = SequenceCommand::new(
            StrapSequenceKind::NormalReboot,
            self.now(),
            CommandSource::UsbHost,
        );
        command.flags.start_after = args.start_after;

        self.enqueue(command)
    }
}

impl<'a, C> CommandDispatcher for CommandExecutor<'a, C>
where
    C: Fn() -> Instant,
{
    fn dispatch<'line>(&mut self, intent: CommandIntent<'line>) -> Result<(), DispatchError> {
        match intent.kind {
            CommandKind::Reboot => self.handle_reboot(intent),
            _ => Err(DispatchError::Unsupported),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct RebootArgs {
    start_after: Option<Duration>,
}

fn parse_reboot_args(input: &str) -> Result<RebootArgs, DispatchError> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("now") {
        return Ok(RebootArgs { start_after: None });
    }

    let mut parts = trimmed.split_whitespace();
    let keyword = parts.next().ok_or(DispatchError::InvalidArgument)?;
    if !keyword.eq_ignore_ascii_case("delay") {
        return Err(DispatchError::InvalidArgument);
    }

    let duration_token = parts.next().ok_or(DispatchError::InvalidArgument)?;
    if parts.next().is_some() {
        return Err(DispatchError::InvalidArgument);
    }

    let duration = parse_duration(duration_token)?;
    Ok(RebootArgs {
        start_after: if duration == Duration::from_millis(0) {
            None
        } else {
            Some(duration)
        },
    })
}

fn parse_duration(token: &str) -> Result<Duration, DispatchError> {
    if let Some(value) = token.strip_suffix("ms") {
        let millis = parse_u64(value)?;
        return Ok(Duration::from_millis(millis));
    }

    if let Some(value) = token.strip_suffix('s') {
        let seconds = parse_u64(value)?;
        let millis = seconds
            .checked_mul(1000)
            .ok_or(DispatchError::InvalidArgument)?;
        return Ok(Duration::from_millis(millis));
    }

    Err(DispatchError::InvalidArgument)
}

fn parse_u64(value: &str) -> Result<u64, DispatchError> {
    value
        .parse::<u64>()
        .map_err(|_| DispatchError::InvalidArgument)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::straps::{CommandQueue, CommandSource, SequenceCommand, StrapSequenceKind};
    #[test]
    fn reboot_defaults_to_immediate() {
        let queue = CommandQueue::new();
        let sender = queue.sender();
        let receiver = queue.receiver();
        let mut executor =
            CommandExecutor::with_clock(ReplLink::UsbCdc0, sender, || Instant::from_micros(1));

        executor
            .dispatch(CommandIntent::new(CommandKind::Reboot, ""))
            .expect("dispatch should succeed");

        let command = receiver.try_receive().expect("command missing");
        assert_eq!(command.kind, StrapSequenceKind::NormalReboot);
        assert_eq!(command.flags.start_after, None);
        assert_eq!(command.source, CommandSource::UsbHost);
    }

    #[test]
    fn reboot_accepts_delay_in_milliseconds() {
        let queue = CommandQueue::new();
        let sender = queue.sender();
        let receiver = queue.receiver();
        let mut executor =
            CommandExecutor::with_clock(ReplLink::UsbCdc0, sender, || Instant::from_micros(5_000));

        executor
            .dispatch(CommandIntent::new(CommandKind::Reboot, "delay 250ms"))
            .expect("dispatch should succeed");

        let command = receiver.try_receive().expect("command missing");
        assert_eq!(command.flags.start_after, Some(Duration::from_millis(250)));
        assert_eq!(command.requested_at, Instant::from_micros(5_000));
    }

    #[test]
    fn reboot_rejects_invalid_arguments() {
        let queue = CommandQueue::new();
        let sender = queue.sender();
        let mut executor =
            CommandExecutor::with_clock(ReplLink::UsbCdc0, sender, || Instant::from_micros(0));

        let result = executor.dispatch(CommandIntent::new(CommandKind::Reboot, "delay bananas"));
        assert_eq!(result, Err(DispatchError::InvalidArgument));
    }

    #[test]
    fn reboot_accepts_delay_in_seconds() {
        let queue = CommandQueue::new();
        let sender = queue.sender();
        let receiver = queue.receiver();
        let mut executor =
            CommandExecutor::with_clock(ReplLink::UsbCdc0, sender, || Instant::from_micros(0));

        executor
            .dispatch(CommandIntent::new(CommandKind::Reboot, "delay 2s"))
            .expect("dispatch should succeed");

        let command = receiver.try_receive().expect("command missing");
        assert_eq!(
            command.flags.start_after,
            Some(Duration::from_millis(2_000))
        );
    }

    #[test]
    fn unsupported_commands_return_error() {
        let queue = CommandQueue::new();
        let sender = queue.sender();
        let mut executor =
            CommandExecutor::with_clock(ReplLink::UsbCdc0, sender, || Instant::from_micros(0));

        let result = executor.dispatch(CommandIntent::new(CommandKind::Status, ""));
        assert_eq!(result, Err(DispatchError::Unsupported));
    }

    #[test]
    fn reboot_respects_queue_capacity() {
        let queue = CommandQueue::new();
        let sender = queue.sender();
        let receiver = queue.receiver();
        // Pre-fill the queue to capacity.
        for _ in 0..crate::straps::COMMAND_QUEUE_DEPTH {
            sender
                .try_send(SequenceCommand::new(
                    StrapSequenceKind::NormalReboot,
                    Instant::from_micros(0),
                    CommandSource::UsbHost,
                ))
                .expect("prefill should succeed");
        }

        let sender2 = queue.sender();
        let mut executor =
            CommandExecutor::with_clock(ReplLink::UsbCdc0, sender2, || Instant::from_micros(0));

        let result = executor.dispatch(CommandIntent::new(CommandKind::Reboot, ""));
        assert_eq!(result, Err(DispatchError::Busy));

        // Drain one command to prove queue still contains items.
        assert!(receiver.try_receive().is_ok());
    }
}
