mod session;

use std::convert::TryFrom;
use std::env;
use std::io::{self, Write};
use std::process;

use controller_core::repl::completion::Replacement;
use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{self, ClearType};
use crossterm::{queue, terminal::disable_raw_mode};

use session::{CompletionResponse, Session, TranscriptProfile};

fn main() -> io::Result<()> {
    let profile = parse_profile().unwrap_or_else(|err| {
        eprintln!("{err}");
        eprintln!(
            "Usage: emulator [--profile <reboot|recovery|fault>] | emulator <reboot|recovery|fault>"
        );
        process::exit(2);
    });

    let mut session = Session::new(profile)?;
    let _raw_mode = RawModeGuard::activate()?;

    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    display_banner(&mut stdout)?;

    let mut buffer = String::new();
    let mut cursor_index = 0usize;

    render_prompt(&mut stdout, &buffer, cursor_index)?;

    loop {
        let event = event::read()?;
        match event {
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                if handle_key_event(
                    key,
                    &mut session,
                    &mut stdout,
                    &mut buffer,
                    &mut cursor_index,
                )? {
                    break;
                }
            }
            Event::Resize(_, _) => {
                render_prompt(&mut stdout, &buffer, cursor_index)?;
            }
            _ => {}
        }
    }

    Ok(())
}

fn handle_key_event(
    key: KeyEvent,
    session: &mut Session,
    stdout: &mut io::StdoutLock<'_>,
    buffer: &mut String,
    cursor_index: &mut usize,
) -> io::Result<bool> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('c') => {
                write!(stdout, "\r\nSession interrupted.\r\n")?;
                stdout.flush()?;
                return Ok(true);
            }
            KeyCode::Char('d') => {
                if buffer.is_empty() {
                    write!(stdout, "\r\nSession closed.\r\n")?;
                    stdout.flush()?;
                    return Ok(true);
                }
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Enter => handle_enter(session, stdout, buffer, cursor_index),
        KeyCode::Tab | KeyCode::BackTab => {
            handle_completion(session, stdout, buffer, cursor_index)?;
            Ok(false)
        }
        KeyCode::Backspace => {
            if *cursor_index > 0 {
                let mut chars = buffer[..*cursor_index].char_indices();
                let (idx, _) = chars.next_back().unwrap();
                buffer.replace_range(idx..*cursor_index, "");
                *cursor_index = idx;
                render_prompt(stdout, buffer, *cursor_index)?;
            } else {
                beep(stdout)?;
            }
            Ok(false)
        }
        KeyCode::Char(ch) => {
            if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            {
                return Ok(false);
            }
            if ch.is_ascii() && ch != '\n' && ch != '\r' && ch != '\t' {
                buffer.insert(*cursor_index, ch);
                *cursor_index += ch.len_utf8();
                render_prompt(stdout, buffer, *cursor_index)?;
            } else {
                beep(stdout)?;
            }
            Ok(false)
        }
        KeyCode::Esc => {
            write!(stdout, "\r\nSession closed.\r\n")?;
            stdout.flush()?;
            Ok(true)
        }
        KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Home
        | KeyCode::End => {
            beep(stdout)?;
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn handle_enter(
    session: &mut Session,
    stdout: &mut io::StdoutLock<'_>,
    buffer: &mut String,
    cursor_index: &mut usize,
) -> io::Result<bool> {
    write!(stdout, "\r\n")?;
    stdout.flush()?;

    let trimmed = buffer.trim();
    if trimmed.is_empty() {
        buffer.clear();
        *cursor_index = 0;
        render_prompt(stdout, buffer, *cursor_index)?;
        return Ok(false);
    }

    if should_terminate(trimmed) {
        write!(stdout, "Session closed.\r\n")?;
        stdout.flush()?;
        return Ok(true);
    }

    let responses = session.handle_command(trimmed)?;
    for response in responses {
        write!(stdout, "{response}\r\n")?;
    }
    stdout.flush()?;

    buffer.clear();
    *cursor_index = 0;
    render_prompt(stdout, buffer, *cursor_index)?;
    Ok(false)
}

fn handle_completion(
    session: &mut Session,
    stdout: &mut io::StdoutLock<'_>,
    buffer: &mut String,
    cursor_index: &mut usize,
) -> io::Result<()> {
    let response = session.handle_completion(buffer, *cursor_index)?;
    match response {
        CompletionResponse::NoMatches => {
            beep(stdout)?;
        }
        CompletionResponse::Applied { replacement } => {
            apply_replacement(buffer, cursor_index, &replacement);
            render_prompt(stdout, buffer, *cursor_index)?;
        }
        CompletionResponse::Suggestions { options } => {
            write!(stdout, "\r\n")?;
            for option in options {
                write!(stdout, "  {option}\r\n")?;
            }
            stdout.flush()?;
            render_prompt(stdout, buffer, *cursor_index)?;
        }
    }
    Ok(())
}

fn apply_replacement(buffer: &mut String, cursor_index: &mut usize, replacement: &Replacement) {
    let clamped_start = replacement.start.min(buffer.len());
    let clamped_end = replacement.end.min(buffer.len());
    buffer.replace_range(clamped_start..clamped_end, replacement.value);
    let mut new_cursor = clamped_start + replacement.value.len();
    if replacement.append_space {
        buffer.insert(new_cursor, ' ');
        new_cursor += 1;
    }
    *cursor_index = new_cursor;
}

fn render_prompt(
    stdout: &mut io::StdoutLock<'_>,
    buffer: &str,
    cursor_index: usize,
) -> io::Result<()> {
    queue!(
        stdout,
        cursor::MoveToColumn(0),
        terminal::Clear(ClearType::CurrentLine)
    )?;
    write!(stdout, "> {buffer}")?;
    let base = u16::try_from(buffer[..cursor_index].chars().count()).unwrap_or(u16::MAX);
    let cursor_column = base.saturating_add(2);
    queue!(stdout, cursor::MoveToColumn(cursor_column))?;
    stdout.flush()
}

fn display_banner(stdout: &mut io::StdoutLock<'_>) -> io::Result<()> {
    write!(
        stdout,
        "Orin Controller Emulator ready. Type `help` for commands or `exit` to quit.\r\n"
    )?;
    stdout.flush()
}

fn beep(stdout: &mut io::StdoutLock<'_>) -> io::Result<()> {
    stdout.write_all(b"\x07")?;
    stdout.flush()
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

struct RawModeGuard;

impl RawModeGuard {
    fn activate() -> io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}
