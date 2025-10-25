#![allow(clippy::module_name_repetitions)]

//! Lexer and parser for the controller REPL.
//!
//! This module exposes an embedded-friendly lexer/parser pipeline. The lexer
//! uses `logos` to produce a bounded token stream, while the parser composes
//! `winnow` combinators over those tokens to build structured command values.

use core::fmt;
use core::ops::Range;
use core::time::Duration;

use heapless::Vec as HeaplessVec;
use logos::Logos;
use winnow::combinator::{alt, cut_err, opt, preceded};
#[allow(deprecated)]
use winnow::error::ErrorKind;
use winnow::error::{ErrMode, ParserError};
use winnow::prelude::*;
use winnow::stream::Stream;

/// Maximum number of tokens produced per REPL line. Commands remain short and bounded.
pub const MAX_TOKENS: usize = 32;

/// Lexical token kinds recognized by the REPL grammar.
#[derive(Logos, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    /// Duration literal ending in `ms` or `s`.
    #[regex(r"[0-9]+(?:ms|s)", priority = 2)]
    Duration,
    /// Unsuffixed integer literal.
    #[regex(r"[0-9]+")]
    Integer,
    /// Identifier or keyword (case-insensitive match performed later).
    #[regex(r"[A-Za-z][A-Za-z0-9-]*")]
    Ident,
    /// CLI-style flag placeholder (future extension).
    #[regex(r"-{1,2}[A-Za-z][A-Za-z0-9-]*")]
    Flag,
    /// Equals sign for key/value assignments.
    #[token("=")]
    Equals,
    /// Comma separator.
    #[token(",")]
    Comma,
    /// Inline whitespace is ignored.
    #[regex(r"[ \t]+", logos::skip)]
    Whitespace,
    /// End-of-line token (`\r`, `\n`, or `\r\n`).
    #[token("\r\n")]
    #[token("\n")]
    #[token("\r")]
    Eol,
    /// Pseudo variant used when the lexer encounters unsupported input.
    Error,
}

/// Token emitted by the lexer with a byte span back into the source line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: TokenKind,
    pub lexeme: &'a str,
    pub span: Range<usize>,
}

/// Bounded token buffer to avoid dynamic allocation in `no_std` environments.
pub type TokenBuffer<'a> = HeaplessVec<Token<'a>, MAX_TOKENS>;

/// Lexer errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LexError {
    /// Input produced more tokens than the static buffer allows.
    TooManyTokens { processed: usize },
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::TooManyTokens { processed } => {
                write!(f, "token buffer exhausted after {processed} items")
            }
        }
    }
}

/// Grammar errors emitted by the parser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrammarErrorKind<'a> {
    UnexpectedToken {
        expected: &'static str,
        found: Option<TokenKind>,
        span: Range<usize>,
    },
    UnexpectedEnd {
        expected: &'static str,
    },
    InvalidInteger {
        span: Range<usize>,
    },
    InvalidDuration {
        span: Range<usize>,
    },
    InvalidToken {
        span: Range<usize>,
        lexeme: &'a str,
    },
}

impl<'a> fmt::Display for GrammarErrorKind<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GrammarErrorKind::UnexpectedToken {
                expected,
                found,
                span,
            } => write!(f, "expected {expected}, found {found:?} at {span:?}"),
            GrammarErrorKind::UnexpectedEnd { expected } => {
                write!(f, "unexpected end of input, expected {expected}")
            }
            GrammarErrorKind::InvalidInteger { span } => {
                write!(f, "invalid integer literal at {span:?}")
            }
            GrammarErrorKind::InvalidDuration { span } => {
                write!(f, "invalid duration literal at {span:?}")
            }
            GrammarErrorKind::InvalidToken { span, lexeme } => {
                write!(f, "unsupported token `{lexeme}` at {span:?}")
            }
        }
    }
}

/// Wrapper type enabling a consistent error surface for consumers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrammarError<'a> {
    pub kind: GrammarErrorKind<'a>,
}

