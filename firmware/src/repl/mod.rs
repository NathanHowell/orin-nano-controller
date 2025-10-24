//! REPL session scaffolding.
//!
//! This module provides the initial building blocks for the USB CDC0 operator
//! console described in `specs/001-build-orin-controller/plan.md`. It wires a
//! line-oriented session on top of the local USB link, exposes the lexer and
//! parser primitives that will back the REPL grammar, and defines dispatcher
//! traits so higher layers can enqueue strap sequences safely.

#![allow(dead_code)]

use core::mem;
use core::str;

use heapless::Vec;
use logos::Logos;
use winnow::ascii::multispace0;
use winnow::combinator::alt;
use winnow::error::ContextError;
use winnow::prelude::*;

/// Maximum number of bytes accepted on a single REPL line (excluding terminator).
pub const MAX_LINE_LEN: usize = 96;

/// Identifies the transport surface that can host the REPL.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplLink {
    /// Local USB CDC ACM interface dedicated to the operator REPL (CDC0).
    UsbCdc0,
}

/// Abstraction over the CDC transport feeding the REPL.
pub trait ReplTransport {
    /// Returns the link associated with this transport.
    fn link(&self) -> ReplLink;
}

/// Errors surfaced by the REPL session.
#[derive(Debug, PartialEq, Eq)]
pub enum ReplError {
    /// Attempted to bind the REPL to a transport other than USB CDC0.
    UnsupportedLink,
    /// Encountered non-UTF-8 data in the assembled line buffer.
    InvalidUtf8,
    /// Input exceeded the maximum configured line length.
    LineOverflow,
    /// Parser rejected the submitted command.
    Syntax,
    /// Command dispatcher returned an application error.
    Dispatch(DispatchError),
}

impl From<DispatchError> for ReplError {
    fn from(error: DispatchError) -> Self {
        Self::Dispatch(error)
    }
}

/// Errors returned by command dispatchers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchError {
    /// The system is busy executing other work and cannot accept the command.
    Busy,
    /// The requested command is not supported in the current context.
    Unsupported,
    /// Internal error surfaced by the command executor.
    Internal,
}

/// Classifies the top-level command keyword.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandKind {
    Reboot,
    Recovery,
    Fault,
    Status,
    Help,
}

/// Parsed command intent pending further argument decoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandIntent<'a> {
    pub kind: CommandKind,
    pub remainder: &'a str,
}

impl<'a> CommandIntent<'a> {
    /// Creates a new command intent.
    pub const fn new(kind: CommandKind, remainder: &'a str) -> Self {
        Self { kind, remainder }
    }
}

/// Trait implemented by consumers able to execute parsed commands.
pub trait CommandDispatcher {
    /// Executes the supplied command intent.
    fn dispatch<'line>(&mut self, intent: CommandIntent<'line>) -> Result<(), DispatchError>;
}

/// Maintains REPL state for the CDC0 session.
pub struct ReplSession<T, D> {
    link: ReplLink,
    transport: T,
    dispatcher: D,
    buffer: Vec<u8, MAX_LINE_LEN>,
    state: SessionState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionState {
    Disconnected,
    Connected,
}

impl<T, D> ReplSession<T, D>
where
    T: ReplTransport,
    D: CommandDispatcher,
{
    /// Creates a new REPL session bound to CDC0.
    pub fn new(transport: T, dispatcher: D) -> Result<Self, ReplError> {
        if transport.link() != ReplLink::UsbCdc0 {
            return Err(ReplError::UnsupportedLink);
        }

        Ok(Self {
            link: ReplLink::UsbCdc0,
            transport,
            dispatcher,
            buffer: Vec::new(),
            state: SessionState::Disconnected,
        })
    }

    /// Marks the transport as connected (host opened CDC0).
    pub fn on_connect(&mut self) {
        self.state = SessionState::Connected;
        self.buffer.clear();
    }

    /// Marks the transport as disconnected (host closed CDC0).
    pub fn on_disconnect(&mut self) {
        self.state = SessionState::Disconnected;
        self.buffer.clear();
    }

    /// Feeds a single byte into the session. Newline triggers parsing/dispatch.
    pub fn ingest(&mut self, byte: u8) -> Result<(), ReplError> {
        if self.state != SessionState::Connected {
            return Ok(());
        }

        match byte {
            b'\r' | b'\n' => self.process_line(),
            0x08 | 0x7f => {
                self.buffer.pop();
                Ok(())
            }
            value => {
                if self.buffer.len() >= MAX_LINE_LEN {
                    return Err(ReplError::LineOverflow);
                }

                self.buffer.push(value).map_err(|_| ReplError::LineOverflow)
            }
        }
    }

    fn process_line(&mut self) -> Result<(), ReplError> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        let line = str::from_utf8(self.buffer.as_slice()).map_err(|_| ReplError::InvalidUtf8)?;
        let intent = CommandParser::parse(line)?;
        self.dispatcher.dispatch(intent)?;
        self.buffer.clear();
        Ok(())
    }

    /// Provides a lexer view over the current line buffer (used for completions).
    pub fn preview_tokens(&self) -> CommandLexer<'_> {
        let line = str::from_utf8(self.buffer.as_slice()).unwrap_or("");
        CommandLexer::new(line)
    }
}

