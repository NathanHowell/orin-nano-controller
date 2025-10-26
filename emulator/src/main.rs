mod session;

use std::io::{self, BufRead, Write};

use session::Session;

const REBOOT_LOG_PATH: &str = "specs/001-build-orin-controller/evidence/emulator-reboot.log";

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    let mut session = Session::new(REBOOT_LOG_PATH)?;
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