impl<'a> fmt::Display for GrammarError<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl<'a> GrammarError<'a> {
    fn unexpected(expected: &'static str, token: Option<&Token<'a>>) -> Self {
        GrammarError {
            kind: match token {
                Some(tok) => GrammarErrorKind::UnexpectedToken {
                    expected,
                    found: Some(tok.kind),
                    span: tok.span.clone(),
                },
                None => GrammarErrorKind::UnexpectedEnd { expected },
            },
        }
    }

    fn invalid_integer(token: &Token<'a>) -> Self {
        GrammarError {
            kind: GrammarErrorKind::InvalidInteger {
                span: token.span.clone(),
            },
        }
    }

    fn invalid_duration(token: &Token<'a>) -> Self {
        GrammarError {
            kind: GrammarErrorKind::InvalidDuration {
                span: token.span.clone(),
            },
        }
    }

    fn invalid_token(token: &Token<'a>) -> Self {
        GrammarError {
            kind: GrammarErrorKind::InvalidToken {
                span: token.span.clone(),
                lexeme: token.lexeme,
            },
        }
    }
}

type Input<'src, 'slice> = &'slice [Token<'src>];

#[allow(deprecated)]
impl<'src, 'slice> ParserError<Input<'src, 'slice>> for GrammarError<'src>
where
    'src: 'slice,
{
    fn from_error_kind(input: &Input<'src, 'slice>, _kind: ErrorKind) -> Self {
        GrammarError::unexpected("token", input.first())
    }

    fn append(
        self,
        _input: &Input<'src, 'slice>,
        _token_start: &<Input<'src, 'slice> as Stream>::Checkpoint,
        _kind: ErrorKind,
    ) -> Self {
        self
    }

    fn or(self, other: Self) -> Self {
        other
    }
}

/// Combined lex/parse error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError<'a> {
    Lex(LexError),
    Grammar(GrammarError<'a>),
}

impl<'a> fmt::Display for ParseError<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Lex(err) => err.fmt(f),
            ParseError::Grammar(err) => err.fmt(f),
        }
    }
}

