#![allow(clippy::module_name_repetitions)]

//! Lexer and parser for the controller REPL.
//!
//! This module exposes an embedded-friendly lexer/parser pipeline. The lexer
//! uses `regal` to produce a bounded token stream, while the parser composes
//! `winnow` combinators over those tokens to build structured command values.

use super::catalog::{
    self, ChoiceBranch, ChoiceTag, CommandTag, DefaultChoice, HelpTopics, Node, SubcommandBranch,
    SubcommandTag, ValueSpec,
};
use core::fmt;
use core::ops::Range;
use core::time::Duration;

use heapless::{String as HeaplessString, Vec as HeaplessVec};
use regal::IncrementalError;
use regal::TokenCache;
use regal_macros::RegalLexer;
use winnow::error::{ErrMode, ParserError};
use winnow::prelude::*;
use winnow::stream::Stream;

/// Maximum number of tokens produced per REPL line. Commands remain short and bounded.
pub const MAX_TOKENS: usize = 32;
const MAX_CACHE_RECORDS: usize = MAX_TOKENS * 2;

/// Lexical token kinds recognized by the REPL grammar.
#[derive(RegalLexer, Clone, Copy, Debug, PartialEq, Eq, Default)]
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
    #[regex(r"[ \t]+", skip)]
    Whitespace,
    /// End-of-line token (`\r`, `\n`, or `\r\n`).
    #[token("\r\n")]
    #[token("\n")]
    #[token("\r")]
    Eol,
    /// Pseudo variant used when the lexer encounters unsupported input.
    #[default]
    #[regex(r".", priority = 1024)]
    Error,
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            TokenKind::Duration => "duration literal",
            TokenKind::Integer => "integer literal",
            TokenKind::Ident => "identifier",
            TokenKind::Flag => "flag",
            TokenKind::Equals => "equals sign",
            TokenKind::Comma => "comma",
            TokenKind::Whitespace => "whitespace",
            TokenKind::Eol => "end-of-line marker",
            TokenKind::Error => "unsupported token",
        };
        f.write_str(label)
    }
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
    /// Underlying lexer reported an unrecoverable error.
    Engine,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::TooManyTokens { processed } => {
                write!(f, "token buffer exhausted after {processed} items")
            }
            LexError::Engine => write!(f, "lexer engine error"),
        }
    }
}

/// Grammar errors emitted by the parser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrammarErrorKind {
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
        lexeme: HeaplessString<32>,
    },
}

impl fmt::Display for GrammarErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GrammarErrorKind::UnexpectedToken {
                expected,
                found,
                span,
            } => match found {
                Some(kind) => write!(
                    f,
                    "expected {expected}, found {kind} at {}",
                    SpanDisplay(span)
                ),
                None => write!(
                    f,
                    "expected {expected}, found end of input at {}",
                    SpanDisplay(span)
                ),
            },
            GrammarErrorKind::UnexpectedEnd { expected } => {
                write!(f, "unexpected end of input, expected {expected}")
            }
            GrammarErrorKind::InvalidInteger { span } => {
                write!(f, "invalid integer literal at {}", SpanDisplay(span))
            }
            GrammarErrorKind::InvalidDuration { span } => {
                write!(f, "invalid duration literal at {}", SpanDisplay(span))
            }
            GrammarErrorKind::InvalidToken { span, lexeme } => {
                write!(f, "unsupported token `{lexeme}` at {}", SpanDisplay(span))
            }
        }
    }
}

/// Wrapper type enabling a consistent error surface for consumers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrammarError {
    pub kind: GrammarErrorKind,
}

struct SpanDisplay<'a>(&'a Range<usize>);

impl fmt::Display for SpanDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.0.start, self.0.end)
    }
}

