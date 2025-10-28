//! Strap orchestrator skeleton.
//!
//! This module wires together the strap command queue, sequence templates, and
//! the `SequenceRun` state machine. The skeleton provides just enough structure
//! for future tasks to add real hardware control, telemetry, and error
//! handling logic.

#![allow(dead_code)]

use core::convert::TryFrom;

use crate::bridge::BridgeDisconnectNotice;
use crate::telemetry::{TelemetryPayload, TelemetryRecorder};
use controller_core::orchestrator::{
    self as core_orchestrator, NoopStrapDriver, StrapDriver, retry_budget_for,
};
pub use controller_core::orchestrator::{
    ActiveRunError, CommandRejection, CommandRejectionReason, OrchestratorState, TemplateRegistry,
};
use controller_core::telemetry::TelemetryInstant;
use embassy_time::{Duration, Instant, Timer};
use heapless::Deque;

#[cfg(target_os = "none")]
use embassy_stm32::gpio::OutputOpenDrain;

use super::{
    COMMAND_QUEUE_DEPTH, CommandReceiver, EventId, FirmwareInstant, SequenceCommand, SequenceError,
    SequenceOutcome, SequenceRun, SequenceState, SequenceTemplate, StepCompletion, StrapAction,
    StrapId, StrapLine, StrapStep, TelemetryEventKind, strap_by_id,
};

pub type PowerSample = core_orchestrator::PowerSample<FirmwareInstant>;
pub type PowerStatus = core_orchestrator::PowerStatus<FirmwareInstant>;
pub type NoopPowerMonitor = core_orchestrator::NoopPowerMonitor<FirmwareInstant>;
#[cfg(target_os = "none")]
pub type FirmwarePowerMonitor<'d> =
    core_orchestrator::VrefintPowerMonitor<crate::hw::power::VrefintAdc<'d>>;

pub trait PowerMonitor: core_orchestrator::PowerMonitor<Instant = FirmwareInstant> {}

impl<T> PowerMonitor for T where T: core_orchestrator::PowerMonitor<Instant = FirmwareInstant> {}

fn strap_metadata(line: StrapId) -> StrapLine {
    strap_by_id(line)
}

fn strap_label(line: StrapId) -> &'static str {
    strap_by_id(line).name
}

fn strap_action_label(action: StrapAction) -> &'static str {
    match action {
        StrapAction::AssertLow => "assert-low",
        StrapAction::ReleaseHigh => "release-high",
    }
}

fn core_duration_to_embassy(duration: core::time::Duration) -> Duration {
    let micros = duration.as_micros();
    let micros = u64::try_from(micros).unwrap_or(u64::MAX);
    Duration::from_micros(micros)
}

fn opt_core_duration_to_embassy(duration: Option<core::time::Duration>) -> Option<Duration> {
    duration.map(core_duration_to_embassy)
}

#[cfg(target_os = "none")]
pub struct HardwareStrapDriver<'d> {
    reset: OutputOpenDrain<'d>,
    recovery: OutputOpenDrain<'d>,
    power: OutputOpenDrain<'d>,
    apo: OutputOpenDrain<'d>,
}

#[cfg(target_os = "none")]
impl<'d> HardwareStrapDriver<'d> {
    pub fn new(
        reset: OutputOpenDrain<'d>,
        recovery: OutputOpenDrain<'d>,
        power: OutputOpenDrain<'d>,
        apo: OutputOpenDrain<'d>,
    ) -> Self {
        Self {
            reset,
            recovery,
            power,
            apo,
        }
    }

    fn output_mut(&mut self, line: StrapId) -> &mut OutputOpenDrain<'d> {
        match line {
            StrapId::Reset => &mut self.reset,
            StrapId::Rec => &mut self.recovery,
            StrapId::Pwr => &mut self.power,
            StrapId::Apo => &mut self.apo,
        }
    }
}

#[cfg(target_os = "none")]
impl<'d> StrapDriver for HardwareStrapDriver<'d> {
    fn apply(&mut self, line: StrapId, action: StrapAction) {
        let output = self.output_mut(line);
        match action {
            StrapAction::AssertLow => output.set_low(),
            StrapAction::ReleaseHigh => output.set_high(),
        }
    }

