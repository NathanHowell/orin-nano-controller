#![allow(dead_code)]
//! Lightweight helpers for the firmware REPL transport.
//!
//! The embedded target shares its command parser and dispatcher with the
//! `controller-core` crate. This module keeps only the pieces that must remain
//! firmware-specific: a bounded line buffer that copes with the CDC transport
//! and a couple of convenience types for error handling.

#[cfg(not(target_os = "none"))]
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
#[cfg(target_os = "none")]
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::channel::Channel;
use heapless::Vec;

#[cfg(target_os = "none")]
use controller_core::orchestrator::{
    CommandEnqueueError, CommandFlags, CommandQueueProducer, CommandSource, SequenceCommand,
};
#[cfg(target_os = "none")]
use controller_core::repl::commands::{CommandOutcome, RebootAck, RecoveryAck};
#[cfg(target_os = "none")]
use controller_core::repl::grammar::{self, Command, RebootCommand, RecoveryCommand};
#[cfg(target_os = "none")]
use controller_core::sequences::StrapSequenceKind;
#[cfg(target_os = "none")]
use core::fmt::Write as _;
#[cfg(target_os = "none")]
use embassy_sync::channel::{Receiver, Sender};
#[cfg(target_os = "none")]
use embassy_time::Instant;
#[cfg(target_os = "none")]
use heapless::String;

#[cfg(target_os = "none")]
use crate::straps::{CommandProducer, CommandSender};

/// Capacity for USB CDC frames exchanged with the REPL task.
pub const FRAME_CAPACITY: usize = 64;

/// Queue depth for REPL RX/TX channels.
pub const FRAME_QUEUE_DEPTH: usize = 4;

#[cfg(target_os = "none")]
type ReplMutex = ThreadModeRawMutex;
#[cfg(not(target_os = "none"))]
type ReplMutex = NoopRawMutex;

/// Frame exchanged between the USB CDC handler and the REPL processor.
pub type ReplFrame = Vec<u8, FRAME_CAPACITY>;

/// USB→REPL frame queue.
pub static REPL_RX_QUEUE: Channel<ReplMutex, ReplFrame, FRAME_QUEUE_DEPTH> = Channel::new();
/// REPL→USB frame queue.
pub static REPL_TX_QUEUE: Channel<ReplMutex, ReplFrame, FRAME_QUEUE_DEPTH> = Channel::new();

/// Maximum number of bytes accepted on a single REPL line (excluding the
/// terminator).
pub const MAX_LINE_LEN: usize = 96;

/// Errors that may occur while ingesting input into the line buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineError {
    /// The REPL line reached the configured capacity.
    Overflow,
}

/// Bounded buffer that accumulates bytes until a newline completes the line.
#[derive(Default)]
pub struct LineBuffer {
    buf: Vec<u8, MAX_LINE_LEN>,
}

impl LineBuffer {
    /// Creates an empty line buffer.
    pub const fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Clears any accumulated bytes.
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Returns `true` when no input has been buffered.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Number of bytes currently stored in the buffer.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Appends a byte to the buffer.
    pub fn push(&mut self, byte: u8) -> Result<(), LineError> {
        self.buf.push(byte).map_err(|_| LineError::Overflow)
    }

    /// Removes the most recently buffered byte, if any.
    pub fn pop(&mut self) {
        self.buf.pop();
    }

    /// Yields a copy of the buffered line and clears the buffer.
    ///
    /// Returns `None` when the buffer is empty.
    pub fn take(&mut self) -> Option<Vec<u8, MAX_LINE_LEN>> {
        if self.buf.is_empty() {
            return None;
        }

        let mut line = Vec::<u8, MAX_LINE_LEN>::new();
        if line.extend_from_slice(self.buf.as_slice()).is_err() {
            // Capacities match, so this branch should never trigger. Return
            // `None` instead of panicking to keep the REPL resilient.
            return None;
        }

        self.buf.clear();
        Some(line)
    }

    /// Provides a read-only view of the underlying byte slice.
    pub fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }
}

#[cfg(target_os = "none")]
type ReplReceiver<'a> = Receiver<'a, ReplMutex, ReplFrame, FRAME_QUEUE_DEPTH>;

#[cfg(target_os = "none")]
type ReplSender<'a> = Sender<'a, ReplMutex, ReplFrame, FRAME_QUEUE_DEPTH>;

