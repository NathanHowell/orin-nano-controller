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
#[cfg(target_os = "none")]
use embassy_sync::channel::Channel;
use heapless::Vec;

#[cfg(target_os = "none")]
use controller_core::orchestrator::{
    CommandEnqueueError, CommandSource, ScheduleError, SequenceScheduler,
};
#[cfg(target_os = "none")]
use controller_core::repl::commands::{
    CommandError as ExecutorError, CommandExecutor, CommandOutcome, FaultAck, RebootAck,
    RecoveryAck,
};
#[cfg(target_os = "none")]
use controller_core::repl::completion::{CompletionEngine, CompletionResult};
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
use crate::straps::{CommandProducer, FirmwareInstant};

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
#[cfg(target_os = "none")]
pub static REPL_RX_QUEUE: Channel<ReplMutex, ReplFrame, FRAME_QUEUE_DEPTH> = Channel::new();
/// REPL→USB frame queue.
#[cfg(target_os = "none")]
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

    /// Replaces the tail of the buffer starting at `start` with the provided
    /// `replacement`.
    pub fn replace_from(&mut self, start: usize, replacement: &str) -> Result<(), LineError> {
        if start > self.buf.len() {
            return Err(LineError::Overflow);
        }

        self.buf.truncate(start);
        self.buf
            .extend_from_slice(replacement.as_bytes())
            .map_err(|_| LineError::Overflow)
    }
}

#[cfg(target_os = "none")]
type ReplReceiver<'a> = Receiver<'a, ReplMutex, ReplFrame, FRAME_QUEUE_DEPTH>;

#[cfg(target_os = "none")]
type ReplSender<'a> = Sender<'a, ReplMutex, ReplFrame, FRAME_QUEUE_DEPTH>;

#[cfg(target_os = "none")]
type FirmwareExecutor<'a> = CommandExecutor<SequenceScheduler<CommandProducer<'a>>>;

/// Minimal REPL session that consumes frames from the USB CDC queue and
/// dispatches parsed commands into the strap command scheduler.
#[cfg(target_os = "none")]
pub struct ReplSession<'a> {
    rx: ReplReceiver<'a>,
    tx: ReplSender<'a>,
    executor: FirmwareExecutor<'a>,
    buffer: LineBuffer,
    completion: CompletionEngine,
    drop_input: bool,
}