    fn release_all(&mut self) {
        self.reset.set_high();
        self.recovery.set_high();
        self.power.set_high();
        self.apo.set_high();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::straps::FirmwareInstant;
    use crate::straps::{CommandQueue, StrapId, StrapSequenceKind};
    use controller_core::orchestrator::CommandSource;
    use controller_core::sequences::{fault_recovery_template, normal_reboot_template};
    use embassy_time::{Duration, Instant};

    #[test]
    fn normal_reboot_sequence_advances_through_template() {
        let queue = CommandQueue::new();
        let mut orchestrator = StrapOrchestrator::new(queue.receiver());

        orchestrator
            .templates_mut()
            .register(normal_reboot_template())
            .expect("template registry should accept normal reboot");

        let command = SequenceCommand::new(
            StrapSequenceKind::NormalReboot,
            FirmwareInstant::from(Instant::from_micros(0)),
            CommandSource::UsbHost,
        );
        orchestrator.begin_run(command).expect("run should start");

        let mut telemetry = TelemetryRecorder::new();
        let mut now = Instant::from_micros(0);

        orchestrator.drive_active_run(&mut telemetry, now);
        assert_eq!(orchestrator.state(), OrchestratorState::Running);
        {
            let run = orchestrator.active_run().expect("active run missing");
            assert_eq!(run.current_step_index(), Some(0));
            assert_eq!(
                run.step_deadline(),
                Some((now + Duration::from_millis(200)).into())
            );
        }
        assert_eq!(telemetry.len(), 1);
        assert_eq!(
            telemetry.latest().unwrap().event,
            TelemetryEventKind::StrapAsserted(StrapId::Pwr)
        );

        now += Duration::from_millis(200);
        orchestrator.drive_active_run(&mut telemetry, now);
        {
            let run = orchestrator.active_run().expect("active run missing");
            assert_eq!(run.current_step_index(), Some(1));
            assert_eq!(
                run.step_deadline(),
                Some((now + Duration::from_millis(1_000)).into())
            );
        }
        assert_eq!(telemetry.len(), 2);
        assert_eq!(
            telemetry.latest().unwrap().event,
            TelemetryEventKind::StrapReleased(StrapId::Pwr)
        );

        now += Duration::from_millis(1_000);
        orchestrator.drive_active_run(&mut telemetry, now);
        {
            let run = orchestrator.active_run().expect("active run missing");
            assert_eq!(run.current_step_index(), Some(2));
            assert_eq!(
                run.step_deadline(),
                Some((now + Duration::from_millis(20)).into())
            );
        }
        assert_eq!(
            telemetry.latest().unwrap().event,
            TelemetryEventKind::StrapAsserted(StrapId::Reset)
        );

        now += Duration::from_millis(20);
        orchestrator.drive_active_run(&mut telemetry, now);
        {
            let run = orchestrator.active_run().expect("active run missing");
            assert_eq!(run.state, SequenceState::Cooldown);
            assert!(run.current_step_index().is_none());
        }
        assert_eq!(
            telemetry.latest().unwrap().event,
            TelemetryEventKind::StrapReleased(StrapId::Reset)
        );
        assert_eq!(telemetry.len(), 4);

        let cooldown_deadline: Instant = orchestrator
            .active_run()
            .expect("active run missing")
            .cooldown_deadline()
            .expect("cooldown deadline unset")
            .into_embassy();

        now = cooldown_deadline;
        orchestrator.drive_active_run(&mut telemetry, now);
        {
            let run = orchestrator.active_run().expect("active run missing");
            assert!(matches!(
                run.state,
                SequenceState::Complete(SequenceOutcome::Completed)
            ));
        }

        assert_eq!(telemetry.len(), 5);
        let final_record = telemetry.latest().copied().unwrap();
        assert_eq!(
            final_record.event,
            TelemetryEventKind::SequenceComplete(StrapSequenceKind::NormalReboot)
        );
        match final_record.details {
            TelemetryPayload::Sequence(summary) => {
                assert_eq!(summary.outcome, SequenceOutcome::Completed);
                let duration = summary.duration.expect("missing duration");
                assert_eq!(duration.as_millis(), 2_220);
                assert_eq!(summary.events_recorded, 4);
            }
            _ => panic!("expected sequence payload"),
        }
    }

    #[test]
    fn retry_budget_prefers_command_override() {
        let queue = CommandQueue::new();
        let mut orchestrator = StrapOrchestrator::new(queue.receiver());

        orchestrator
            .templates_mut()
            .register(fault_recovery_template())
            .expect("register fault recovery template");

        let mut command = SequenceCommand::new(
            StrapSequenceKind::FaultRecovery,
            FirmwareInstant::from(Instant::from_micros(0)),
            CommandSource::UsbHost,
        );
        command.flags.retry_override = Some(2);

        let template = orchestrator
            .templates()
            .get(StrapSequenceKind::FaultRecovery)
            .expect("fault recovery template missing");

        assert_eq!(retry_budget_for(&command, template), 2);
    }
}

#[cfg(target_os = "none")]
fn log_control_link_attached(timestamp: FirmwareInstant) {
    let timestamp = timestamp.into_embassy();
    defmt::info!(
        "orchestrator: USB control link attached t={}us",
        timestamp.as_micros()
    );
}

#[cfg(not(target_os = "none"))]
fn log_control_link_attached(timestamp: FirmwareInstant) {
    let timestamp = timestamp.into_embassy();
    println!(
        "orchestrator: USB control link attached t={}us",
        timestamp.as_micros()
    );
}

#[cfg(target_os = "none")]
fn log_control_link_lost(had_active_run: bool, recovery_pending: bool, timestamp: FirmwareInstant) {
    let timestamp = timestamp.into_embassy();
    let tag = match (had_active_run, recovery_pending) {
        (true, true) => "awaiting recovery console activity",
        (true, false) => "aborting active strap run",
        (false, _) => "controller idle",
    };

    defmt::warn!(
        "orchestrator: USB control link lost ({}) t={}us",
        tag,
        timestamp.as_micros()
    );
}

#[cfg(not(target_os = "none"))]
fn log_control_link_lost(had_active_run: bool, recovery_pending: bool, timestamp: FirmwareInstant) {
    let timestamp = timestamp.into_embassy();
    let tag = match (had_active_run, recovery_pending) {
        (true, true) => "awaiting recovery console activity",
        (true, false) => "aborting active strap run",
        (false, _) => "controller idle",
    };

    println!(
        "orchestrator: USB control link lost ({}) t={}us",
        tag,
        timestamp.as_micros()
    );
}

#[cfg(target_os = "none")]
fn log_strap_drive(line: StrapId, action: StrapAction, timestamp: FirmwareInstant) {
    let timestamp = timestamp.into_embassy();
    let strap = strap_metadata(line);
    defmt::info!(
        "straps:{} {} pin={} driver={} J14-{=u8} t={}us",
        strap_label(line),
        strap_action_label(action),
        strap.mcu_pin,
        strap.driver_output,
        strap.j14_pin,
        timestamp.as_micros()
    );
}

#[cfg(not(target_os = "none"))]
fn log_strap_drive(line: StrapId, action: StrapAction, timestamp: FirmwareInstant) {
    let timestamp = timestamp.into_embassy();
    let strap = strap_metadata(line);
    println!(
        "straps:{} {} pin={} driver={} J14-{} t={}us",
        strap_label(line),
        strap_action_label(action),
        strap.mcu_pin,
        strap.driver_output,
        strap.j14_pin,
        timestamp.as_micros()
    );
}

#[derive(Clone, Debug)]
struct QueuedCommand {
    command: SequenceCommand,
    pending_event: Option<EventId>,
    not_before: Option<Instant>,
}

fn compute_not_before(command: &SequenceCommand) -> Option<Instant> {
    command.flags.start_after.map(|delay| {
        let requested = command.requested_at + delay;
        Instant::from(requested)
    })
}

fn remaining_delay(queued: &QueuedCommand) -> Option<Duration> {
    queued.not_before.and_then(|deadline| {
        let now = Instant::now();
        if now >= deadline {
            None
        } else {
            Some(deadline.saturating_duration_since(now))
        }
    })
}

const MIN_PROCESS_SLEEP_MS: u64 = 1;

#[cfg(target_os = "none")]
fn log_brown_out_detected(sample: &PowerSample, retries_used: u8, retry_budget: u8) {
    match sample.millivolts {
        Some(mv) => defmt::warn!(
            "VDD_3V3 brown-out detected (retry {} of {}, {=u16} mV)",
            retries_used + 1,
            retry_budget,
            mv
        ),
        None => defmt::warn!(
            "VDD_3V3 brown-out detected (retry {} of {}, reading unavailable)",
            retries_used + 1,
            retry_budget
        ),
    }
}

#[cfg(not(target_os = "none"))]
fn log_brown_out_detected(_: &PowerSample, _: u8, _: u8) {}

#[cfg(target_os = "none")]
fn log_retry_started(attempt: u8, budget: u8) {
    defmt::info!(
        "retrying strap sequence after brown-out (attempt {} of {})",
        attempt,
        budget
    );
}

#[cfg(not(target_os = "none"))]
fn log_retry_started(_: u8, _: u8) {}

#[cfg(target_os = "none")]
fn log_retry_exhausted(budget: u8) {
    defmt::error!("brown-out retry budget exhausted after {} attempts", budget);
}

#[cfg(not(target_os = "none"))]
fn log_retry_exhausted(_: u8) {}

#[cfg(target_os = "none")]
fn log_power_recovered(sample: &PowerSample, attempt: u8, holdoff: core::time::Duration) {
    let elapsed = holdoff.as_millis();
    match sample.millivolts {
        Some(mv) => defmt::info!(
            "power stable after brown-out (attempt {}, {=u16} mV, holdoff {} ms)",
            attempt,
            mv,
            elapsed
        ),
        None => defmt::info!(
            "power stable after brown-out (attempt {}, holdoff {} ms, reading unavailable)",
            attempt,
            elapsed
        ),
    }
}

#[cfg(not(target_os = "none"))]
fn log_power_recovered(_: &PowerSample, _: u8, _: core::time::Duration) {}

/// Coordinates strap sequencing based on queued commands.
pub struct StrapOrchestrator<
    'a,
    M: PowerMonitor = NoopPowerMonitor,
    D: StrapDriver = NoopStrapDriver,
> {
    command_rx: CommandReceiver<'a>,
    templates: TemplateRegistry,
    strap_driver: D,
    active_run: Option<SequenceRun>,
    last_rejection: Option<CommandRejection<FirmwareInstant>>,
    pending_commands: Deque<QueuedCommand, COMMAND_QUEUE_DEPTH>,
    power_monitor: M,
    last_power_sample: Option<PowerSample>,
    recovering_power: bool,
    control_link_attached: bool,
}

impl<'a> StrapOrchestrator<'a> {
    /// Creates a new orchestrator with an empty template registry.
    pub fn new(command_rx: CommandReceiver<'a>) -> Self {
        StrapOrchestrator::with_components(
            command_rx,
            NoopPowerMonitor::new(),
            NoopStrapDriver::new(),
        )
    }

