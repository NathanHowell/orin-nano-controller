use std::io;

#[allow(dead_code)]
#[path = "../session.rs"]
mod session;

use session::{Session, TranscriptProfile};

fn main() -> io::Result<()> {
    record_profile(TranscriptProfile::Reboot)?;
    record_profile(TranscriptProfile::Recovery)?;
    record_profile(TranscriptProfile::Fault)?;
    Ok(())
}

fn record_profile(profile: TranscriptProfile) -> io::Result<()> {
    let mut session = Session::new(profile)?;
    match profile {
        TranscriptProfile::Reboot => record_reboot(&mut session),
        TranscriptProfile::Recovery => record_recovery(&mut session),
        TranscriptProfile::Fault => record_fault(&mut session),
    }
}

fn record_reboot(session: &mut Session) -> io::Result<()> {
    session.handle_completion("re", 2)?;
    session.handle_completion("rebo", 4)?;
    session.handle_completion("reboot ", "reboot ".len())?;
    session.handle_completion("reboot n", "reboot n".len())?;
    session.handle_completion("fault ", "fault ".len())?;
    session.handle_completion("fault recover ", "fault recover ".len())?;
    session.handle_completion("fault recover retries=", "fault recover retries=".len())?;
    session.handle_completion("fault recover retries=2", "fault recover retries=2".len())?;

    let _ = session.handle_command("reboot now")?;
    let _ = session.handle_command("fault recover retries=2")?;
    Ok(())
}

fn record_recovery(session: &mut Session) -> io::Result<()> {
    session.handle_completion("re", 2)?;
    session.handle_completion("recovery ", "recovery ".len())?;
    session.handle_completion("recovery e", "recovery e".len())?;
    session.handle_completion("recovery n", "recovery n".len())?;
    session.handle_completion("help ", "help ".len())?;

    let _ = session.handle_command("recovery enter")?;
    let _ = session.handle_command("recovery now")?;
    let _ = session.handle_command("help status")?;
    Ok(())
}

fn record_fault(session: &mut Session) -> io::Result<()> {
    session.handle_completion("fa", 2)?;
    session.handle_completion("fault ", "fault ".len())?;
    session.handle_completion("fault r", "fault r".len())?;
    session.handle_completion("fault recover ", "fault recover ".len())?;
    session.handle_completion("fault recover r", "fault recover r".len())?;
    session.handle_completion(
        "fault recover retries=",
        "fault recover retries=".len(),
    )?;
    session.handle_completion(
        "fault recover retries=3",
        "fault recover retries=3".len(),
    )?;

    let _ = session.handle_command("fault recover")?;
    std::thread::sleep(std::time::Duration::from_millis(2500));
    let _ = session.handle_command("fault recover retries=1")?;
    Ok(())
}
