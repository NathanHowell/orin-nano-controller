//! Grammar-aware completion engine shared by firmware and emulator REPLs.
//!
//! The line editor can invoke this module to look up suggestions based on the
//! current buffer contents and cursor position without pulling in `std`.

use super::grammar::{self, Command, GrammarErrorKind, Token, TokenKind};
use heapless::Vec as HeaplessVec;

const MAX_SUGGESTIONS: usize = 16;

const ROOT_COMMANDS: &[&str] = &["reboot", "recovery", "fault", "status", "help"];
const REBOOT_ARGS: &[&str] = &["now", "delay"];
const RECOVERY_ARGS: &[&str] = &["enter", "exit", "now"];
const FAULT_SUBCOMMANDS: &[&str] = &["recover"];
const FAULT_RETRY_VALUES: &[&str] = &["retries=1", "retries=2", "retries=3"];

/// Completion result returned to the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionResult {
    /// Replacement metadata to apply automatically when only one candidate
    /// matches or when a longer shared prefix exists across candidates.
    pub replacement: Option<Replacement>,
    /// Candidate list corresponding to the current cursor position. An empty
    /// list indicates that no completions were found.
    pub options: HeaplessVec<&'static str, MAX_SUGGESTIONS>,
}

/// Replacement metadata describing which portion of the buffer should be
/// substituted by the completion string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replacement {
    pub start: usize,
    pub end: usize,
    pub value: &'static str,
    pub append_space: bool,
}

/// Stateless completion engine that mirrors the REPL grammar.
#[derive(Default)]
pub struct CompletionEngine;

impl CompletionEngine {
    /// Creates a new completion engine.
    pub const fn new() -> Self {
        Self
    }