    /// Creates a new orchestrator seeded with templates.
    pub fn with_templates(command_rx: CommandReceiver<'a>, templates: TemplateRegistry) -> Self {
        StrapOrchestrator::with_components_and_templates(
            command_rx,
            NoopPowerMonitor::new(),
            NoopStrapDriver::new(),
            templates,
        )
    }
}

impl<'a, M: PowerMonitor> StrapOrchestrator<'a, M> {
    /// Creates a new orchestrator with a supplied power monitor.
    pub fn with_power_monitor(command_rx: CommandReceiver<'a>, power_monitor: M) -> Self {
        StrapOrchestrator::with_components(command_rx, power_monitor, NoopStrapDriver::new())
    }

    /// Creates a new orchestrator with templates and a supplied power monitor.
    pub fn with_power_monitor_and_templates(
        command_rx: CommandReceiver<'a>,
        power_monitor: M,
        templates: TemplateRegistry,
    ) -> Self {
        StrapOrchestrator::with_components_and_templates(
            command_rx,
            power_monitor,
            NoopStrapDriver::new(),
            templates,
        )
    }
}

impl<'a, M: PowerMonitor, D: StrapDriver> StrapOrchestrator<'a, M, D> {
    /// Creates a new orchestrator with supplied hardware components.
    pub fn with_components(
        command_rx: CommandReceiver<'a>,
        power_monitor: M,
        mut strap_driver: D,
    ) -> Self {
        strap_driver.release_all();
        Self {
            command_rx,
            templates: TemplateRegistry::new(),
            strap_driver,
            active_run: None,
            last_rejection: None,
            pending_commands: Deque::new(),
            power_monitor,
            last_power_sample: None,
            recovering_power: false,
            control_link_attached: true,
        }
    }