impl fmt::Display for GrammarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl GrammarError {
    fn unexpected(expected: &'static str, token: Option<&Token<'_>>) -> Self {
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

    fn invalid_integer(token: &Token<'_>) -> Self {
        GrammarError {
            kind: GrammarErrorKind::InvalidInteger {
                span: token.span.clone(),
            },
        }
    }

    fn invalid_duration(token: &Token<'_>) -> Self {
        GrammarError {
            kind: GrammarErrorKind::InvalidDuration {
                span: token.span.clone(),
            },
        }
    }

    fn invalid_token(token: &Token<'_>) -> Self {
        let mut lexeme = HeaplessString::<32>::new();
        for ch in token.lexeme.chars() {
            if lexeme.push(ch).is_err() {
                break;
            }
        }

        GrammarError {
            kind: GrammarErrorKind::InvalidToken {
                span: token.span.clone(),
                lexeme,
            },
        }
    }
}

type Input<'src, 'slice> = &'slice [Token<'src>];

impl<'src, 'slice> ParserError<Input<'src, 'slice>> for GrammarError
where
    'src: 'slice,
{
    type Inner = Self;

    fn from_input(input: &Input<'src, 'slice>) -> Self {
        GrammarError::unexpected("token", input.first())
    }

    fn append(
        self,
        _input: &Input<'src, 'slice>,
        _token_start: &<Input<'src, 'slice> as Stream>::Checkpoint,
    ) -> Self {
        self
    }

    fn or(self, other: Self) -> Self {
        other
    }

    fn into_inner(self) -> Result<Self::Inner, Self> {
        Ok(self)
    }
}

/// Combined lex/parse error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    Lex(LexError),
    Grammar(GrammarError),
}

