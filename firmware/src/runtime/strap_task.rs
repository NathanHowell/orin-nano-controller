use crate::straps::orchestrator::{FirmwarePowerMonitor, HardwareStrapDriver, StrapOrchestrator};
use crate::telemetry::TelemetryRecorder;

#[embassy_executor::task]
pub async fn run(
    orchestrator: StrapOrchestrator<
        'static,
        FirmwarePowerMonitor<'static>,
        HardwareStrapDriver<'static>,
    >,
    mut telemetry: TelemetryRecorder,
) -> ! {
    orchestrator.run(&mut telemetry).await;
}
