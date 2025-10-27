mod session;

use std::env;
use std::io::{self, BufRead, Write};
use std::process;

use session::{Session, TranscriptProfile};

fn main() -> io::Result<()> {
    let profile = parse_profile().unwrap_or_else(|err| {
        eprintln!("{err}");
        eprintln!("Usage: emulator [--profile <reboot|recovery>] | emulator <reboot|recovery>");
        process::exit(2);
    });

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    let mut session = Session::new(profile)?;
    let mut line = String::new();

    writeln!(
        writer,
        "Orin Controller Emulator ready. Type `help` for commands or `exit` to quit."
    )?;

    loop {
        line.clear();
        write!(writer, "> ")?;
        writer.flush()?;

        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            writeln!(writer)?;
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if should_terminate(trimmed) {
            writeln!(writer, "Session closed.")?;
            break;
        }

        let responses = session.handle_command(trimmed)?;
        for response in responses {
            writeln!(writer, "{response}")?;
        }
    }

    Ok(())
}

fn should_terminate(input: &str) -> bool {
    input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit")
}

fn parse_profile() -> Result<TranscriptProfile, String> {
    let mut args = env::args().skip(1);
    if let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--profile=") {
            TranscriptProfile::from_tag(value)
        } else if arg == "--profile" {
            if let Some(value) = args.next() {
                TranscriptProfile::from_tag(&value)
            } else {
                Err("Expected value after --profile".to_string())
            }
        } else {
            TranscriptProfile::from_tag(&arg)
        }
    } else {
        Ok(TranscriptProfile::Reboot)
    }
}