    /// Creates a new orchestrator with templates and supplied hardware components.
    pub fn with_components_and_templates(
        command_rx: CommandReceiver<'a>,
        power_monitor: M,
        strap_driver: D,
        templates: TemplateRegistry,
    ) -> Self {
        let mut orchestrator = Self::with_components(command_rx, power_monitor, strap_driver);
        orchestrator.templates = templates;
        orchestrator
    }

    /// Returns the current orchestrator state.
    pub fn state(&self) -> OrchestratorState {
        match &self.active_run {
            None => OrchestratorState::Idle,
            Some(run) => match run.state {
                SequenceState::Idle => OrchestratorState::Idle,
                SequenceState::Arming => OrchestratorState::Arming,
                SequenceState::Executing => OrchestratorState::Running,
                SequenceState::Cooldown => OrchestratorState::Cooldown,
                SequenceState::Complete(_) => OrchestratorState::Completed,
                SequenceState::Error(_) => OrchestratorState::Error,
            },
        }
    }

    /// Returns a reference to the template registry.
    pub fn templates(&self) -> &TemplateRegistry {
        &self.templates
    }

    /// Returns a mutable reference to the template registry.
    pub fn templates_mut(&mut self) -> &mut TemplateRegistry {
        &mut self.templates
    }

    /// Returns the currently active sequence run, if present.
    pub fn active_run(&self) -> Option<&SequenceRun> {
        self.active_run.as_ref()
    }

    /// Returns a mutable handle to the active sequence run.
    pub fn active_run_mut(&mut self) -> Option<&mut SequenceRun> {
        self.active_run.as_mut()
    }

    /// Returns the sequence template for the active command, if available.
    pub fn active_template(&self) -> Option<&SequenceTemplate> {
        self.active_run
            .as_ref()
            .and_then(|run| self.templates.get(run.command.kind))
    }

