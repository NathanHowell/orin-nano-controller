use super::{COMMAND_QUEUE, REGISTERED_TEMPLATES};
use crate::repl::ReplSession;
use crate::straps::CommandProducer;
use controller_core::orchestrator::SequenceScheduler;
use controller_core::repl::commands::CommandExecutor;

#[embassy_executor::task]
pub async fn run() -> ! {
    let command_sender = COMMAND_QUEUE.sender();
    let producer = CommandProducer::new(command_sender);
    let mut scheduler = SequenceScheduler::new(producer);

    {
        let templates = scheduler.templates_mut();
        for template in REGISTERED_TEMPLATES {
            templates
                .register(template)
                .expect("scheduler template registration");
        }
    }

    let executor = CommandExecutor::new(scheduler);
    let mut session = ReplSession::new(executor);
    session.run().await;
}