/// Lexeme returned by the command lexer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandLexeme<'a> {
    Keyword(CommandKind),
    Ident(&'a str),
    Integer(&'a str),
    Duration(&'a str),
    Equals,
    End,
    Error(&'a str),
}

/// Token definitions used by the lexer.
#[derive(Logos, Clone, Copy, Debug, PartialEq, Eq)]
enum Token<'a> {
    #[token("reboot", priority = 4)]
    Reboot,
    #[token("recovery", priority = 4)]
    Recovery,
    #[token("fault", priority = 4)]
    Fault,
    #[token("status", priority = 4)]
    Status,
    #[token("help", priority = 4)]
    Help,
    #[regex(r"[0-9]+(?:ms|s)", priority = 2)]
    Duration(&'a str),
    #[regex(r"[0-9]+", priority = 1)]
    Integer(&'a str),
    #[regex(r"[A-Za-z][A-Za-z0-9_-]*")]
    Ident(&'a str),
    #[token("=")]
    Equals,
    #[token("\r")]
    CarriageReturn,
    #[token("\n")]
    LineFeed,
    #[regex(r"[ \t]+", logos::skip)]
    #[allow(dead_code)]
    _Whitespace,
}

/// Lightweight wrapper around the `logos` lexer.
pub struct CommandLexer<'a> {
    inner: logos::Lexer<'a, Token<'a>>,
}

impl<'a> CommandLexer<'a> {
    /// Creates a new lexer over the supplied input slice.
    pub fn new(input: &'a str) -> Self {
        Self {
            inner: Token::lexer(input),
        }
    }

    /// Returns the next lexeme, skipping whitespace automatically.
    pub fn next(&mut self) -> Option<CommandLexeme<'a>> {
        while let Some(token) = self.inner.next() {
            match token {
                Ok(Token::Reboot) => return Some(CommandLexeme::Keyword(CommandKind::Reboot)),
                Ok(Token::Recovery) => return Some(CommandLexeme::Keyword(CommandKind::Recovery)),
                Ok(Token::Fault) => return Some(CommandLexeme::Keyword(CommandKind::Fault)),
                Ok(Token::Status) => return Some(CommandLexeme::Keyword(CommandKind::Status)),
                Ok(Token::Help) => return Some(CommandLexeme::Keyword(CommandKind::Help)),
                Ok(Token::Duration(value)) => return Some(CommandLexeme::Duration(value)),
                Ok(Token::Integer(value)) => return Some(CommandLexeme::Integer(value)),
                Ok(Token::Ident(value)) => return Some(CommandLexeme::Ident(value)),
                Ok(Token::Equals) => return Some(CommandLexeme::Equals),
                Ok(Token::CarriageReturn) | Ok(Token::LineFeed) => return Some(CommandLexeme::End),
                Ok(Token::_Whitespace) => continue,
                Err(()) => {
                    let slice = self.inner.slice();
                    return Some(CommandLexeme::Error(slice));
                }
            }
        }

        None
    }
}

/// Command parser built on top of `winnow`.
pub struct CommandParser;

impl CommandParser {
    /// Parses a command line and returns the root intent.
    pub fn parse(input: &str) -> Result<CommandIntent<'_>, ReplError> {
        Self::intent().parse(input).map_err(|_| ReplError::Syntax)
    }

    fn intent<'a>() -> impl Parser<&'a str, CommandIntent<'a>, ContextError> {
        move |input: &mut &'a str| {
            let kind = alt((
                "reboot".value(CommandKind::Reboot),
                "recovery".value(CommandKind::Recovery),
                "fault".value(CommandKind::Fault),
                "status".value(CommandKind::Status),
                "help".value(CommandKind::Help),
            ))
            .parse_next(input)?;

            multispace0.parse_next(input)?;
            let remainder = mem::take(input);

            Ok(CommandIntent::new(kind, remainder.trim()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyTransport;

    impl ReplTransport for DummyTransport {
        fn link(&self) -> ReplLink {
            ReplLink::UsbCdc0
        }
    }

    #[derive(Default)]
    struct DummyDispatcher {
        hits: usize,
        last_kind: Option<CommandKind>,
    }

    impl CommandDispatcher for DummyDispatcher {
        fn dispatch<'line>(&mut self, intent: CommandIntent<'line>) -> Result<(), DispatchError> {
            self.hits += 1;
            self.last_kind = Some(intent.kind);
            Ok(())
        }
    }

    #[test]
    fn lexer_yields_keywords_and_identifiers() {
        let mut lexer = CommandLexer::new("reboot now");
        assert_eq!(
            lexer.next(),
            Some(CommandLexeme::Keyword(CommandKind::Reboot))
        );
        assert_eq!(lexer.next(), Some(CommandLexeme::Ident("now")));
    }

    #[test]
    fn parser_extracts_command_kind() {
        let intent = CommandParser::parse("recovery enter").unwrap();
        assert_eq!(intent.kind, CommandKind::Recovery);
        assert_eq!(intent.remainder, "enter");
    }

    #[test]
    fn session_routes_complete_lines() {
        let transport = DummyTransport;
        let dispatcher = DummyDispatcher::default();
        let mut session = ReplSession::new(transport, dispatcher).unwrap();

        session.on_connect();
        for byte in b"status\n" {
            session.ingest(*byte).unwrap();
        }

        assert_eq!(session.buffer.len(), 0);
        assert_eq!(session.dispatcher.hits, 1);
        assert_eq!(session.dispatcher.last_kind, Some(CommandKind::Status));
    }

    #[test]
    fn overflow_is_reported() {
        let transport = DummyTransport;
        let dispatcher = DummyDispatcher::default();
        let mut session = ReplSession::new(transport, dispatcher).unwrap();
        session.on_connect();

        for _ in 0..MAX_LINE_LEN {
            session.ingest(b'a').unwrap();
        }

        let result = session.ingest(b'b');
        assert!(matches!(result, Err(ReplError::LineOverflow)));
    }
}