    /// Returns the last command rejection, if any.
    pub fn last_rejection(&self) -> Option<&CommandRejection<FirmwareInstant>> {
        self.last_rejection.as_ref()
    }

    /// Clears and returns the last command rejection.
    pub fn take_last_rejection(&mut self) -> Option<CommandRejection<FirmwareInstant>> {
        self.last_rejection.take()
    }

    /// Returns `true` when the USB control link is currently attached.
    pub fn control_link_attached(&self) -> bool {
        self.control_link_attached
    }

    /// Marks the USB control link as attached, clearing any prior fault state.
    pub fn notify_control_link_attached(&mut self) {
        if self.control_link_attached {
            return;
        }

        self.control_link_attached = true;
        log_control_link_attached(FirmwareInstant::from(Instant::now()));
    }

    /// Handles a USB control link disconnect by aborting active work and logging telemetry.
    pub fn notify_control_link_lost(
        &mut self,
        telemetry: &mut TelemetryRecorder,
        notice: Option<BridgeDisconnectNotice>,
    ) {
        let (timestamp, recovery_pending) = notice
            .map(|notice| (notice.timestamp, notice.recovery_release_pending))
            .unwrap_or_else(|| (FirmwareInstant::from(Instant::now()), false));

        if !self.control_link_attached {
            return;
        }

        self.control_link_attached = false;

        let had_active_run = self.active_run.is_some();
        log_control_link_lost(had_active_run, recovery_pending, timestamp);

        let disconnect_event = telemetry.record(
            TelemetryEventKind::UsbDisconnect,
            TelemetryPayload::None,
            timestamp,
        );

        if let Some(run) = self.active_run.as_mut() {
            let _ = run.track_event(disconnect_event);
        }

        self.release_all_straps(telemetry, timestamp);

        if let Some(run) = self.active_run.as_mut() {
            run.state = SequenceState::Error(SequenceError::ControlLinkLost);
        }

        while let Some(queued) = self.pending_commands.pop_front() {
            self.last_rejection = Some(CommandRejection::control_link_lost(queued.command));
        }

        if had_active_run {
            self.finish_run();
        }
    }

    /// Begins executing a new sequence command.
    pub fn begin_run(
        &mut self,
        command: SequenceCommand,
    ) -> Result<(), CommandRejection<FirmwareInstant>> {
        if self.active_run.is_some() {
            return Err(CommandRejection::busy(command));
        }

        if !self.control_link_attached {
            return Err(CommandRejection::control_link_lost(command));
        }

        if !self.templates.contains(command.kind) {
            return Err(CommandRejection::missing_template(command));
        }

        self.active_run = Some(SequenceRun::new(command));
        self.recovering_power = false;
        self.last_power_sample = None;
        Ok(())
    }

    /// Finishes the active run and returns to idle.
    pub fn finish_run(&mut self) {
        self.active_run = None;
        self.recovering_power = false;
        self.last_power_sample = None;
    }

    /// Marks the active run as completed and records a telemetry event.
    pub fn complete_run(
        &mut self,
        telemetry: &mut TelemetryRecorder,
        outcome: SequenceOutcome,
        timestamp: FirmwareInstant,
    ) -> Result<(), ActiveRunError> {
        let (kind, started_at, requested_at, events_recorded) = match self.active_run.as_ref() {
            Some(run) => (
                run.command.kind,
                run.sequence_started_at,
                run.command.requested_at,
                run.emitted_events.len(),
            ),
            None => return Err(ActiveRunError::NoActiveRun),
        };

        let start = started_at.or(Some(requested_at));
        let event_id =
            telemetry.record_sequence_completion(kind, outcome, start, timestamp, events_recorded);

        if let Some(run) = self.active_run.as_mut() {
            let _ = run.track_event(event_id);
            run.state = SequenceState::Complete(outcome);
            Ok(())
        } else {
            Err(ActiveRunError::NoActiveRun)
        }
    }

    /// Marks the active run as failed.
    pub fn fail_run(&mut self, error: SequenceError) -> Result<(), ActiveRunError> {
        if let Some(run) = self.active_run.as_mut() {
            run.state = SequenceState::Error(error);
            Ok(())
        } else {
            Err(ActiveRunError::NoActiveRun)
        }
    }

    /// Updates the state of the active sequence run.
    pub fn transition_to(&mut self, next: SequenceState) -> Result<(), ActiveRunError> {
        if let Some(run) = self.active_run.as_mut() {
            run.state = next;
            Ok(())
        } else {
            Err(ActiveRunError::NoActiveRun)
        }
    }