    /// Computes completions for the provided buffer at the supplied cursor
    /// position.
    ///
    /// The cursor must be positioned at a valid UTF-8 boundary; the caller is
    /// expected to enforce ASCII-only input.
    pub fn complete(&self, buffer: &str, cursor: usize) -> CompletionResult {
        if cursor > buffer.len() {
            return CompletionResult {
                replacement: None,
                options: HeaplessVec::new(),
            };
        }

        let upto_cursor = &buffer[..cursor];
        let prefix_start = token_start(upto_cursor);
        let prefix = &upto_cursor[prefix_start..];
        let leading = &upto_cursor[..prefix_start];

        let leading_tokens = match grammar::lex(leading) {
            Ok(tokens) => tokens,
            Err(_) => {
                return CompletionResult {
                    replacement: None,
                    options: HeaplessVec::new(),
                };
            }
        };

        if leading_tokens
            .iter()
            .any(|token| token.kind == TokenKind::Error)
        {
            return CompletionResult {
                replacement: None,
                options: HeaplessVec::new(),
            };
        }

        let context = determine_context(leading_tokens.as_slice());
        let candidates = match context {
            CompletionContext::Root => ROOT_COMMANDS,
            CompletionContext::RebootArg => REBOOT_ARGS,
            CompletionContext::RecoveryArg => RECOVERY_ARGS,
            CompletionContext::FaultKeyword => FAULT_SUBCOMMANDS,
            CompletionContext::FaultRetry => FAULT_RETRY_VALUES,
            CompletionContext::HelpTopic => ROOT_COMMANDS,
            CompletionContext::None => {
                return CompletionResult {
                    replacement: None,
                    options: HeaplessVec::new(),
                };
            }
        };

        let mut matches: HeaplessVec<&'static str, MAX_SUGGESTIONS> = HeaplessVec::new();
        for candidate in candidates {
            if starts_with_ignore_ascii_case(candidate, prefix) {
                let _ = matches.push(*candidate);
            }
        }

        if matches.is_empty() {
            return CompletionResult {
                replacement: None,
                options: matches,
            };
        }

        let matches_slice = matches.as_slice();
        let mut append_space = false;
        let replacement_value = if matches_slice.len() == 1 {
            let candidate = matches_slice[0];
            append_space = should_append_space(context, candidate);
            Some(candidate)
        } else {
            let lcp = longest_common_prefix(matches_slice);
            let shared = common_prefix_len_ignore_case(prefix, lcp);
            if lcp.len() > shared { Some(lcp) } else { None }
        };

        let replacement = replacement_value.map(|value| Replacement {
            start: prefix_start,
            end: cursor,
            value,
            append_space,
        });

        CompletionResult {
            replacement,
            options: matches,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompletionContext {
    Root,
    RebootArg,
    RecoveryArg,
    FaultKeyword,
    FaultRetry,
    HelpTopic,
    None,
}

fn determine_context(tokens: &[Token<'_>]) -> CompletionContext {
    if tokens.is_empty() {
        return CompletionContext::Root;
    }

    if tokens.iter().any(|token| token.kind == TokenKind::Error) {
        return CompletionContext::None;
    }

    match grammar::parse_tokens_partial(tokens) {
        Ok((command, _)) => classify_success(tokens, &command),
        Err(err) => classify_error(tokens, &err.kind),
    }
}

fn classify_success(tokens: &[Token<'_>], command: &Command<'_>) -> CompletionContext {
    match command {
        Command::Reboot(_) if tokens.len() == 1 => CompletionContext::RebootArg,
        Command::Recovery(_) if tokens.len() == 1 => CompletionContext::RecoveryArg,
        Command::Fault(_)
            if tokens.len() == 2 && equals_ignore_ascii_case(tokens[1].lexeme, "recover") =>
        {
            CompletionContext::FaultRetry
        }
        Command::Help(_) if tokens.len() == 1 => CompletionContext::HelpTopic,
        Command::Fault(_) if tokens.len() == 1 => CompletionContext::FaultKeyword,
        _ => infer_from_tokens(tokens),
    }
}

fn classify_error(tokens: &[Token<'_>], error: &GrammarErrorKind<'_>) -> CompletionContext {
    match error {
        GrammarErrorKind::UnexpectedEnd { expected } => match *expected {
            "reboot" | "recovery" | "fault" | "status" | "help" => CompletionContext::Root,
            "recover" => CompletionContext::FaultKeyword,
            "identifier" if first_token_is(tokens, "help") => CompletionContext::HelpTopic,
            _ => infer_from_tokens(tokens),
        },
        GrammarErrorKind::UnexpectedToken { expected, .. } => match *expected {
            "recover" => CompletionContext::FaultKeyword,
            "identifier" if first_token_is(tokens, "help") => CompletionContext::HelpTopic,
            _ => infer_from_tokens(tokens),
        },
        _ => infer_from_tokens(tokens),
    }
}

fn infer_from_tokens(tokens: &[Token<'_>]) -> CompletionContext {
    match tokens {
        [] => CompletionContext::Root,
        [first] if equals_ignore_ascii_case(first.lexeme, "reboot") => CompletionContext::RebootArg,
        [first] if equals_ignore_ascii_case(first.lexeme, "recovery") => {
            CompletionContext::RecoveryArg
        }
        [first] if equals_ignore_ascii_case(first.lexeme, "fault") => {
            CompletionContext::FaultKeyword
        }
        [first, second]
            if equals_ignore_ascii_case(first.lexeme, "fault")
                && equals_ignore_ascii_case(second.lexeme, "recover") =>
        {
            CompletionContext::FaultRetry
        }
        [first] if equals_ignore_ascii_case(first.lexeme, "help") => CompletionContext::HelpTopic,
        _ => CompletionContext::None,
    }
}

fn first_token_is(tokens: &[Token<'_>], expected: &str) -> bool {
    tokens
        .first()
        .map(|token| equals_ignore_ascii_case(token.lexeme, expected))
        .unwrap_or(false)
}

fn token_start(buffer: &str) -> usize {
    let mut index = buffer.len();
    let bytes = buffer.as_bytes();
    while index > 0 {
        let byte = bytes[index - 1];
        if byte == b' ' || byte == b'\t' {
            break;
        }
        index -= 1;
    }
    index
}

fn equals_ignore_ascii_case(lhs: &str, rhs: &str) -> bool {
    lhs.eq_ignore_ascii_case(rhs)
}

fn starts_with_ignore_ascii_case(candidate: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }

    if prefix.len() > candidate.len() {
        return false;
    }

    candidate[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn common_prefix_len_ignore_case(lhs: &str, rhs: &str) -> usize {
    lhs.as_bytes()
        .iter()
        .zip(rhs.as_bytes())
        .take_while(|(l, r)| l.eq_ignore_ascii_case(r))
        .count()
}

fn longest_common_prefix(candidates: &[&'static str]) -> &'static str {
    if let Some((first, rest)) = candidates.split_first() {
        let mut prefix = *first;
        for candidate in rest {
            let len = common_prefix_len_ignore_case(prefix, candidate);
            prefix = &prefix[..len];
            if prefix.is_empty() {
                break;
            }
        }
        prefix
    } else {
        ""
    }
}

fn should_append_space(context: CompletionContext, candidate: &'static str) -> bool {
    if !matches!(context, CompletionContext::Root) {
        return false;
    }

    let tokens = match grammar::lex(candidate) {
        Ok(tokens) => tokens,
        Err(_) => return false,
    };

    !matches!(
        determine_context(tokens.as_slice()),
        CompletionContext::Root | CompletionContext::None
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_options(
        result: CompletionResult,
    ) -> (
        Option<Replacement>,
        HeaplessVec<&'static str, MAX_SUGGESTIONS>,
    ) {
        assert!(
            !result.options.is_empty(),
            "expected suggestions but got no match"
        );
        (result.replacement, result.options)
    }

    #[test]
    fn offers_root_commands_from_empty_buffer() {
        let engine = CompletionEngine::new();
        let (replacement, options) = expect_options(engine.complete("", 0));
        assert!(replacement.is_none());
        assert_eq!(options.as_slice(), ROOT_COMMANDS);
    }

    #[test]
    fn filters_root_commands_by_prefix() {
        let engine = CompletionEngine::new();
        let (replacement, options) = expect_options(engine.complete("re", 2));
        assert!(replacement.is_none());
        assert_eq!(options.as_slice(), ["reboot", "recovery"]);
    }

    #[test]
    fn expands_unique_root_command() {
        let engine = CompletionEngine::new();
        let (replacement, options) = expect_options(engine.complete("reb", 3));
        let replacement = replacement.expect("expected replacement");
        assert_eq!(replacement.start, 0);
        assert_eq!(replacement.end, 3);
        assert_eq!(replacement.value, "reboot");
        assert!(replacement.append_space);
        assert_eq!(options.as_slice(), ["reboot"]);
    }

    #[test]
    fn does_not_append_space_for_status_command() {
        let engine = CompletionEngine::new();
        let (replacement, options) = expect_options(engine.complete("statu", 5));
        let replacement = replacement.expect("expected replacement");
        assert_eq!(replacement.start, 0);
        assert_eq!(replacement.end, 5);
        assert_eq!(replacement.value, "status");
        assert!(!replacement.append_space);
        assert_eq!(options.as_slice(), ["status"]);
    }

    #[test]
    fn appends_space_for_help_topics() {
        let engine = CompletionEngine::new();
        let (replacement, options) = expect_options(engine.complete("hel", 3));
        let replacement = replacement.expect("expected replacement");
        assert_eq!(replacement.start, 0);
        assert_eq!(replacement.end, 3);
        assert_eq!(replacement.value, "help");
        assert!(replacement.append_space);
        assert_eq!(options.as_slice(), ["help"]);
    }

    #[test]
    fn suggests_reboot_arguments() {
        let engine = CompletionEngine::new();
        let (replacement, options) = expect_options(engine.complete("reboot ", 7));
        assert!(replacement.is_none());
        assert_eq!(options.as_slice(), REBOOT_ARGS);
    }

    #[test]
    fn narrows_reboot_argument_by_prefix() {
        let engine = CompletionEngine::new();
        let (replacement, options) = expect_options(engine.complete("reboot n", 8));
        let replacement = replacement.expect("expected replacement");
        assert_eq!(replacement.start, 7);
        assert_eq!(replacement.end, 8);
        assert_eq!(replacement.value, "now");
        assert_eq!(options.as_slice(), ["now"]);
    }

    #[test]
    fn suggests_fault_retry_values() {
        let engine = CompletionEngine::new();
        let (replacement, options) = expect_options(engine.complete("fault recover ", 14));
        let replacement = replacement.expect("expected retries= prefix");
        assert_eq!(replacement.start, 14);
        assert_eq!(replacement.end, 14);
        assert_eq!(replacement.value, "retries=");
        assert_eq!(options.as_slice(), FAULT_RETRY_VALUES);
    }

    #[test]
    fn applies_case_insensitive_matching() {
        let engine = CompletionEngine::new();
        let (replacement, options) = expect_options(engine.complete("ReBoOt D", 8));
        let replacement = replacement.expect("expected replacement");
        assert_eq!(replacement.start, 7);
        assert_eq!(replacement.end, 8);
        assert_eq!(replacement.value, "delay");
        assert_eq!(options.as_slice(), ["delay"]);
    }

    #[test]
    fn provides_help_topics() {
        let engine = CompletionEngine::new();
        let (replacement, options) = expect_options(engine.complete("help r", 6));
        let replacement = replacement.expect("expected replacement");
        assert_eq!(replacement.start, 5);
        assert_eq!(replacement.end, 6);
        assert_eq!(replacement.value, "re");
        assert_eq!(options.as_slice(), ["reboot", "recovery"]);
    }
}
