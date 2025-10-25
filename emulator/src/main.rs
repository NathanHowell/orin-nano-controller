use std::io::{self, BufRead, Write};
use std::time::Duration;

use controller_core::repl::grammar::{self, Command, RebootCommand, RecoveryCommand};

const HELP_TOPICS: &[(&str, &str)] = &[
    (
        "reboot",
        "reboot [now|delay <duration>]  - queue the normal reboot sequence",
    ),
    (
        "recovery",
        "recovery [enter|exit|now]    - manage recovery strap flows",
    ),
    (
        "fault",
        "fault recover [retries=<1-3>]   - attempt the fault recovery sequence",
    ),
    (
        "status",
        "status                        - display orchestrator state",
    ),
    (
        "help",
        "help [topic]                    - show help for a command",
    ),
];

fn main() -> io::Result<()> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut line = String::new();

    writeln!(
        stdout,
        "Orin Controller Emulator ready. Type `help` for commands or `exit` to quit."
    )?;

    loop {
        line.clear();
        write!(stdout, "> ")?;
        stdout.flush()?;

        let bytes_read = stdin.read_line(&mut line)?;
        if bytes_read == 0 {
            writeln!(stdout)?;
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if should_terminate(trimmed) {
            writeln!(stdout, "Session closed.")?;
            break;
        }

        match grammar::parse(trimmed) {
            Ok(command) => {
                respond_to_command(&mut stdout, command)?;
            }
            Err(err) => {
                writeln!(stdout, "ERR {err}")?;
            }
        }
    }

    Ok(())
}

fn should_terminate(input: &str) -> bool {
    input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit")
}

fn respond_to_command<W: Write>(writer: &mut W, command: Command<'_>) -> io::Result<()> {
    match command {
        Command::Reboot(action) => respond_reboot(writer, action),
        Command::Recovery(action) => respond_recovery(writer, action),
        Command::Fault(cmd) => respond_fault(writer, cmd.retries),
        Command::Status => respond_status(writer),
        Command::Help(cmd) => respond_help(writer, cmd.topic),
    }
}

fn respond_reboot<W: Write>(writer: &mut W, command: RebootCommand) -> io::Result<()> {
    match command {
        RebootCommand::Now => writeln!(
            writer,
            "OK reboot now (emulator queue wiring arrives in later tasks)."
        ),
        RebootCommand::Delay(duration) => writeln!(
            writer,
            "OK reboot delay={} (stub queue).",
            format_duration(duration)
        ),
    }
}

fn respond_recovery<W: Write>(writer: &mut W, command: RecoveryCommand) -> io::Result<()> {
    let action = match command {
        RecoveryCommand::Enter => "enter",
        RecoveryCommand::Exit => "exit",
        RecoveryCommand::Now => "now",
    };
    writeln!(
        writer,
        "OK recovery {action} (strap orchestration will attach in follow-up tasks)."
    )
}

fn respond_fault<W: Write>(writer: &mut W, retries: Option<u8>) -> io::Result<()> {
    match retries {
        Some(value) => writeln!(
            writer,
            "OK fault recover retries={value} (emulator orchestrator not yet linked)."
        ),
        None => writeln!(
            writer,
            "OK fault recover (default retries) (emulator orchestrator not yet linked)."
        ),
    }
}

fn respond_status<W: Write>(writer: &mut W) -> io::Result<()> {
    writeln!(
        writer,
        "Status unavailable: host strap orchestrator scaffolding is pending."
    )
}

fn respond_help<W: Write>(writer: &mut W, topic: Option<&str>) -> io::Result<()> {
    if let Some(topic) = topic {
        if let Some((_, detail)) = HELP_TOPICS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(topic))
        {
            writeln!(writer, "{detail}")
        } else {
            writeln!(writer, "No help available for `{topic}`.")?;
            write!(writer, "Available topics: ")?;
            write_topic_list(writer)?;
            writeln!(writer)
        }
    } else {
        writeln!(writer, "Available commands:")?;
        for (_, detail) in HELP_TOPICS {
            writeln!(writer, "  {detail}")?;
        }
        writeln!(writer, "Type `help <topic>` for a specific command.")
    }
}

fn write_topic_list<W: Write>(writer: &mut W) -> io::Result<()> {
    let mut first = true;
    for (name, _) in HELP_TOPICS {
        if first {
            first = false;
        } else {
            write!(writer, ", ")?;
        }
        write!(writer, "{name}")?;
    }
    Ok(())
}

fn format_duration(duration: Duration) -> String {
    if duration.subsec_millis() == 0 && duration.subsec_nanos() == 0 && duration.as_secs() > 0 {
        format!("{}s", duration.as_secs())
    } else {
        format!("{}ms", duration.as_millis())
    }
}