    /// Handles the intake of commands and basic lifecycle management.
    pub async fn run(mut self, telemetry: &mut TelemetryRecorder) -> ! {
        loop {
            if self.active_run.is_some() {
                self.collect_pending_commands(telemetry);
                self.process_active_run(telemetry).await;
                continue;
            }

            if let Some(queued) = self.pending_commands.pop_front() {
                if let Some(delay) = remaining_delay(&queued) {
                    debug_assert!(
                        self.pending_commands.push_front(queued).is_ok(),
                        "pending command queue reinsertion failed"
                    );
                    Timer::after(delay).await;
                    continue;
                }

                match self.start_queued_command(queued, telemetry) {
                    Ok(()) => self.last_rejection = None,
                    Err(rejection) => self.last_rejection = Some(rejection),
                }
                continue;
            }

            let command = self.command_rx.receive().await;
            let not_before = compute_not_before(&command);
            let queued = QueuedCommand {
                command,
                pending_event: None,
                not_before,
            };

            if let Some(delay) = remaining_delay(&queued) {
                Timer::after(delay).await;
            }

            match self.start_queued_command(queued, telemetry) {
                Ok(()) => self.last_rejection = None,
                Err(rejection) => self.last_rejection = Some(rejection),
            }
        }
    }

    async fn process_active_run(&mut self, telemetry: &mut TelemetryRecorder) {
        if self.state().is_terminal() {
            self.finish_run();
            return;
        }

        match self.power_monitor.poll() {
            PowerStatus::BrownOut(sample) => {
                self.last_power_sample = Some(sample);
                if self.handle_brown_out(sample, telemetry).await {
                    return;
                }
            }
            PowerStatus::Stable(sample) => {
                self.last_power_sample = Some(sample);
            }
            PowerStatus::Unknown => {}
        }

        let now = Instant::now();
        self.drive_active_run(telemetry, now);

        // Yield to allow other tasks to interact with the active run while power remains stable.
        Timer::after(self.idle_delay()).await;
    }

    fn drive_active_run(&mut self, telemetry: &mut TelemetryRecorder, now: Instant) {
        loop {
            let kind = match self.active_run.as_ref() {
                Some(run) => run.command.kind,
                None => return,
            };

            let template = match self.templates.get(kind).cloned() {
                Some(template) => template,
                None => {
                    if self.fail_run(SequenceError::UnexpectedState).is_ok() {
                        continue;
                    } else {
                        return;
                    }
                }
            };

            let advanced = self.advance_run_state(&template, telemetry, now);

            if !advanced {
                break;
            }
        }
    }

    fn advance_run_state(
        &mut self,
        template: &SequenceTemplate,
        telemetry: &mut TelemetryRecorder,
        now: Instant,
    ) -> bool {
        let state = match self.active_run.as_ref() {
            Some(run) => run.state,
            None => return false,
        };

        match state {
            SequenceState::Idle => {
                if let Some(run) = self.active_run.as_mut() {
                    run.state = SequenceState::Arming;
                }
                true
            }
            SequenceState::Arming => {
                if template.phases.is_empty() {
                    if let Some(run) = self.active_run.as_mut() {
                        run.current_step_index = None;
                        run.state = SequenceState::Cooldown;
                    }
                    self.begin_cooldown(template.cooldown_duration(), telemetry, now)
                } else {
                    if let Some(run) = self.active_run.as_mut() {
                        run.current_step_index = Some(0);
                        run.state = SequenceState::Executing;
                    }
                    let step = &template.phases[0];
                    self.start_step(step, telemetry, now)
                }
            }
            SequenceState::Executing => {
                let current_index = match self
                    .active_run
                    .as_ref()
                    .and_then(|run| run.current_step_index)
                {
                    Some(index) => index,
                    None => {
                        if let Some(run) = self.active_run.as_mut() {
                            run.state = SequenceState::Cooldown;
                        }
                        return self.begin_cooldown(template.cooldown_duration(), telemetry, now);
                    }
                };

                let step = match template.phases.get(current_index) {
                    Some(step) => step,
                    None => {
                        if let Some(run) = self.active_run.as_mut() {
                            run.state = SequenceState::Cooldown;
                        }
                        return self.begin_cooldown(template.cooldown_duration(), telemetry, now);
                    }
                };

                match step.completion {
                    StepCompletion::AfterDuration => {
                        let ready = match self.active_run.as_ref().and_then(|run| run.step_deadline)
                        {
                            Some(deadline) => now >= Instant::from(deadline),
                            None => true,
                        };

                        if ready {
                            self.finish_step(template, telemetry, now)
                        } else {
                            false
                        }
                    }
                    StepCompletion::OnBridgeActivity => false,
                    StepCompletion::OnEvent(_) => false,
                }
            }
            SequenceState::Cooldown => {
                self.progress_cooldown(template.cooldown_duration(), telemetry, now)
            }
            SequenceState::Complete(_) | SequenceState::Error(_) => false,
        }
    }