impl fmt::Display for ParseError {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebootCommand {
    Now,
    Delay(Duration),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

pub(crate) fn parse_tokens_partial<'src, 'slice>(
    tokens: &'slice [Token<'src>],
) -> Result<(Command<'src>, &'slice [Token<'src>]), GrammarError>
where
    'src: 'slice,
{
    let mut input = tokens;
    match command().parse_next(&mut input) {
        Ok(cmd) => Ok((cmd, input)),
        Err(ErrMode::Backtrack(err) | ErrMode::Cut(err)) => Err(err),
        Err(ErrMode::Incomplete(_)) => Err(GrammarError::unexpected("token", input.first())),
    }
}

/// Tokenize the provided line.
///
/// # Errors
/// Returns a [`LexError`] when the line produces more tokens than the buffer
/// can hold or when incremental compilation fails.
pub fn lex(line: &str) -> Result<TokenBuffer<'_>, LexError> {
    let compiled = TokenKind::lexer();
    let mut cache: TokenCache<TokenKind, MAX_CACHE_RECORDS> = TokenCache::new();
    let partial = cache
        .rebuild(compiled, line)
        .map_err(|error| map_incremental_error(&error))?;
    let mut buffer = TokenBuffer::new();

    for record in cache.tokens() {
        if record.skipped {
            continue;
        }

        let span = record.start..record.end;
        let lexeme = &line[span.clone()];
        if buffer
            .push(Token {
                kind: record.token,
                lexeme,
                span,
            })
            .is_err()
        {
            return Err(LexError::TooManyTokens {
                processed: buffer.len() + 1,
            });
        }
    }

    if let Some(partial) = partial.filter(|partial| !partial.fragment.is_empty()) {
        let start = partial.start;
        let end = start + partial.fragment.len();
        let span = start..end;
        if buffer
            .push(Token {
                kind: TokenKind::Error,
                lexeme: partial.fragment,
                span,
            })
            .is_err()
        {
            return Err(LexError::TooManyTokens {
                processed: buffer.len() + 1,
            });
        }
    }

    Ok(buffer)
}

fn map_incremental_error(error: &IncrementalError) -> LexError {
    match error {
        IncrementalError::TokenOverflow => LexError::TooManyTokens {
            processed: MAX_TOKENS,
        },
        _ => LexError::Engine,
    }
}

/// Parse a REPL command from the provided line.
///
/// # Errors
/// Returns a [`ParseError`] when the line is empty, fails lexical analysis, or
/// does not match the grammar.
pub fn parse(line: &str) -> Result<Command<'_>, ParseError> {
    let tokens = lex(line).map_err(ParseError::Lex)?;

    for token in &tokens {
        if token.kind == TokenKind::Error {
            return Err(ParseError::Grammar(GrammarError::invalid_token(token)));
        }
    }

    let (command, mut rest) =
        parse_tokens_partial(tokens.as_slice()).map_err(ParseError::Grammar)?;

    while let Some((token, remaining)) = rest.split_first() {
        if token.kind == TokenKind::Eol {
            rest = remaining;
        } else {
            return Err(ParseError::Grammar(GrammarError::unexpected(
                "end of command",
                Some(token),
            )));
        }
    }

    Ok(command)
}

fn command<'src, 'slice>() -> impl Parser<Input<'src, 'slice>, Command<'src>, ErrMode<GrammarError>>
where
    'src: 'slice,
{
    move |input: &mut Input<'src, 'slice>| {
        let snapshot = *input;
        let command_token = expect_kind(TokenKind::Ident, "command keyword").parse_next(input)?;

        if let Some(spec) = catalog::find(command_token.lexeme) {
            let mut state = CommandState::new(spec.tag);
            parse_node(spec.grammar, input, &mut state)?;
            state.finish()
        } else {
            *input = snapshot;
            Err(ErrMode::Backtrack(GrammarError::unexpected(
                "command keyword",
                Some(&command_token),
            )))
        }
    }
}

fn parse_node<'src, 'slice>(
    node: &'static Node,
    input: &mut Input<'src, 'slice>,
    state: &mut CommandState<'src>,
) -> Result<(), ErrMode<GrammarError>>
where
    'src: 'slice,
{
    match node {
        Node::End => Ok(()),
        Node::OptionalChoice { choices, default } => {
            parse_optional_choice(input, choices, *default, state)
        }
        Node::Subcommands(branches) => parse_subcommands(input, branches, state),
        Node::Topic { topics, next } => {
            parse_topic(*topics, input, state)?;
            parse_node(next, input, state)
        }
    }
}

fn parse_optional_choice<'src, 'slice>(
    input: &mut Input<'src, 'slice>,
    choices: &'static [ChoiceBranch],
    default: Option<DefaultChoice>,
    state: &mut CommandState<'src>,
) -> Result<(), ErrMode<GrammarError>>
where
    'src: 'slice,
{
    match input.split_first() {
        Some((token, rest)) if token.kind == TokenKind::Ident => {
            if let Some(branch) = find_choice(choices, token.lexeme) {
                *input = rest;
                parse_choice_branch(input, branch, state)
            } else {
                Err(ErrMode::Backtrack(GrammarError::unexpected(
                    choice_expected_label(choices),
                    Some(token),
                )))
            }
        }
        Some((token, _)) if token.kind == TokenKind::Eol => {
            if let Some(default_choice) = default {
                state.apply_default_choice(default_choice.tag)?;
                parse_node(default_choice.next, input, state)
            } else {
                Ok(())
            }
        }
        Some((token, _)) => Err(ErrMode::Backtrack(GrammarError::unexpected(
            choice_expected_label(choices),
            Some(token),
        ))),
        None => {
            if let Some(default_choice) = default {
                state.apply_default_choice(default_choice.tag)?;
                parse_node(default_choice.next, input, state)
            } else {
                Ok(())
            }
        }
    }
}

fn parse_choice_branch<'src, 'slice>(
    input: &mut Input<'src, 'slice>,
    branch: &'static ChoiceBranch,
    state: &mut CommandState<'src>,
) -> Result<(), ErrMode<GrammarError>>
where
    'src: 'slice,
{
    let value = parse_value(input, branch.value)?;
    state.apply_choice(branch.tag, value)?;
    parse_node(branch.next, input, state)
}

fn parse_subcommands<'src, 'slice>(
    input: &mut Input<'src, 'slice>,
    branches: &'static [SubcommandBranch],
    state: &mut CommandState<'src>,
) -> Result<(), ErrMode<GrammarError>>
where
    'src: 'slice,
{
    match input.split_first() {
        Some((token, rest)) if token.kind == TokenKind::Ident => {
            if let Some(branch) = branches
                .iter()
                .find(|candidate| candidate.name.eq_ignore_ascii_case(token.lexeme))
            {
                *input = rest;
                state.set_subcommand(branch.tag);
                parse_node(branch.grammar, input, state)
            } else {
                Err(ErrMode::Backtrack(GrammarError::unexpected(
                    branches.first().map_or("subcommand", |branch| branch.name),
                    Some(token),
                )))
            }
        }
        Some((token, _)) if token.kind == TokenKind::Eol => {
            Err(ErrMode::Backtrack(GrammarError::unexpected(
                branches.first().map_or("subcommand", |branch| branch.name),
                Some(token),
            )))
        }
        Some((token, _)) => Err(ErrMode::Backtrack(GrammarError::unexpected(
            branches.first().map_or("subcommand", |branch| branch.name),
            Some(token),
        ))),
        None => Err(ErrMode::Backtrack(GrammarError::unexpected(
            branches.first().map_or("subcommand", |branch| branch.name),
            None,
        ))),
    }
}

fn parse_topic<'src, 'slice>(
    _topics: HelpTopics,
    input: &mut Input<'src, 'slice>,
    state: &mut CommandState<'src>,
) -> Result<(), ErrMode<GrammarError>>
where
    'src: 'slice,
{
    state.set_topic(None);

    match input.split_first() {
        Some((token, rest)) if token.kind == TokenKind::Ident => {
            state.set_topic(Some(token.lexeme));
            *input = rest;
            Ok(())
        }
        Some((token, _)) if token.kind == TokenKind::Eol => Ok(()),
        Some((token, _)) => Err(ErrMode::Backtrack(GrammarError::unexpected(
            "identifier",
            Some(token),
        ))),
        None => Ok(()),
    }
}

fn parse_value<'src, 'slice>(
    input: &mut Input<'src, 'slice>,
    spec: ValueSpec,
) -> Result<ChoiceValue, ErrMode<GrammarError>>
where
    'src: 'slice,
{
    match spec {
        ValueSpec::None => Ok(ChoiceValue::None),
        ValueSpec::Duration => {
            let duration_token = expect_kind(TokenKind::Duration, "duration").parse_next(input)?;
            let duration = parse_duration(&duration_token).map_err(ErrMode::Cut)?;
            Ok(ChoiceValue::Duration(duration))
        }
        ValueSpec::IntegerAssignment { .. } => {
            let _ = expect_kind(TokenKind::Equals, "=").parse_next(input)?;
            let integer_token = expect_kind(TokenKind::Integer, "integer").parse_next(input)?;
            let value = parse_integer(&integer_token).map_err(ErrMode::Cut)?;
            Ok(ChoiceValue::Integer(value))
        }
    }
}

fn find_choice(choices: &'static [ChoiceBranch], lexeme: &str) -> Option<&'static ChoiceBranch> {
    choices
        .iter()
        .find(|choice| choice.keyword.eq_ignore_ascii_case(lexeme))
}

fn choice_expected_label(choices: &'static [ChoiceBranch]) -> &'static str {
    choices.first().map_or("keyword", |choice| choice.keyword)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChoiceValue {
    None,
    Duration(Duration),
    Integer(u8),
}

enum CommandState<'a> {
    Reboot {
        action: Option<RebootCommand>,
    },
    Recovery {
        action: Option<RecoveryCommand>,
    },
    Fault {
        subcommand: Option<SubcommandTag>,
        retries: Option<u8>,
    },
    Status,
    Help {
        topic: Option<&'a str>,
    },
}

impl<'a> CommandState<'a> {
    fn new(tag: CommandTag) -> Self {
        match tag {
            CommandTag::Reboot => CommandState::Reboot { action: None },
            CommandTag::Recovery => CommandState::Recovery { action: None },
            CommandTag::Fault => CommandState::Fault {
                subcommand: None,
                retries: None,
            },
            CommandTag::Status => CommandState::Status,
            CommandTag::Help => CommandState::Help { topic: None },
        }
    }

    fn apply_choice(
        &mut self,
        tag: ChoiceTag,
        value: ChoiceValue,
    ) -> Result<(), ErrMode<GrammarError>> {
        match (self, tag, value) {
            (CommandState::Reboot { action }, ChoiceTag::RebootNow, _) => {
                *action = Some(RebootCommand::Now);
                Ok(())
            }
            (
                CommandState::Reboot { action },
                ChoiceTag::RebootDelay,
                ChoiceValue::Duration(duration),
            ) => {
                *action = Some(RebootCommand::Delay(duration));
                Ok(())
            }
            (CommandState::Recovery { action }, ChoiceTag::RecoveryEnter, _) => {
                *action = Some(RecoveryCommand::Enter);
                Ok(())
            }
            (CommandState::Recovery { action }, ChoiceTag::RecoveryExit, _) => {
                *action = Some(RecoveryCommand::Exit);
                Ok(())
            }
            (CommandState::Recovery { action }, ChoiceTag::RecoveryNow, _) => {
                *action = Some(RecoveryCommand::Now);
                Ok(())
            }
            (
                CommandState::Fault { retries, .. },
                ChoiceTag::FaultRetries,
                ChoiceValue::Integer(value),
            ) => {
                *retries = Some(value);
                Ok(())
            }
            (_, unexpected_tag, _) => {
                let _ = unexpected_tag;
                Err(ErrMode::Backtrack(GrammarError::unexpected("choice", None)))
            }
        }
    }

    fn apply_default_choice(&mut self, tag: ChoiceTag) -> Result<(), ErrMode<GrammarError>> {
        self.apply_choice(tag, ChoiceValue::None)
    }

    fn set_subcommand(&mut self, tag: SubcommandTag) {
        if let CommandState::Fault { subcommand, .. } = self {
            *subcommand = Some(tag);
        } else {
            unreachable!("subcommands only apply to fault commands");
        }
    }

    fn set_topic(&mut self, topic: Option<&'a str>) {
        if let CommandState::Help { topic: slot } = self {
            *slot = topic;
        }
    }

    fn finish(self) -> Result<Command<'a>, ErrMode<GrammarError>> {
        match self {
            CommandState::Reboot {
                action: Some(command),
            } => Ok(Command::Reboot(command)),
            CommandState::Recovery {
                action: Some(command),
            } => Ok(Command::Recovery(command)),
            CommandState::Fault {
                subcommand: Some(SubcommandTag::FaultRecover),
                retries,
            } => Ok(Command::Fault(FaultCommand { retries })),
            CommandState::Status => Ok(Command::Status),
            CommandState::Help { topic } => Ok(Command::Help(HelpCommand { topic })),
            CommandState::Reboot { action: None } => Err(ErrMode::Backtrack(
                GrammarError::unexpected("reboot argument", None),
            )),
            CommandState::Recovery { action: None } => Err(ErrMode::Backtrack(
                GrammarError::unexpected("recovery argument", None),
            )),
            CommandState::Fault {
                subcommand: None, ..
            } => Err(ErrMode::Backtrack(GrammarError::unexpected(
                "fault subcommand",
                None,
            ))),
        }
    }
}

fn expect_kind<'src, 'slice>(
    kind: TokenKind,
    label: &'static str,
) -> impl Parser<Input<'src, 'slice>, Token<'src>, ErrMode<GrammarError>>
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

fn parse_integer(token: &Token<'_>) -> Result<u8, GrammarError> {
    token
        .lexeme
        .parse::<u8>()
        .map_err(|_| GrammarError::invalid_integer(token))
}

fn parse_duration(token: &Token<'_>) -> Result<Duration, GrammarError> {
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
                assert!(matches!(err.kind, GrammarErrorKind::InvalidToken { .. }));
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn supports_case_insensitive_keywords() {
        assert_eq!(parse_ok("ReBoOt Now"), Command::Reboot(RebootCommand::Now));
    }

    #[test]
    fn lexer_emits_error_token_for_unknown_symbol() {
        let tokens = lex("reboot now$").expect("lexing should succeed");
        let last = tokens.last().expect("expected at least one token");
        assert_eq!(last.kind, TokenKind::Error);
        assert_eq!(last.lexeme, "$");
    }
}
