//! Grammar-aware completion engine shared by firmware and emulator REPLs.
//!
//! The line editor can invoke this module to look up suggestions based on the
//! current buffer contents and cursor position without pulling in `std`.

use super::catalog::{
    self, ChoiceBranch, DefaultChoice, HelpTopics, Node, SubcommandBranch, ValueSpec,
};
use super::grammar::{self, Token, TokenKind};
use heapless::Vec as HeaplessVec;

const MAX_SUGGESTIONS: usize = 16;

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

        let expectation = determine_expectation(leading_tokens.as_slice());
        let candidates = collect_candidates(expectation);
        if candidates.is_empty() {
            return CompletionResult {
                replacement: None,
                options: HeaplessVec::new(),
            };
        }

        let mut matches: HeaplessVec<&'static str, MAX_SUGGESTIONS> = HeaplessVec::new();
        for candidate in candidates.iter() {
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
            append_space = should_append_space(expectation, candidate);
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
enum CompletionExpectation {
    Root,
    Choices(&'static [ChoiceBranch]),
    Subcommands(&'static [SubcommandBranch]),
    Topic(HelpTopics),
    Value(ValueSpec),
    None,
}

fn collect_candidates(
    expectation: CompletionExpectation,
) -> HeaplessVec<&'static str, MAX_SUGGESTIONS> {
    let mut options = HeaplessVec::new();

    match expectation {
        CompletionExpectation::Root => {
            for command in catalog::commands() {
                let _ = options.push(command.name);
            }
        }
        CompletionExpectation::Choices(choices) => {
            for choice in choices {
                match choice.value {
                    ValueSpec::IntegerAssignment { suggestions } => {
                        for suggestion in suggestions {
                            let _ = options.push(*suggestion);
                        }
                    }
                    ValueSpec::None | ValueSpec::Duration => {
                        let _ = options.push(choice.keyword);
                    }
                }
            }
        }
        CompletionExpectation::Subcommands(subcommands) => {
            for subcommand in subcommands {
                let _ = options.push(subcommand.name);
            }
        }
        CompletionExpectation::Topic(HelpTopics::Commands) => {
            for command in catalog::commands() {
                let _ = options.push(command.name);
            }
        }
        CompletionExpectation::Topic(HelpTopics::None)
        | CompletionExpectation::Value(ValueSpec::None)
        | CompletionExpectation::Value(ValueSpec::Duration)
        | CompletionExpectation::None => {}
        CompletionExpectation::Value(ValueSpec::IntegerAssignment { suggestions }) => {
            for suggestion in suggestions {
                let _ = options.push(*suggestion);
            }
        }
    }

    options
}

fn determine_expectation(tokens: &[Token<'_>]) -> CompletionExpectation {
    let tokens = trim_trailing_eol(tokens);
    if tokens.is_empty() {
        return CompletionExpectation::Root;
    }

    if tokens.iter().any(|token| token.kind == TokenKind::Error) {
        return CompletionExpectation::None;
    }

    let (first, rest) = match tokens.split_first() {
        Some(pair) => pair,
        None => return CompletionExpectation::Root,
    };

    if first.kind != TokenKind::Ident {
        return CompletionExpectation::Root;
    }

    match catalog::find(first.lexeme) {
        Some(spec) => evaluate_node(spec.grammar, rest),
        None => CompletionExpectation::Root,
    }
}

fn trim_trailing_eol<'slice, 'src>(mut tokens: &'slice [Token<'src>]) -> &'slice [Token<'src>] {
    while let Some(token) = tokens.last() {
        if token.kind == TokenKind::Eol {
            tokens = &tokens[..tokens.len() - 1];
        } else {
            break;
        }
    }
    tokens
}

fn evaluate_node(node: &'static Node, tokens: &[Token<'_>]) -> CompletionExpectation {
    match node {
        Node::End => CompletionExpectation::None,
        Node::OptionalChoice { choices, default } => {
            evaluate_optional_choice(choices, *default, tokens)
        }
        Node::Subcommands(branches) => evaluate_subcommands(branches, tokens),
        Node::Topic { topics, next } => evaluate_topic(*topics, next, tokens),
    }
}