    fn start_step(
        &mut self,
        step: &StrapStep,
        telemetry: &mut TelemetryRecorder,
        now: Instant,
    ) -> bool {
        if let Some(run) = self.active_run.as_mut() {
            run.waiting_on_bridge = matches!(step.completion, StepCompletion::OnBridgeActivity);
            run.step_started_at = Some(now.into());
            if run.sequence_started_at.is_none() {
                run.sequence_started_at = Some(now.into());
            }
            let hold = core_duration_to_embassy(step.hold_duration());
            run.step_deadline = match step.completion {
                StepCompletion::AfterDuration => Some((now + hold).into()),
                _ => None,
            };
        } else {
            return false;
        }

        self.drive_strap_transition(step.line, step.action, telemetry, now.into());
        true
    }

    fn finish_step(
        &mut self,
        template: &SequenceTemplate,
        telemetry: &mut TelemetryRecorder,
        now: Instant,
    ) -> bool {
        let (next_index, more_steps) = match self.active_run.as_ref() {
            Some(run) => {
                let current = run.current_step_index.unwrap_or(usize::MAX);
                let next = current.saturating_add(1);
                (next, next < template.phases.len())
            }
            None => return false,
        };

        if let Some(run) = self.active_run.as_mut() {
            run.step_started_at = None;
            run.step_deadline = None;
            if more_steps {
                run.current_step_index = Some(next_index);
            } else {
                run.current_step_index = None;
                run.state = SequenceState::Cooldown;
            }
        }

        if more_steps {
            let step = &template.phases[next_index];
            self.start_step(step, telemetry, now)
        } else {
            self.begin_cooldown(template.cooldown_duration(), telemetry, now)
        }
    }

    fn begin_cooldown(
        &mut self,
        cooldown: core::time::Duration,
        telemetry: &mut TelemetryRecorder,
        now: Instant,
    ) -> bool {
        let cooldown = core_duration_to_embassy(cooldown);
        if let Some(run) = self.active_run.as_mut() {
            if cooldown.as_ticks() == 0 {
                run.cooldown_deadline = None;
            } else {
                run.cooldown_deadline = Some((now + cooldown).into());
            }
        }

        if cooldown.as_ticks() == 0 {
            let _ = self.complete_run(telemetry, SequenceOutcome::SkippedCooldown, now.into());
            true
        } else {
            false
        }
    }

    fn progress_cooldown(
        &mut self,
        cooldown: core::time::Duration,
        telemetry: &mut TelemetryRecorder,
        now: Instant,
    ) -> bool {
        let cooldown = core_duration_to_embassy(cooldown);
        if cooldown.as_ticks() == 0 {
            let _ = self.complete_run(telemetry, SequenceOutcome::SkippedCooldown, now.into());
            return true;
        }

        let deadline = self
            .active_run
            .as_ref()
            .and_then(|run| run.cooldown_deadline);

        match deadline {
            Some(deadline) if now >= Instant::from(deadline) => {
                if let Some(run) = self.active_run.as_mut() {
                    run.cooldown_deadline = None;
                }
                let _ = self.complete_run(telemetry, SequenceOutcome::Completed, now.into());
                true
            }
            Some(_) => false,
            None => {
                if let Some(run) = self.active_run.as_mut() {
                    run.cooldown_deadline = Some((now + cooldown).into());
                }
                false
            }
        }
    }

    async fn handle_brown_out(
        &mut self,
        sample: PowerSample,
        telemetry: &mut TelemetryRecorder,
    ) -> bool {
        let (retry_budget, retries_used, did_retry, attempt) = {
            let Some(run) = self.active_run.as_mut() else {
                return false;
            };

            let Some(template) = self.templates.get(run.command.kind) else {
                let _ = self.fail_run(SequenceError::UnexpectedState);
                return false;
            };

            let retry_budget = retry_budget_for(&run.command, template);
            let retries_used = run.retry_count;
            let mut attempt = run.retry_count;
            let mut did_retry = false;

            if retries_used < retry_budget {
                run.begin_retry();
                attempt = run.retry_count;
                did_retry = true;
            }

            (retry_budget, retries_used, did_retry, attempt)
        };

        log_brown_out_detected(&sample, retries_used, retry_budget);

        let release_timestamp = FirmwareInstant::from(Instant::now());
        self.release_all_straps(telemetry, release_timestamp);

        if !did_retry {
            log_retry_exhausted(retry_budget);
            let _ = self.fail_run(SequenceError::RetryLimitExceeded);
            return false;
        }

        log_retry_started(attempt, retry_budget);

        self.recovering_power = true;
        self.await_power_recovery(telemetry, attempt).await;
        self.recovering_power = false;
        true
    }