/// Minimal REPL session that consumes frames from the USB CDC queue and
/// dispatches parsed commands into the strap command scheduler.
#[cfg(target_os = "none")]
pub struct ReplSession<'a> {
    rx: ReplReceiver<'a>,
    tx: ReplSender<'a>,
    producer: CommandProducer<'a>,
    buffer: LineBuffer,
    drop_input: bool,
}

#[cfg(target_os = "none")]
impl<'a> ReplSession<'a> {
    /// Creates a new REPL session bound to the shared command queue.
    pub fn new(command_sender: CommandSender<'a>) -> Self {
        Self {
            rx: REPL_RX_QUEUE.receiver(),
            tx: REPL_TX_QUEUE.sender(),
            producer: CommandProducer::new(command_sender),
            buffer: LineBuffer::new(),
            drop_input: false,
        }
    }

    /// Drives the session indefinitely.
    pub async fn run(&mut self) -> ! {
        loop {
            let frame = self.rx.receive().await;
            self.consume_frame(&frame).await;
        }
    }

    async fn consume_frame(&mut self, frame: &ReplFrame) {
        for &byte in frame {
            match byte {
                b'\r' | b'\n' => {
                    if let Some(line) = self.buffer.take() {
                        let discard = self.drop_input;
                        self.drop_input = false;
                        if discard {
                            continue;
                        }
                        self.handle_line(line).await;
                    }
                }
                b'\x08' | b'\x7f' => {
                    if !self.buffer.is_empty() {
                        self.buffer.pop();
                    }
                }
                byte if byte.is_ascii() && !self.drop_input => {
                    if let Err(LineError::Overflow) = self.buffer.push(byte) {
                        self.drop_input = true;
                        self.notify_error("ERR line-too-long").await;
                    }
                }
                _ => {}
            }
        }
    }

    async fn handle_line(&mut self, line: Vec<u8, MAX_LINE_LEN>) {
        let Ok(text) = core::str::from_utf8(line.as_slice()) else {
            self.notify_error("ERR invalid-utf8").await;
            return;
        };

        if text.trim().is_empty() {
            return;
        }

        let now = Instant::now();
        match self.execute_command(text.trim(), now) {
            Ok(outcome) => self.notify_success(outcome).await,
            Err(err) => self.notify_execution_error(err).await,
        }
    }

    async fn notify_success(&mut self, outcome: CommandOutcome<Instant>) {
        let mut message: String<FRAME_CAPACITY> = String::new();

        match outcome {
            CommandOutcome::Reboot(ack) => format_reboot_ack(&mut message, ack),
            CommandOutcome::Recovery(ack) => format_recovery_ack(&mut message, ack),
        }

        if message.is_empty() {
            let _ = message.push_str("OK");
        }

        self.send_line(message.as_str()).await;
    }

    async fn notify_execution_error(&mut self, error: ExecuteError) {
        let mut message: String<FRAME_CAPACITY> = String::new();

        match error {
            ExecuteError::Parse(parse) => {
                let _ = message.push_str("ERR syntax ");
                let _ = message.push_str(parse.as_str());
            }
            ExecuteError::Unsupported(topic) => {
                let _ = write!(message, "ERR unsupported {topic}");
            }
            ExecuteError::Queue(error) => describe_queue_error(&mut message, error),
        }

        if message.is_empty() {
            let _ = message.push_str("ERR");
        }

        self.send_line(message.as_str()).await;
    }

    async fn notify_error(&mut self, message: &str) {
        self.send_line(message).await;
    }

    async fn send_line(&mut self, message: &str) {
        if message.is_empty() {
            return;
        }

        let mut frame = ReplFrame::new();
        if frame.extend_from_slice(message.as_bytes()).is_err() {
            return;
        }
        let _ = frame.push(b'\n');
        self.tx.send(frame).await;
    }

    fn execute_command(
        &mut self,
        line: &str,
        now: Instant,
    ) -> Result<CommandOutcome<Instant>, ExecuteError> {
        let command = match grammar::parse(line) {
            Ok(command) => command,
            Err(error) => {
                let mut message = String::<FRAME_CAPACITY>::new();
                let _ = write!(message, "{error}");
                return Err(ExecuteError::Parse(message));
            }
        };

        self.dispatch_command(command, now)
    }