/// Structured commands produced by the parser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command<'a> {
    Reboot(RebootCommand),
    Recovery(RecoveryCommand),
    Fault(FaultCommand),
    Status,
    Help(HelpCommand<'a>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RebootCommand {
    Now,
    Delay(Duration),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryCommand {
    Enter,
    Exit,
    Now,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaultCommand {
    pub retries: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelpCommand<'a> {
    pub topic: Option<&'a str>,
}

/// Tokenize the provided line.
pub fn lex(line: &str) -> Result<TokenBuffer<'_>, LexError> {
    let mut buffer = TokenBuffer::new();
    let mut lexer = TokenKind::lexer(line);
    let mut count = 0usize;

    while let Some(result) = lexer.next() {
        count += 1;
        let span = lexer.span();
        let lexeme = lexer.slice();
        let kind = match result {
            Ok(kind) => kind,
            Err(_) => TokenKind::Error,
        };

        if buffer.push(Token { kind, lexeme, span }).is_err() {
            return Err(LexError::TooManyTokens { processed: count });
        }
    }

    Ok(buffer)
}

/// Parse a REPL command from the provided line.
pub fn parse(line: &str) -> Result<Command<'_>, ParseError<'_>> {
    let tokens = lex(line).map_err(ParseError::Lex)?;

    for token in tokens.iter() {
        if token.kind == TokenKind::Error {
            return Err(ParseError::Grammar(GrammarError::invalid_token(token)));
        }
    }

    let mut input = tokens.as_slice();
    match command().parse_next(&mut input) {
        Ok(cmd) => {
            while let Some((token, rest)) = input.split_first() {
                if token.kind == TokenKind::Eol {
                    input = rest;
                } else {
                    return Err(ParseError::Grammar(GrammarError::unexpected(
                        "end of command",
                        Some(token),
                    )));
                }
            }
            Ok(cmd)
        }
        Err(ErrMode::Backtrack(err)) | Err(ErrMode::Cut(err)) => Err(ParseError::Grammar(err)),
        Err(ErrMode::Incomplete(_)) => Err(ParseError::Grammar(GrammarError::unexpected(
            "token",
            input.first(),
        ))),
    }
}

fn command<'src, 'slice>() -> impl Parser<Input<'src, 'slice>, Command<'src>, GrammarError<'src>>
where
    'src: 'slice,
{
    alt((
        parse_reboot().map(Command::Reboot),
        parse_recovery().map(Command::Recovery),
        parse_fault().map(Command::Fault),
        parse_status(),
        parse_help().map(Command::Help),
    ))
}

fn parse_reboot<'src, 'slice>()
-> impl Parser<Input<'src, 'slice>, RebootCommand, GrammarError<'src>>
where
    'src: 'slice,
{
    move |input: &mut Input<'src, 'slice>| {
        let _ = keyword("reboot").parse_next(input)?;
        opt(alt((
            keyword("now").map(|_| RebootCommand::Now),
            parse_delay_arg(),
        )))
        .map(|opt| opt.unwrap_or(RebootCommand::Now))
        .parse_next(input)
    }
}

fn parse_delay_arg<'src, 'slice>()
-> impl Parser<Input<'src, 'slice>, RebootCommand, GrammarError<'src>>
where
    'src: 'slice,
{
    move |input: &mut Input<'src, 'slice>| {
        let _ = keyword("delay").parse_next(input)?;
        let duration_token = expect_kind(TokenKind::Duration, "duration").parse_next(input)?;
        let duration = parse_duration(&duration_token).map_err(ErrMode::Cut)?;
        Ok(RebootCommand::Delay(duration))
    }
}

fn parse_recovery<'src, 'slice>()
-> impl Parser<Input<'src, 'slice>, RecoveryCommand, GrammarError<'src>>
where
    'src: 'slice,
{
    move |input: &mut Input<'src, 'slice>| {
        let _ = keyword("recovery").parse_next(input)?;
        opt(alt((
            keyword("enter").map(|_| RecoveryCommand::Enter),
            keyword("exit").map(|_| RecoveryCommand::Exit),
            keyword("now").map(|_| RecoveryCommand::Now),
        )))
        .map(|opt| opt.unwrap_or(RecoveryCommand::Enter))
        .parse_next(input)
    }
}

fn parse_fault<'src, 'slice>() -> impl Parser<Input<'src, 'slice>, FaultCommand, GrammarError<'src>>
where
    'src: 'slice,
{
    move |input: &mut Input<'src, 'slice>| {
        let _ = keyword("fault").parse_next(input)?;
        let _ = keyword("recover").parse_next(input)?;
        let retries = opt(preceded(
            keyword("retries"),
            cut_err(preceded(
                expect_kind(TokenKind::Equals, "="),
                expect_kind(TokenKind::Integer, "integer"),
            )),
        ))
        .parse_next(input)?;

        let retries = match retries {
            Some(token) => {
                let value = parse_integer(&token).map_err(ErrMode::Cut)?;
                Some(value)
            }
            None => None,
        };

        Ok(FaultCommand { retries })
    }
}

fn parse_status<'src, 'slice>()
-> impl Parser<Input<'src, 'slice>, Command<'src>, GrammarError<'src>>
where
    'src: 'slice,
{
    move |input: &mut Input<'src, 'slice>| {
        let _ = keyword("status").parse_next(input)?;
        Ok(Command::Status)
    }
}

fn parse_help<'src, 'slice>()
-> impl Parser<Input<'src, 'slice>, HelpCommand<'src>, GrammarError<'src>>
where
    'src: 'slice,
{
    move |input: &mut Input<'src, 'slice>| {
        let _ = keyword("help").parse_next(input)?;
        let topic = opt(expect_kind(TokenKind::Ident, "identifier"))
            .parse_next(input)?
            .map(|tok| tok.lexeme);
        Ok(HelpCommand { topic })
    }
}

fn keyword<'src, 'slice>(
    expected: &'static str,
) -> impl Parser<Input<'src, 'slice>, &'src str, GrammarError<'src>>
where
    'src: 'slice,
{
    move |input: &mut Input<'src, 'slice>| {
        let token = expect_kind(TokenKind::Ident, expected).parse_next(input)?;
        if token.lexeme.eq_ignore_ascii_case(expected) {
            Ok(token.lexeme)
        } else {
            Err(ErrMode::Backtrack(GrammarError::unexpected(
                expected,
                Some(&token),
            )))
        }
    }
}

fn expect_kind<'src, 'slice>(
    kind: TokenKind,
    label: &'static str,
) -> impl Parser<Input<'src, 'slice>, Token<'src>, GrammarError<'src>>
where
    'src: 'slice,
{
    move |input: &mut Input<'src, 'slice>| match input.split_first() {
        Some((token, rest)) if token.kind == kind => {
            *input = rest;
            Ok(token.clone())
        }
        Some((token, _)) => Err(ErrMode::Backtrack(GrammarError::unexpected(
            label,
            Some(token),
        ))),
        None => Err(ErrMode::Backtrack(GrammarError::unexpected(label, None))),
    }
}

fn parse_integer<'a>(token: &Token<'a>) -> Result<u8, GrammarError<'a>> {
    token
        .lexeme
        .parse::<u8>()
        .map_err(|_| GrammarError::invalid_integer(token))
}