    async fn await_power_recovery(&mut self, telemetry: &mut TelemetryRecorder, attempt: u8) {
        let holdoff = self.power_monitor.stable_holdoff();
        let mut first_stable: Option<PowerSample> = None;

        loop {
            self.collect_pending_commands(telemetry);

            match self.power_monitor.poll() {
                PowerStatus::Stable(sample) => {
                    self.last_power_sample = Some(sample);
                    if first_stable.is_none() {
                        first_stable = Some(sample);
                    }

                    if let Some(stable_anchor) = first_stable
                        && sample
                            .timestamp
                            .saturating_duration_since(stable_anchor.timestamp)
                            >= holdoff
                    {
                        telemetry.record(
                            TelemetryEventKind::PowerStable,
                            TelemetryPayload::None,
                            sample.timestamp,
                        );
                        log_power_recovered(&sample, attempt, holdoff);
                        return;
                    }
                }
                PowerStatus::BrownOut(sample) => {
                    self.last_power_sample = Some(sample);
                    first_stable = None;
                }
                PowerStatus::Unknown => {
                    let now = Instant::now();
                    let now_instant = FirmwareInstant::from(now);
                    let sample = PowerSample::new(now_instant, None);
                    self.last_power_sample = Some(sample);
                    if first_stable.is_none() {
                        first_stable = Some(sample);
                    }

                    if let Some(stable_anchor) = first_stable
                        && now_instant.saturating_duration_since(stable_anchor.timestamp) >= holdoff
                    {
                        telemetry.record(
                            TelemetryEventKind::PowerStable,
                            TelemetryPayload::None,
                            now_instant,
                        );
                        log_power_recovered(&sample, attempt, holdoff);
                        return;
                    }
                }
            }

            Timer::after(self.idle_delay()).await;
        }
    }

    fn drive_strap_transition(
        &mut self,
        line: StrapId,
        action: StrapAction,
        telemetry: &mut TelemetryRecorder,
        timestamp: FirmwareInstant,
    ) {
        self.strap_driver.apply(line, action);
        log_strap_drive(line, action, timestamp);

        let event_id = telemetry.record_strap_transition(line, action, timestamp);
        if let Some(run) = self.active_run.as_mut() {
            let _ = run.track_event(event_id);
        }
    }

    fn release_all_straps(
        &mut self,
        telemetry: &mut TelemetryRecorder,
        timestamp: FirmwareInstant,
    ) {
        for strap in super::ALL_STRAPS.iter() {
            self.drive_strap_transition(strap.id, StrapAction::ReleaseHigh, telemetry, timestamp);
        }
    }

    fn idle_delay(&self) -> Duration {
        let candidate = core_duration_to_embassy(self.power_monitor.sample_interval());
        let minimum = Duration::from_millis(MIN_PROCESS_SLEEP_MS);
        if candidate < minimum {
            minimum
        } else {
            candidate
        }
    }

    fn collect_pending_commands(&mut self, telemetry: &mut TelemetryRecorder) {
        while let Ok(command) = self.command_rx.try_receive() {
            if !self.control_link_attached {
                self.last_rejection = Some(CommandRejection::control_link_lost(command));
                continue;
            }

            if self.pending_commands.is_full() {
                self.last_rejection = Some(CommandRejection::busy(command));
                continue;
            }

            let timestamp = FirmwareInstant::from(Instant::now());
            let queue_depth = self.pending_commands.len();
            let event_id = telemetry.record_command_pending(
                command.kind,
                queue_depth,
                command.requested_at,
                timestamp,
            );

            let queued = QueuedCommand {
                not_before: compute_not_before(&command),
                command,
                pending_event: Some(event_id),
            };

            debug_assert!(
                self.pending_commands.push_back(queued).is_ok(),
                "pending command queue overflow"
            );
        }
    }

    fn start_queued_command(
        &mut self,
        queued: QueuedCommand,
        telemetry: &mut TelemetryRecorder,
    ) -> Result<(), CommandRejection<FirmwareInstant>> {
        let pending_event = queued.pending_event;
        let not_before = queued.not_before;
        let command = queued.command;

        match self.begin_run(command) {
            Ok(()) => {
                if let Some(run) = self.active_run.as_mut() {
                    if let Some(event_id) = pending_event {
                        let _ = run.track_event(event_id);
                    }

                    let start_event = telemetry.record_command_started(
                        run.command.kind,
                        self.pending_commands.len(),
                        run.command.requested_at,
                        FirmwareInstant::from(Instant::now()),
                    );
                    let _ = run.track_event(start_event);
                }

                Ok(())
            }
            Err(rejection) => {
                if rejection.reason() == CommandRejectionReason::Busy {
                    let queued = QueuedCommand {
                        command: *rejection.command(),
                        pending_event,
                        not_before,
                    };

                    // If reinsertion fails the queue is full; drop the request after flagging busy.
                    if self.pending_commands.push_front(queued).is_err() {
                        self.last_rejection = Some(CommandRejection::busy(*rejection.command()));
                    }
                }

                Err(rejection)
            }
        }
    }
}
