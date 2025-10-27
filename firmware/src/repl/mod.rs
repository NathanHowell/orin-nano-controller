//! Lightweight helpers for the firmware REPL transport.
//!
//! The embedded target shares its command parser and dispatcher with the
//! `controller-core` crate. This module keeps only the pieces that must remain
//! firmware-specific: a bounded line buffer that copes with the CDC transport
//! and a couple of convenience types for error handling.

use heapless::Vec;

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