fn evaluate_optional_choice(
    choices: &'static [ChoiceBranch],
    default: Option<DefaultChoice>,
    tokens: &[Token<'_>],
) -> CompletionExpectation {
    match tokens.split_first() {
        Some((token, rest)) if token.kind == TokenKind::Ident => {
            if let Some(branch) = choices
                .iter()
                .find(|choice| choice.keyword.eq_ignore_ascii_case(token.lexeme))
            {
                evaluate_choice_branch(branch, rest)
            } else {
                CompletionExpectation::Choices(choices)
            }
        }
        Some((token, _)) if token.kind == TokenKind::Eol => {
            default_expectation_or_choices(choices, default)
        }
        Some(_) => CompletionExpectation::Choices(choices),
        None => default_expectation_or_choices(choices, default),
    }
}

fn default_expectation_or_choices(
    choices: &'static [ChoiceBranch],
    default: Option<DefaultChoice>,
) -> CompletionExpectation {
    if let Some(default_choice) = default {
        match evaluate_node(default_choice.next, &[]) {
            CompletionExpectation::None => CompletionExpectation::Choices(choices),
            other => other,
        }
    } else {
        CompletionExpectation::Choices(choices)
    }
}

fn evaluate_choice_branch(
    branch: &'static ChoiceBranch,
    tokens: &[Token<'_>],
) -> CompletionExpectation {
    match evaluate_value(branch.value, tokens) {
        ValueProgress::Advance(remaining) => evaluate_node(branch.next, remaining),
        ValueProgress::Need(spec) => CompletionExpectation::Value(spec),
    }
}

fn evaluate_subcommands(
    branches: &'static [SubcommandBranch],
    tokens: &[Token<'_>],
) -> CompletionExpectation {
    match tokens.split_first() {
        Some((token, rest)) if token.kind == TokenKind::Ident => {
            if let Some(branch) = branches
                .iter()
                .find(|candidate| candidate.name.eq_ignore_ascii_case(token.lexeme))
            {
                evaluate_node(branch.grammar, rest)
            } else {
                CompletionExpectation::Subcommands(branches)
            }
        }
        Some((token, _)) if token.kind == TokenKind::Eol => {
            CompletionExpectation::Subcommands(branches)
        }
        Some(_) => CompletionExpectation::Subcommands(branches),
        None => CompletionExpectation::Subcommands(branches),
    }
}

fn evaluate_topic(
    topics: HelpTopics,
    next: &'static Node,
    tokens: &[Token<'_>],
) -> CompletionExpectation {
    match topics {
        HelpTopics::None => evaluate_node(next, tokens),
        HelpTopics::Commands => match tokens.split_first() {
            Some((token, rest)) if token.kind == TokenKind::Ident => evaluate_node(next, rest),
            Some((token, _)) if token.kind == TokenKind::Eol => {
                CompletionExpectation::Topic(topics)
            }
            Some(_) => CompletionExpectation::Topic(topics),
            None => CompletionExpectation::Topic(topics),
        },
    }
}

fn evaluate_value<'src, 'slice>(
    spec: ValueSpec,
    tokens: &'slice [Token<'src>],
) -> ValueProgress<'slice, 'src> {
    match spec {
        ValueSpec::None => ValueProgress::Advance(tokens),
        ValueSpec::Duration => match tokens.split_first() {
            Some((token, rest)) if token.kind == TokenKind::Duration => {
                ValueProgress::Advance(rest)
            }
            Some((token, _)) if token.kind == TokenKind::Eol => ValueProgress::Need(spec),
            Some(_) => ValueProgress::Need(spec),
            None => ValueProgress::Need(spec),
        },
        ValueSpec::IntegerAssignment { .. } => match tokens.split_first() {
            Some((token, rest)) if token.kind == TokenKind::Equals => match rest.split_first() {
                Some((value_token, remaining)) if value_token.kind == TokenKind::Integer => {
                    ValueProgress::Advance(remaining)
                }
                Some((value_token, _)) if value_token.kind == TokenKind::Eol => {
                    ValueProgress::Need(spec)
                }
                Some(_) => ValueProgress::Need(spec),
                None => ValueProgress::Need(spec),
            },
            Some((token, _)) if token.kind == TokenKind::Eol => ValueProgress::Need(spec),
            Some(_) => ValueProgress::Need(spec),
            None => ValueProgress::Need(spec),
        },
    }
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

fn should_append_space(expectation: CompletionExpectation, candidate: &'static str) -> bool {
    if !matches!(expectation, CompletionExpectation::Root) {
        return false;
    }

    let tokens = match grammar::lex(candidate) {
        Ok(tokens) => tokens,
        Err(_) => return false,
    };

    !matches!(
        determine_expectation(tokens.as_slice()),
        CompletionExpectation::Root | CompletionExpectation::None
    )
}

enum ValueProgress<'slice, 'src> {
    Advance(&'slice [Token<'src>]),
    Need(ValueSpec),
}

#[cfg(test)]
mod tests {
    use super::catalog::{self, CommandTag, HelpTopics, Node, SubcommandTag};
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

    fn reboot_arg_expectation() -> CompletionExpectation {
        match catalog::command(CommandTag::Reboot).grammar {
            Node::OptionalChoice { choices, .. } => CompletionExpectation::Choices(choices),
            other => panic!("unexpected reboot grammar: {other:?}"),
        }
    }

    fn fault_retry_expectation() -> CompletionExpectation {
        let subcommands = match catalog::command(CommandTag::Fault).grammar {
            Node::Subcommands(subcommands) => subcommands,
            other => panic!("unexpected fault grammar: {other:?}"),
        };
        let recover = subcommands
            .iter()
            .find(|sub| sub.tag == SubcommandTag::FaultRecover)
            .expect("missing fault recover subcommand");
        match recover.grammar {
            Node::OptionalChoice { choices, .. } => CompletionExpectation::Choices(choices),
            other => panic!("unexpected fault recover grammar: {other:?}"),
        }
    }

    fn help_topics_expectation() -> CompletionExpectation {
        CompletionExpectation::Topic(HelpTopics::Commands)
    }

    fn filter_candidates(
        prefix: &str,
        candidates: &[&'static str],
    ) -> HeaplessVec<&'static str, MAX_SUGGESTIONS> {
        let mut filtered: HeaplessVec<&'static str, MAX_SUGGESTIONS> = HeaplessVec::new();
        for candidate in candidates {
            if starts_with_ignore_ascii_case(candidate, prefix) {
                let _ = filtered.push(*candidate);
            }
        }
        filtered
    }

    #[test]
    fn offers_root_commands_from_empty_buffer() {
        let engine = CompletionEngine::new();
        let (replacement, options) = expect_options(engine.complete("", 0));
        assert!(replacement.is_none());
        let expected = collect_candidates(CompletionExpectation::Root);
        assert_eq!(options.as_slice(), expected.as_slice());
    }

    #[test]
    fn filters_root_commands_by_prefix() {
        let engine = CompletionEngine::new();
        let (replacement, options) = expect_options(engine.complete("re", 2));
        assert!(replacement.is_none());
        let expected_root = collect_candidates(CompletionExpectation::Root);
        let expected = filter_candidates("re", expected_root.as_slice());
        assert_eq!(options.as_slice(), expected.as_slice());
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
        let expected = collect_candidates(reboot_arg_expectation());
        assert_eq!(options.as_slice(), expected.as_slice());
    }

    #[test]
    fn narrows_reboot_argument_by_prefix() {
        let engine = CompletionEngine::new();
        let (replacement, options) = expect_options(engine.complete("reboot n", 8));
        let replacement = replacement.expect("expected replacement");
        assert_eq!(replacement.start, 7);
        assert_eq!(replacement.end, 8);
        assert_eq!(replacement.value, "now");
        let candidates = collect_candidates(reboot_arg_expectation());
        let expected = filter_candidates("n", candidates.as_slice());
        assert_eq!(options.as_slice(), expected.as_slice());
    }

    #[test]
    fn suggests_fault_retry_values() {
        let engine = CompletionEngine::new();
        let (replacement, options) = expect_options(engine.complete("fault recover ", 14));
        let replacement = replacement.expect("expected retries= prefix");
        assert_eq!(replacement.start, 14);
        assert_eq!(replacement.end, 14);
        assert_eq!(replacement.value, "retries=");
        let expected = collect_candidates(fault_retry_expectation());
        assert_eq!(options.as_slice(), expected.as_slice());
    }

    #[test]
    fn applies_case_insensitive_matching() {
        let engine = CompletionEngine::new();
        let (replacement, options) = expect_options(engine.complete("ReBoOt D", 8));
        let replacement = replacement.expect("expected replacement");
        assert_eq!(replacement.start, 7);
        assert_eq!(replacement.end, 8);
        assert_eq!(replacement.value, "delay");
        let candidates = collect_candidates(reboot_arg_expectation());
        let expected = filter_candidates("d", candidates.as_slice());
        assert_eq!(options.as_slice(), expected.as_slice());
    }

    #[test]
    fn provides_help_topics() {
        let engine = CompletionEngine::new();
        let (replacement, options) = expect_options(engine.complete("help r", 6));
        let replacement = replacement.expect("expected replacement");
        assert_eq!(replacement.start, 5);
        assert_eq!(replacement.end, 6);
        assert_eq!(replacement.value, "re");
        let candidates = collect_candidates(help_topics_expectation());
        let expected = filter_candidates("r", candidates.as_slice());
        assert_eq!(options.as_slice(), expected.as_slice());
    }
}