    fn dispatch_command(
        &mut self,
        command: Command<'_>,
        now: Instant,
    ) -> Result<CommandOutcome<Instant>, ExecuteError> {
        match command {
            Command::Reboot(action) => self.enqueue_reboot(action, now).map(CommandOutcome::Reboot),
            Command::Recovery(action) => self
                .enqueue_recovery(action, now)
                .map(CommandOutcome::Recovery),
            Command::Fault(_) => Err(ExecuteError::Unsupported("fault")),
            Command::Status => Err(ExecuteError::Unsupported("status")),
            Command::Help(_) => Err(ExecuteError::Unsupported("help")),
        }
    }

    fn enqueue_reboot(
        &mut self,
        action: RebootCommand,
        now: Instant,
    ) -> Result<RebootAck<Instant>, ExecuteError> {
        let mut flags = CommandFlags::default();
        let start_after = match action {
            RebootCommand::Now => None,
            RebootCommand::Delay(duration) if duration.is_zero() => None,
            RebootCommand::Delay(duration) => {
                flags.start_after = Some(duration);
                Some(duration)
            }
        };

        let command = SequenceCommand::with_flags(
            StrapSequenceKind::NormalReboot,
            now,
            CommandSource::UsbHost,
            flags,
        );
        self.producer
            .try_enqueue(command)
            .map_err(ExecuteError::Queue)?;

        Ok(RebootAck {
            requested_at: now,
            start_after,
        })
    }

    fn enqueue_recovery(
        &mut self,
        action: RecoveryCommand,
        now: Instant,
    ) -> Result<RecoveryAck<Instant>, ExecuteError> {
        let (sequence, flags) = match action {
            RecoveryCommand::Enter => (StrapSequenceKind::RecoveryEntry, CommandFlags::default()),
            RecoveryCommand::Exit => (StrapSequenceKind::NormalReboot, CommandFlags::default()),
            RecoveryCommand::Now => (
                StrapSequenceKind::RecoveryImmediate,
                CommandFlags {
                    force_recovery: true,
                    ..CommandFlags::default()
                },
            ),
        };

        let command = SequenceCommand::with_flags(sequence, now, CommandSource::UsbHost, flags);
        self.producer
            .try_enqueue(command)
            .map_err(ExecuteError::Queue)?;

        Ok(RecoveryAck {
            requested_at: now,
            sequence,
            command: action,
        })
    }
}

#[cfg(target_os = "none")]
fn format_reboot_ack(buffer: &mut String<FRAME_CAPACITY>, ack: RebootAck<Instant>) {
    let _ = buffer.push_str("OK reboot");
    if let Some(delay) = ack.start_after {
        let millis = delay.as_millis();
        let _ = write!(buffer, " start-after={}ms", millis);
    }
}

#[cfg(target_os = "none")]
fn format_recovery_ack(buffer: &mut String<FRAME_CAPACITY>, ack: RecoveryAck<Instant>) {
    let _ = buffer.push_str("OK recovery");
    let _ = write!(buffer, " sequence={:?}", ack.sequence);
}

#[cfg(target_os = "none")]
fn describe_queue_error(buffer: &mut String<FRAME_CAPACITY>, error: CommandEnqueueError<()>) {
    match error {
        CommandEnqueueError::QueueFull => {
            let _ = buffer.push_str("ERR busy queue-full");
        }
        CommandEnqueueError::Disconnected => {
            let _ = buffer.push_str("ERR busy queue-disconnected");
        }
        CommandEnqueueError::Other(_) => {
            let _ = buffer.push_str("ERR busy queue-error");
        }
    }
}

#[cfg(target_os = "none")]
enum ExecuteError {
    Parse(String<FRAME_CAPACITY>),
    Unsupported(&'static str),
    Queue(CommandEnqueueError<()>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pushes_and_takes_lines() {
        let mut buffer = LineBuffer::new();
        buffer.push(b'r').unwrap();
        buffer.push(b'e').unwrap();
        buffer.push(b'b').unwrap();
        buffer.push(b'o').unwrap();
        buffer.push(b'o').unwrap();
        buffer.push(b't').unwrap();

        let line = buffer.take().expect("line missing");
        assert_eq!(line.as_slice(), b"reboot");
        assert!(buffer.is_empty());
    }

    #[test]
    fn pop_resets_tail() {
        let mut buffer = LineBuffer::new();
        buffer.push(b'a').unwrap();
        buffer.push(b'b').unwrap();
        buffer.pop();
        let line = buffer.take().expect("line missing");
        assert_eq!(line.as_slice(), b"a");
    }
}
