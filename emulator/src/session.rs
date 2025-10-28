use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::time::{Duration, Instant as HostInstant};

use controller_core::orchestrator::{
    CommandEnqueueError, CommandQueueProducer, CommandSource, ScheduleError, SequenceCommand,
    SequenceScheduler, register_default_templates,
};
use controller_core::repl::commands::{
    CommandError, CommandExecutor, CommandOutcome, FaultAck, RebootAck, RecoveryAck,
};
use controller_core::repl::completion::{CompletionEngine, Replacement};
use controller_core::repl::grammar::RecoveryCommand;
use controller_core::sequences::fault::FAULT_RECOVERY_MAX_RETRIES;
use controller_core::sequences::{
    SequenceTemplate, StepCompletion, StrapAction, StrapSequenceKind, StrapStep,
};

const DEFAULT_QUEUE_DEPTH: usize = 4;

pub const HELP_TOPICS: &[(&str, &str)] = &[
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TranscriptProfile {
    Reboot,
    Recovery,
    Fault,
}

impl TranscriptProfile {
    pub fn log_path(self) -> &'static str {
        match self {
            TranscriptProfile::Reboot => {
                "specs/001-build-orin-controller/evidence/emulator-reboot.log"
            }
            TranscriptProfile::Recovery => {
                "specs/001-build-orin-controller/evidence/emulator-recovery.log"
            }
            TranscriptProfile::Fault => {
                "specs/001-build-orin-controller/evidence/emulator-fault.log"
            }
        }
    }

    pub fn header(self) -> &'static str {
        match self {
            TranscriptProfile::Reboot => "Orin Controller Emulator reboot transcript",
            TranscriptProfile::Recovery => "Orin Controller Emulator recovery transcript",
            TranscriptProfile::Fault => "Orin Controller Emulator fault recovery transcript",
        }
    }

    pub fn from_tag(tag: &str) -> Result<Self, String> {
        if tag.eq_ignore_ascii_case("reboot") {
            Ok(Self::Reboot)
        } else if tag.eq_ignore_ascii_case("recovery") {
            Ok(Self::Recovery)
        } else if tag.eq_ignore_ascii_case("fault") {
            Ok(Self::Fault)
        } else {
            Err(format!("Unknown transcript profile `{tag}`"))
        }
    }
}

#[derive(Debug)]
pub enum CompletionResponse {
    NoMatches,
    Applied { replacement: Replacement },
    Suggestions { options: Vec<&'static str> },
}

pub struct Session {
    executor: CommandExecutor<SequenceScheduler<HostQueue>>,
    transcript: TranscriptLogger,
    started_at: HostInstant,
    command_count: usize,
    completion: CompletionEngine,
}

impl Session {
    pub fn new(profile: TranscriptProfile) -> io::Result<Self> {
        let transcript = TranscriptLogger::new(profile)?;
        let queue = HostQueue::new(DEFAULT_QUEUE_DEPTH);
        let mut scheduler = SequenceScheduler::new(queue);
        {
            let templates = scheduler.templates_mut();
            register_default_templates(templates).expect("register default sequence templates");
        }
        let executor = CommandExecutor::new(scheduler);

        Ok(Self {
            executor,
            transcript,
            started_at: HostInstant::now(),
            command_count: 0,
            completion: CompletionEngine::new(),
        })
    }

    pub fn handle_command(&mut self, line: &str) -> io::Result<Vec<String>> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let elapsed = self.started_at.elapsed();
        self.transcript
            .append_line(elapsed, TranscriptRole::Host, trimmed)?;

        if trimmed.eq_ignore_ascii_case("help") {
            return self.handle_help(None, elapsed);
        }
        if let Some(rest) = trimmed.strip_prefix("help ") {
            return self.handle_help(Some(rest.trim()), elapsed);
        }

