//! Shared REPL grammar specification expressed as an applicative AST.
//!
//! The parser and completion engine interpret the same structure, ensuring
//! keywords, defaults, and value layouts stay in sync.

#![allow(dead_code)]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandTag {
    Reboot,
    Recovery,
    Fault,
    Status,
    Help,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubcommandTag {
    FaultRecover,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceTag {
    RebootNow,
    RebootDelay,
    RecoveryEnter,
    RecoveryExit,
    RecoveryNow,
    FaultRetries,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueSpec {
    None,
    Duration,
    IntegerAssignment {
        suggestions: &'static [&'static str],
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpTopics {
    None,
    Commands,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub tag: CommandTag,
    pub grammar: &'static Node,
    pub help: HelpTopics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Node {
    End,
    OptionalChoice {
        choices: &'static [ChoiceBranch],
        default: Option<DefaultChoice>,
    },
    Subcommands(&'static [SubcommandBranch]),
    Topic {
        topics: HelpTopics,
        next: &'static Node,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoiceBranch {
    pub keyword: &'static str,
    pub tag: ChoiceTag,
    pub value: ValueSpec,
    pub next: &'static Node,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefaultChoice {
    pub tag: ChoiceTag,
    pub next: &'static Node,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubcommandBranch {
    pub name: &'static str,
    pub tag: SubcommandTag,
    pub grammar: &'static Node,
}

const END: Node = Node::End;

const REBOOT_CHOICES: [ChoiceBranch; 2] = [
    ChoiceBranch {
        keyword: "now",
        tag: ChoiceTag::RebootNow,
        value: ValueSpec::None,
        next: &END,
    },
    ChoiceBranch {
        keyword: "delay",
        tag: ChoiceTag::RebootDelay,
        value: ValueSpec::Duration,
        next: &END,
    },
];

const REBOOT_GRAMMAR: Node = Node::OptionalChoice {
    choices: &REBOOT_CHOICES,
    default: Some(DefaultChoice {
        tag: ChoiceTag::RebootNow,
        next: &END,
    }),
};

const RECOVERY_CHOICES: [ChoiceBranch; 3] = [
    ChoiceBranch {
        keyword: "enter",
        tag: ChoiceTag::RecoveryEnter,
        value: ValueSpec::None,
        next: &END,
    },
    ChoiceBranch {
        keyword: "exit",
        tag: ChoiceTag::RecoveryExit,
        value: ValueSpec::None,
        next: &END,
    },
    ChoiceBranch {
        keyword: "now",
        tag: ChoiceTag::RecoveryNow,
        value: ValueSpec::None,
        next: &END,
    },
];

const RECOVERY_GRAMMAR: Node = Node::OptionalChoice {
    choices: &RECOVERY_CHOICES,
    default: Some(DefaultChoice {
        tag: ChoiceTag::RecoveryEnter,
        next: &END,
    }),
};

const FAULT_RETRY_SUGGESTIONS: [&str; 3] = ["retries=1", "retries=2", "retries=3"];

const FAULT_RECOVER_CHOICES: [ChoiceBranch; 1] = [ChoiceBranch {
    keyword: "retries",
    tag: ChoiceTag::FaultRetries,
    value: ValueSpec::IntegerAssignment {
        suggestions: &FAULT_RETRY_SUGGESTIONS,
    },
    next: &END,
}];

const FAULT_RECOVER_GRAMMAR: Node = Node::OptionalChoice {
    choices: &FAULT_RECOVER_CHOICES,
    default: None,
};

const FAULT_SUBCOMMANDS: [SubcommandBranch; 1] = [SubcommandBranch {
    name: "recover",
    tag: SubcommandTag::FaultRecover,
    grammar: &FAULT_RECOVER_GRAMMAR,
}];

const FAULT_GRAMMAR: Node = Node::Subcommands(&FAULT_SUBCOMMANDS);

const HELP_GRAMMAR: Node = Node::Topic {
    topics: HelpTopics::Commands,
    next: &END,
};

const COMMANDS: [CommandSpec; 5] = [
    CommandSpec {
        name: "reboot",
        tag: CommandTag::Reboot,
        grammar: &REBOOT_GRAMMAR,
        help: HelpTopics::None,
    },
    CommandSpec {
        name: "recovery",
        tag: CommandTag::Recovery,
        grammar: &RECOVERY_GRAMMAR,
        help: HelpTopics::None,
    },
    CommandSpec {
        name: "fault",
        tag: CommandTag::Fault,
        grammar: &FAULT_GRAMMAR,
        help: HelpTopics::None,
    },
    CommandSpec {
        name: "status",
        tag: CommandTag::Status,
        grammar: &END,
        help: HelpTopics::None,
    },
    CommandSpec {
        name: "help",
        tag: CommandTag::Help,
        grammar: &HELP_GRAMMAR,
        help: HelpTopics::Commands,
    },
];

/// Returns the full command catalog.
#[must_use]
pub const fn commands() -> &'static [CommandSpec] {
    &COMMANDS
}

/// Looks up a command by its tag.
#[must_use]
pub fn command(tag: CommandTag) -> &'static CommandSpec {
    match tag {
        CommandTag::Reboot => &COMMANDS[0],
        CommandTag::Recovery => &COMMANDS[1],
        CommandTag::Fault => &COMMANDS[2],
        CommandTag::Status => &COMMANDS[3],
        CommandTag::Help => &COMMANDS[4],
    }
}

/// Finds a command by name (case insensitive).
#[must_use]
pub fn find(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS
        .iter()
        .find(|command| command.name.eq_ignore_ascii_case(name))
}
