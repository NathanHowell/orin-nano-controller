use core::ops::Add;
use core::time::Duration;

use controller_core::orchestrator::{
    CommandEnqueueError, CommandQueueProducer, CommandSource, SequenceCommand, SequenceScheduler,
};
use controller_core::repl::commands::{CommandError, CommandExecutor, CommandOutcome};
use controller_core::sequences::{
    fault::FAULT_RECOVERY_MAX_RETRIES, fault_recovery_template, StrapSequenceKind,
};
use heapless::Vec as HeaplessVec;

#[test]
fn fault_recovery_template_reports_retry_budget() {
    let template = fault_recovery_template();

    assert_eq!(template.kind, StrapSequenceKind::FaultRecovery);
    assert_eq!(
        template.max_retries,
        Some(FAULT_RECOVERY_MAX_RETRIES),
        "fault recovery template should expose retry budget for exhaustion handling"
    );
}

#[test]
fn fault_recover_defaults_to_template_retry_budget() {
    let mut executor = build_executor();
    let now = MockInstant::micros(1);

    let outcome = executor
        .execute("fault recover", now, CommandSource::UsbHost)
        .expect("command should succeed");

    let ack = match outcome {
        CommandOutcome::Fault(ack) => ack,
        other => panic!("unexpected command outcome: {other:?}"),
    };

    assert_eq!(ack.sequence, StrapSequenceKind::FaultRecovery);
    assert_eq!(ack.retry_budget, FAULT_RECOVERY_MAX_RETRIES);

    let commands = executor.scheduler().producer().commands();
    assert_eq!(commands.len(), 1, "fault recover should enqueue exactly one command");

    let scheduled = &commands[0];
    assert_eq!(scheduled.kind, StrapSequenceKind::FaultRecovery);
    assert_eq!(
        scheduled.flags.retry_override, None,
        "default invocation should rely on the template retry budget"
    );
}

#[test]
fn fault_recover_propagates_retry_override() {
    let mut executor = build_executor();
    let now = MockInstant::micros(5);

    let outcome = executor
        .execute("fault recover retries=2", now, CommandSource::UsbHost)
        .expect("command should succeed");

    let ack = match outcome {
        CommandOutcome::Fault(ack) => ack,
        other => panic!("unexpected command outcome: {other:?}"),
    };

    assert_eq!(ack.retry_budget, 2);

    let commands = executor.scheduler().producer().commands();
    assert_eq!(commands.len(), 1);
    assert_eq!(
        commands[0].flags.retry_override,
        Some(2),
        "override must be forwarded so runtime can detect exhaustion"
    );
}

#[test]
fn fault_recover_rejects_retry_override_below_one() {
    let mut executor = build_executor();
    let now = MockInstant::micros(9);

    let error = executor
        .execute("fault recover retries=0", now, CommandSource::UsbHost)
        .expect_err("retry override of 0 should be rejected");

    match error {
        CommandError::Unsupported(message) => {
            assert!(
                message.contains("retries"),
                "expected unsupported message to mention retries, got: {message}"
            );
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[test]
fn fault_recover_rejects_retry_override_exceeding_budget() {
    let mut executor = build_executor();
    let now = MockInstant::micros(10);

    let error = executor
        .execute(
            "fault recover retries=4",
            now,
            CommandSource::UsbHost,
        )
        .expect_err("override above template budget should fail");

    match error {
        CommandError::Unsupported(message) => {
            assert!(
                message.contains("1-3"),
                "expected unsupported message to reference retry range, got: {message}"
            );
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}

fn build_executor() -> CommandExecutor<SequenceScheduler<MockQueue>> {
    let queue = MockQueue::new(4);
    let mut scheduler = SequenceScheduler::new(queue);

    scheduler
        .templates_mut()
        .register(fault_recovery_template())
        .expect("registering fault recovery template should succeed");

    CommandExecutor::new(scheduler)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct MockInstant(u64);

impl MockInstant {
    fn micros(value: u64) -> Self {
        Self(value)
    }
}

impl Add<Duration> for MockInstant {
    type Output = Self;

    fn add(self, rhs: Duration) -> Self::Output {
        Self(self.0 + rhs.as_micros() as u64)
    }
}

#[derive(Clone)]
struct MockQueue {
    capacity: usize,
    commands: HeaplessVec<SequenceCommand<MockInstant>, 8>,
}

impl MockQueue {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            commands: HeaplessVec::new(),
        }
    }

    fn commands(&self) -> &[SequenceCommand<MockInstant>] {
        &self.commands
    }
}

impl CommandQueueProducer for MockQueue {
    type Instant = MockInstant;
    type Error = ();

    fn try_enqueue(
        &mut self,
        command: SequenceCommand<Self::Instant>,
    ) -> Result<(), CommandEnqueueError<Self::Error>> {
        if self.commands.len() >= self.capacity {
            return Err(CommandEnqueueError::QueueFull);
        }

        self.commands
            .push(command)
            .map_err(|_| CommandEnqueueError::QueueFull)
    }

    fn capacity(&self) -> Option<usize> {
        Some(self.capacity)
    }

    fn len(&self) -> Option<usize> {
        Some(self.commands.len())
    }
}