        let now = HostInstant::now();
        match self.executor.execute(trimmed, now, CommandSource::UsbHost) {
            Ok(CommandOutcome::Reboot(ack)) => self.handle_reboot(ack, elapsed),
            Ok(CommandOutcome::Recovery(ack)) => self.handle_recovery(ack, elapsed),
            Ok(CommandOutcome::Fault(ack)) => self.handle_fault(ack, elapsed),
            Err(CommandError::Parse(err)) => {
                let message = format!("ERR syntax {err}");
                let lines = vec![message];
                self.record_output(elapsed, &lines)?;
                Ok(lines)
            }
            Err(CommandError::Unsupported(topic)) => {
                let message = format!("ERR unsupported {topic} (pending implementation)");
                let lines = vec![message];
                self.record_output(elapsed, &lines)?;
                Ok(lines)
            }
            Err(CommandError::Schedule(err)) => {
                let detail = describe_schedule_error(&err, self.started_at);
                let message = format!("ERR schedule {detail}");
                let lines = vec![message];
                self.record_output(elapsed, &lines)?;
                Ok(lines)
            }
        }
    }

    pub fn handle_completion(
        &mut self,
        buffer: &str,
        cursor: usize,
    ) -> io::Result<CompletionResponse> {
        let length = buffer.len();
        let cursor = cursor.min(length);
        let (prefix, suffix) = buffer.split_at(cursor);
        let elapsed = self.started_at.elapsed();
        self.transcript
            .log_completion_request(elapsed, prefix, suffix, cursor)?;

        let result = self.completion.complete(buffer, cursor);
        if result.options.is_empty() {
            self.transcript.log_completion_none(elapsed)?;
            return Ok(CompletionResponse::NoMatches);
        }

        let options: Vec<&'static str> = result.options.iter().copied().collect();
        if options.len() == 1 {
            let candidate = options[0];
            if let Some(replacement) = result.replacement {
                let replacement_log = replacement.clone();
                self.transcript.log_completion_applied(
                    elapsed,
                    candidate,
                    Some(replacement_log),
                )?;
                return Ok(CompletionResponse::Applied { replacement });
            } else {
                self.transcript
                    .log_completion_applied(elapsed, candidate, None)?;
                return Ok(CompletionResponse::NoMatches);
            }
        }

        self.transcript.log_completion_options(elapsed, &options)?;
        Ok(CompletionResponse::Suggestions { options })
    }

    fn handle_help(&mut self, topic: Option<&str>, elapsed: Duration) -> io::Result<Vec<String>> {
        let mut lines = Vec::new();
        match topic {
            Some(target) if !target.is_empty() => {
                if let Some((_, detail)) = HELP_TOPICS
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(target))
                {
                    lines.push((*detail).to_string());
                } else {
                    lines.push(format!("No help available for `{target}`."));
                    lines.push(format!("Available topics: {}", help_topic_list()));
                }
            }
            _ => {
                lines.push("Available commands:".to_string());
                for (_, detail) in HELP_TOPICS {
                    lines.push(format!("  {detail}"));
                }
                lines.push("Type `help <topic>` for a specific command.".to_string());
            }
        }

