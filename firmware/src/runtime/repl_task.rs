use super::COMMAND_QUEUE;
use crate::repl::{FirmwareStatusProvider, ReplSession};
use crate::straps::CommandProducer;
use controller_core::orchestrator::{SequenceScheduler, register_default_templates};
use controller_core::repl::commands::CommandExecutor;

#[embassy_executor::task]
pub async fn run() -> ! {
    let command_sender = COMMAND_QUEUE.sender();
    let producer = CommandProducer::new(command_sender);
    let mut scheduler = SequenceScheduler::new(producer);

    {
        let templates = scheduler.templates_mut();
        register_default_templates(templates).expect("scheduler template registration");
    }

    let executor = CommandExecutor::new(scheduler).with_status_provider(FirmwareStatusProvider::default());
    let mut session = ReplSession::new(executor);
    session.run().await;
}