fn parse_duration<'a>(token: &Token<'a>) -> Result<Duration, GrammarError<'a>> {
    let text = token.lexeme;
    if let Some(rest) = text.strip_suffix("ms") {
        let millis = rest
            .parse::<u32>()
            .map_err(|_| GrammarError::invalid_duration(token))?;
        Ok(Duration::from_millis(millis.into()))
    } else if let Some(rest) = text.strip_suffix('s') {
        let seconds = rest
            .parse::<u32>()
            .map_err(|_| GrammarError::invalid_duration(token))?;
        Ok(Duration::from_secs(seconds.into()))
    } else {
        Err(GrammarError::invalid_duration(token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(input: &str) -> Command<'_> {
        parse(input).expect("command should parse")
    }

    #[test]
    fn parses_reboot_now() {
        assert_eq!(parse_ok("reboot now"), Command::Reboot(RebootCommand::Now));
    }

    #[test]
    fn parses_reboot_delay_ms() {
        match parse_ok("reboot delay 150ms") {
            Command::Reboot(RebootCommand::Delay(duration)) => {
                assert_eq!(duration, Duration::from_millis(150));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_reboot_delay_seconds() {
        match parse_ok("reboot delay 2s") {
            Command::Reboot(RebootCommand::Delay(duration)) => {
                assert_eq!(duration, Duration::from_secs(2));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_recovery_variants() {
        assert_eq!(
            parse_ok("recovery enter"),
            Command::Recovery(RecoveryCommand::Enter)
        );
        assert_eq!(
            parse_ok("recovery exit"),
            Command::Recovery(RecoveryCommand::Exit)
        );
        assert_eq!(
            parse_ok("recovery now"),
            Command::Recovery(RecoveryCommand::Now)
        );
    }

    #[test]
    fn parser_handles_fault_variants() {
        assert_eq!(
            parse_ok("fault recover"),
            Command::Fault(FaultCommand { retries: None })
        );

        match parse_ok("fault recover retries=2") {
            Command::Fault(FaultCommand { retries: Some(2) }) => {}
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_status() {
        assert_eq!(parse_ok("status"), Command::Status);
    }

    #[test]
    fn parses_help_topic() {
        assert_eq!(
            parse_ok("help reboot"),
            Command::Help(HelpCommand {
                topic: Some("reboot"),
            })
        );
    }

    #[test]
    fn rejects_invalid_token() {
        match parse("reboot now$") {
            Err(ParseError::Grammar(err)) => {
                assert!(matches!(err.kind, GrammarErrorKind::InvalidToken { .. }))
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn supports_case_insensitive_keywords() {
        assert_eq!(parse_ok("ReBoOt Now"), Command::Reboot(RebootCommand::Now));
    }
}