        self.record_output(elapsed, &lines)?;
        Ok(lines)
    }

    fn handle_reboot(
        &mut self,
        ack: RebootAck<HostInstant>,
        elapsed: Duration,
    ) -> io::Result<Vec<String>> {
        let start_delay = ack.start_after.unwrap_or(Duration::ZERO);
        let label = "reboot";
        self.handle_sequence(
            label,
            StrapSequenceKind::NormalReboot,
            ack.requested_at,
            start_delay,
            elapsed,
            |summary| SequenceNarration::new(default_ack(summary)),
        )
    }

    fn handle_recovery(
        &mut self,
        ack: RecoveryAck<HostInstant>,
        elapsed: Duration,
    ) -> io::Result<Vec<String>> {
        match ack.command {
            RecoveryCommand::Enter => {
                let label = "recovery enter";
                self.handle_sequence(
                    label,
                    ack.sequence,
                    ack.requested_at,
                    Duration::ZERO,
                    elapsed,
                    |summary| SequenceNarration::new(default_ack(summary)),
                )
            }
            RecoveryCommand::Exit => {
                let label = "recovery exit";
                self.handle_sequence(
                    label,
                    ack.sequence,
                    ack.requested_at,
                    Duration::ZERO,
                    elapsed,
                    |summary| SequenceNarration::new(default_ack(summary)),
                )
            }
            RecoveryCommand::Now => {
                let label = "recovery now";
                self.handle_sequence(
                    label,
                    ack.sequence,
                    ack.requested_at,
                    Duration::ZERO,
                    elapsed,
                    |summary| {
                        let head = format!(
                            "OK recovery waiting-for-console seq={} at=+{}ms cooldown={} ready=+{}ms queue-depth={}",
                            summary.sequence_id,
                            summary.request_offset.as_millis(),
                            format_duration_short(summary.cooldown),
                            summary.cooldown_ready_offset.as_millis(),
                            summary.queue_depth,
                        );
                        let notes = vec![
                            "monitoring for console activity on bridge (timeout 10s fallback)"
                                .to_string(),
                            "emulator parity: REC releases automatically once activity is detected"
                                .to_string(),
                        ];
                        SequenceNarration::with_notes(head, notes)
                    },
                )
            }
        }
    }

    fn handle_fault(
        &mut self,
        ack: FaultAck<HostInstant>,
        elapsed: Duration,
    ) -> io::Result<Vec<String>> {
        let default_budget = {
            let scheduler = self.executor.scheduler();
            scheduler
                .templates()
                .get(ack.sequence)
                .and_then(|template| template.max_retries)
                .unwrap_or(FAULT_RECOVERY_MAX_RETRIES)
        };
        let override_used = ack.retry_budget != default_budget;

        self.handle_sequence(
            "fault recover",
            ack.sequence,
            ack.requested_at,
            Duration::ZERO,
            elapsed,
            |summary| {
                let head = format!(
                    "OK fault recover seq={} at=+{}ms retries={}",
                    summary.sequence_id,
                    summary.request_offset.as_millis(),
                    ack.retry_budget
                );

                if override_used {
                    let note = format!("retry override applied (default {})", default_budget);
                    SequenceNarration::with_notes(head, vec![note])
                } else {
                    SequenceNarration::new(head)
                }
            },
        )
    }

    fn record_output(&mut self, elapsed: Duration, lines: &[String]) -> io::Result<()> {
        for line in lines {
            self.transcript
                .append_line(elapsed, TranscriptRole::Emulator, line)?;
        }
        Ok(())
    }

    fn handle_sequence<F>(
        &mut self,
        label: &'static str,
        sequence: StrapSequenceKind,
        requested_at: HostInstant,
        start_after: Duration,
        elapsed: Duration,
        formatter: F,
    ) -> io::Result<Vec<String>>
    where
        F: FnOnce(&SequenceSummary) -> SequenceNarration,
    {
        self.command_count += 1;
        let sequence_id = self.command_count;

        let (queue_depth, template): (usize, SequenceTemplate) = {
            let scheduler = self.executor.scheduler();
            let queue_depth = scheduler.producer().len().unwrap_or(0);
            let template = *scheduler
                .templates()
                .get(sequence)
                .expect("sequence template missing");
            (queue_depth, template)
        };

        let run_duration = sequence_run_duration(&template);
        let completion = requested_at + start_after + run_duration;
        let cooldown = template.cooldown_duration();
        let cooldown_ready = completion + cooldown;

        let summary = SequenceSummary {
            label,
            sequence,
            sequence_id,
            queue_depth,
            request_offset: requested_at.duration_since(self.started_at),
            start_after,
            run_duration,
            cooldown,
            cooldown_ready_offset: cooldown_ready.duration_since(self.started_at),
        };

        let narration = formatter(&summary);
        let mut lines = Vec::new();
        lines.push(narration.head);
        for note in narration.notes {
            lines.push(note);
        }

        lines.push(format!(
            "{:?} run-duration={} steps={}",
            summary.sequence,
            format_duration_short(summary.run_duration),
            template.step_count()
        ));

        for (index, step) in template.steps().iter().enumerate() {
            lines.push(describe_step(index + 1, step));
        }

        {
            let scheduler = self.executor.scheduler_mut();
            let _ = scheduler.notify_completed(sequence, completion);
            let _ = scheduler.producer_mut().pop_front();
        }

        self.record_output(elapsed, &lines)?;
        Ok(lines)
    }
}

