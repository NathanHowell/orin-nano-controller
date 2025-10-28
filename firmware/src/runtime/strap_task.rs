use crate::straps::orchestrator::{HardwareStrapDriver, NoopPowerMonitor, StrapOrchestrator};
use crate::telemetry::TelemetryRecorder;

#[embassy_executor::task]
pub async fn run(
    orchestrator: StrapOrchestrator<'static, NoopPowerMonitor, HardwareStrapDriver<'static>>,
    mut telemetry: TelemetryRecorder,
) -> ! {
    orchestrator.run(&mut telemetry).await;
}