#[cfg(target_os = "none")]
impl<'a> ReplSession<'a> {
    /// Creates a new REPL session bound to the shared command queue.
    pub fn new(executor: FirmwareExecutor<'a>) -> Self {
        Self {
            rx: REPL_RX_QUEUE.receiver(),
            tx: REPL_TX_QUEUE.sender(),
            executor,
            buffer: LineBuffer::new(),
            completion: CompletionEngine::new(),
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
                b'\t' => {
                    if !self.drop_input {
                        self.handle_completion().await;
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
        let result = {
            let trimmed = text.trim();
            self.execute_command(trimmed, now)
        };
        match result {
            Ok(outcome) => self.notify_success(outcome).await,
            Err(err) => self.notify_execution_error(err, now).await,
        }
    }

    async fn handle_completion(&mut self) {
        let buffer = self.buffer.as_slice();
        let Ok(text) = core::str::from_utf8(buffer) else {
            self.emit_bell().await;
            return;
        };

        let cursor = text.len();
        let completion = self.completion.complete(text, cursor);
        let CompletionResult {
            replacement,
            options,
        } = completion;

        let mut options = options;
        if options.is_empty() {
            self.emit_bell().await;
            return;
        }

        let option_count = options.len();
        if option_count == 1 {
            let Some(replacement) = replacement else {
                self.emit_bell().await;
                return;
            };

            if replacement.end != cursor || replacement.start > replacement.end {
                self.emit_bell().await;
                return;
            }

            let removal = replacement.end - replacement.start;
            if let Err(LineError::Overflow) = self
                .buffer
                .replace_from(replacement.start, replacement.value)
            {
                self.drop_input = true;
                self.notify_error("ERR line-too-long").await;
                return;
            }

            self.echo_backspaces(removal).await;
            self.echo_bytes(replacement.value.as_bytes()).await;
        } else {
            let mut snapshot: Vec<u8, MAX_LINE_LEN> = Vec::new();
            if snapshot.extend_from_slice(self.buffer.as_slice()).is_err() {
                return;
            }
            self.emit_suggestions(options.as_slice()).await;
            self.echo_bytes(snapshot.as_slice()).await;
        }
    }

    async fn notify_success(&mut self, outcome: CommandOutcome<FirmwareInstant>) {
        let mut message: String<FRAME_CAPACITY> = String::new();

        match outcome {
            CommandOutcome::Reboot(ack) => format_reboot_ack(&mut message, ack),
            CommandOutcome::Recovery(ack) => format_recovery_ack(&mut message, ack),
            CommandOutcome::Fault(ack) => format_fault_ack(&mut message, ack),
        }

        if message.is_empty() {
            let _ = message.push_str("OK");
        }

        self.send_line(message.as_str()).await;
    }

    async fn notify_execution_error(
        &mut self,
        error: ExecutorError<'_, (), FirmwareInstant>,
        now: Instant,
    ) {
        let mut message: String<FRAME_CAPACITY> = String::new();

        match error {
            ExecutorError::Parse(parse) => {
                let _ = message.push_str("ERR syntax ");
                let _ = write!(message, "{parse}");
            }
            ExecutorError::Unsupported(topic) => {
                let _ = write!(message, "ERR unsupported {topic}");
            }
            ExecutorError::Schedule(error) => {
                describe_schedule_error(&mut message, error, now);
            }
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

        self.send_bytes(message.as_bytes()).await;
        self.send_bytes(b"\n").await;
    }

    async fn send_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        let mut offset = 0;
        while offset < bytes.len() {
            let end = (offset + FRAME_CAPACITY).min(bytes.len());
            let mut frame = ReplFrame::new();
            if frame.extend_from_slice(&bytes[offset..end]).is_err() {
                return;
            }
            self.tx.send(frame).await;
            offset = end;
        }
    }

    async fn emit_bell(&mut self) {
        self.send_bytes(&[0x07]).await;
    }

    async fn echo_bytes(&mut self, bytes: &[u8]) {
        self.send_bytes(bytes).await;
    }

    async fn echo_backspaces(&mut self, count: usize) {
        if count == 0 {
            return;
        }

        let mut remaining = count;
        let mut scratch = [0u8; FRAME_CAPACITY];
        while remaining > 0 {
            let chunk = remaining.min(FRAME_CAPACITY);
            for byte in &mut scratch[..chunk] {
                *byte = b'\x08';
            }
            self.send_bytes(&scratch[..chunk]).await;
            remaining -= chunk;
        }
    }

    async fn emit_suggestions(&mut self, options: &[&'static str]) {
        for option in options.iter().copied() {
            self.send_line(option).await;
        }
    }

    fn execute_command<'line>(
        &mut self,
        line: &'line str,
        now: Instant,
    ) -> Result<CommandOutcome<FirmwareInstant>, ExecutorError<'line, (), FirmwareInstant>> {
        let instant = FirmwareInstant::from(now);
        self.executor.execute(line, instant, CommandSource::UsbHost)
    }
}

#[cfg(target_os = "none")]
fn format_reboot_ack(buffer: &mut String<FRAME_CAPACITY>, ack: RebootAck<FirmwareInstant>) {
    let _ = buffer.push_str("OK reboot");
    if let Some(delay) = ack.start_after {
        let millis = delay.as_millis();
        let _ = write!(buffer, " start-after={}ms", millis);
    }
}

#[cfg(target_os = "none")]
fn format_recovery_ack(buffer: &mut String<FRAME_CAPACITY>, ack: RecoveryAck<FirmwareInstant>) {
    let _ = buffer.push_str("OK recovery");
    let _ = write!(buffer, " sequence={:?}", ack.sequence);
}

#[cfg(target_os = "none")]
fn format_fault_ack(buffer: &mut String<FRAME_CAPACITY>, ack: FaultAck<FirmwareInstant>) {
    let _ = buffer.push_str("OK fault recover");
    let _ = write!(buffer, " retries={}", ack.retry_budget);
}

#[cfg(target_os = "none")]
fn describe_schedule_error(
    buffer: &mut String<FRAME_CAPACITY>,
    error: ScheduleError<(), FirmwareInstant>,
    now: Instant,
) {
    match error {
        ScheduleError::Queue(queue) => describe_queue_error(buffer, queue),
        ScheduleError::MissingTemplate(kind) => {
            let _ = write!(buffer, "ERR missing-template {}", sequence_label(kind));
        }
        ScheduleError::CooldownActive { kind, ready_at } => {
            let ready_at = ready_at.into_embassy();
            let remaining = ready_at.saturating_duration_since(now).as_millis();
            let _ = write!(
                buffer,
                "ERR cooldown {} ready-in={}ms",
                sequence_label(kind),
                remaining
            );
        }
    }
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
fn sequence_label(kind: StrapSequenceKind) -> &'static str {
    match kind {
        StrapSequenceKind::NormalReboot => "normal-reboot",
        StrapSequenceKind::RecoveryEntry => "recovery-entry",
        StrapSequenceKind::RecoveryImmediate => "recovery-immediate",
        StrapSequenceKind::FaultRecovery => "fault-recovery",
    }
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