struct SequenceSummary {
    label: &'static str,
    sequence: StrapSequenceKind,
    sequence_id: usize,
    queue_depth: usize,
    request_offset: Duration,
    start_after: Duration,
    run_duration: Duration,
    cooldown: Duration,
    cooldown_ready_offset: Duration,
}

struct SequenceNarration {
    head: String,
    notes: Vec<String>,
}

impl SequenceNarration {
    fn new(head: String) -> Self {
        Self {
            head,
            notes: Vec::new(),
        }
    }

    fn with_notes(head: String, notes: Vec<String>) -> Self {
        Self { head, notes }
    }
}

fn default_ack(summary: &SequenceSummary) -> String {
    format!(
        "OK {label} queued seq={} at=+{}ms start-after={} cooldown={} ready=+{}ms queue-depth={}",
        summary.sequence_id,
        summary.request_offset.as_millis(),
        format_duration_short(summary.start_after),
        format_duration_short(summary.cooldown),
        summary.cooldown_ready_offset.as_millis(),
        summary.queue_depth,
        label = summary.label,
    )
}

#[derive(Clone)]
struct HostQueue {
    capacity: usize,
    commands: VecDeque<SequenceCommand<HostInstant>>,
}

impl HostQueue {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            commands: VecDeque::with_capacity(capacity),
        }
    }

    fn pop_front(&mut self) -> Option<SequenceCommand<HostInstant>> {
        self.commands.pop_front()
    }
}

impl CommandQueueProducer for HostQueue {
    type Instant = HostInstant;
    type Error = ();

    fn try_enqueue(
        &mut self,
        command: SequenceCommand<Self::Instant>,
    ) -> Result<(), CommandEnqueueError<Self::Error>> {
        if self.commands.len() >= self.capacity {
            return Err(CommandEnqueueError::QueueFull);
        }

        self.commands.push_back(command);
        Ok(())
    }

    fn capacity(&self) -> Option<usize> {
        Some(self.capacity)
    }

    fn len(&self) -> Option<usize> {
        Some(self.commands.len())
    }
}

struct TranscriptLogger {
    writer: BufWriter<std::fs::File>,
}

impl TranscriptLogger {
    fn new(profile: TranscriptProfile) -> io::Result<Self> {
        let path = Path::new(profile.log_path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;

        let mut logger = Self {
            writer: BufWriter::new(file),
        };

        logger.write_header(profile)?;
        Ok(logger)
    }

    fn write_header(&mut self, profile: TranscriptProfile) -> io::Result<()> {
        writeln!(self.writer, "# {}", profile.header())?;
        writeln!(
            self.writer,
            "# Timestamps are milliseconds since session start"
        )?;
        writeln!(self.writer)?;
        self.writer.flush()
    }

    fn append_line(
        &mut self,
        elapsed: Duration,
        role: TranscriptRole,
        line: &str,
    ) -> io::Result<()> {
        writeln!(
            self.writer,
            "[+{:>6} ms] {} {}",
            elapsed.as_millis(),
            role.prefix(),
            line
        )?;
        self.writer.flush()
    }

    fn log_completion_request(
        &mut self,
        elapsed: Duration,
        prefix: &str,
        suffix: &str,
        cursor: usize,
    ) -> io::Result<()> {
        let message = format!(
            "[TAB] prefix={prefix:?} suffix={suffix:?} cursor={cursor}",
            prefix = prefix,
            suffix = suffix,
            cursor = cursor
        );
        self.append_line(elapsed, TranscriptRole::Host, &message)
    }

    fn log_completion_none(&mut self, elapsed: Duration) -> io::Result<()> {
        self.append_line(elapsed, TranscriptRole::Emulator, "completion: no matches")
    }

    fn log_completion_applied(
        &mut self,
        elapsed: Duration,
        candidate: &str,
        replacement: Option<Replacement>,
    ) -> io::Result<()> {
        let message = match replacement {
            Some(rep) => format!(
                "completion applied: {candidate} (range={}..{})",
                rep.start, rep.end
            ),
            None => format!("completion candidate: {candidate} (no replacement applied)"),
        };
        self.append_line(elapsed, TranscriptRole::Emulator, &message)
    }

    fn log_completion_options(
        &mut self,
        elapsed: Duration,
        options: &[&'static str],
    ) -> io::Result<()> {
        let summary = format!("completion options ({})", options.len());
        self.append_line(elapsed, TranscriptRole::Emulator, &summary)?;
        for option in options {
            let line = format!("  {option}");
            self.append_line(elapsed, TranscriptRole::Emulator, &line)?;
        }
        Ok(())
    }
}

enum TranscriptRole {
    Host,
    Emulator,
}

impl TranscriptRole {
    fn prefix(&self) -> &'static str {
        match self {
            TranscriptRole::Host => "HOST>",
            TranscriptRole::Emulator => "EMU <",
        }
    }
}

fn help_topic_list() -> String {
    let mut buffer = String::new();
    for (index, (name, _)) in HELP_TOPICS.iter().enumerate() {
        if index > 0 {
            buffer.push_str(", ");
        }
        buffer.push_str(name);
    }
    buffer
}

fn describe_schedule_error(
    error: &ScheduleError<(), HostInstant>,
    session_start: HostInstant,
) -> String {
    match error {
        ScheduleError::Queue(CommandEnqueueError::QueueFull) => "queue-full".to_string(),
        ScheduleError::Queue(CommandEnqueueError::Disconnected) => "queue-disconnected".to_string(),
        ScheduleError::Queue(CommandEnqueueError::Other(_)) => "queue-error".to_string(),
        ScheduleError::MissingTemplate(kind) => format!("missing-template {:?}", kind),
        ScheduleError::CooldownActive { ready_at, .. } => {
            let duration = ready_at.duration_since(session_start);
            format!("cooldown-active ready=+{}ms", duration.as_millis())
        }
    }
}

fn sequence_run_duration(template: &SequenceTemplate) -> Duration {
    template
        .steps()
        .iter()
        .fold(Duration::from_millis(0), |acc, step| {
            acc + step.hold_duration()
        })
}

fn describe_step(index: usize, step: &StrapStep) -> String {
    let strap = step.strap();
    let constraints = describe_constraints(step);
    let mode = match step.completion {
        StepCompletion::AfterDuration => "after-duration".to_string(),
        StepCompletion::OnBridgeActivity => "bridge-activity".to_string(),
        StepCompletion::OnEvent(event) => format!("event({event:?})"),
    };
    format!(
        "  {index}. {name} {action} hold={} {constraints} mode={mode}",
        format_duration_short(step.hold_duration()),
        name = strap.name,
        action = action_label(step.action),
    )
}

fn describe_constraints(step: &StrapStep) -> String {
    let min = step
        .constraints
        .min_hold
        .map(|value| format_duration_short(value.as_duration()));
    let max = step
        .constraints
        .max_hold
        .map(|value| format_duration_short(value.as_duration()));

    match (min, max) {
        (Some(min), Some(max)) => format!("limits={min}..{max}"),
        (Some(min), None) => format!("min={min}"),
        (None, Some(max)) => format!("max={max}"),
        (None, None) => "limits=unbounded".to_string(),
    }
}

fn action_label(action: StrapAction) -> &'static str {
    match action {
        StrapAction::AssertLow => "assert-low",
        StrapAction::ReleaseHigh => "release-high",
    }
}

fn format_duration_short(duration: Duration) -> String {
    if duration.as_secs() == 0 {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{:.3}s", duration.as_secs_f64())
    }
}
