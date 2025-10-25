//! Predefined strap sequence templates.
//!
//! This module contains the concrete strap sequencing data derived from the
//! feature specification. Each sequence encodes the strap order, hold timings,
//! and cooldown expectations required by the orchestrator.

use embassy_time::Duration;

use super::{
    SequenceTemplate, StrapAction, StrapLineId, StrapSequenceKind, StrapStep, TimingConstraintSet,
};

const POWER_PRESS_MS: u64 = 200;
const POWER_PRESS_TOLERANCE_MS: u64 = 20;
const POWER_RELEASE_COOLDOWN_MS: u64 = 1_000;
const RESET_ASSERT_MS: u64 = 20;
const TEMPLATE_COOLDOWN_MS: u64 = 1_000;

/// Builds the `NormalReboot` sequence template as defined by requirement BS-001.
///
/// Sequence timeline:
/// 1. Press the Jetson power button (`PWR*` low for 200 ms ±20 ms).
/// 2. Allow a 1 s high interval to satisfy the mandated cool-down before reusing `PWR*`.
/// 3. Pulse `RESET*` low for 20 ms (minimum), then release while keeping recovery high.
pub fn normal_reboot_template() -> SequenceTemplate {
    let mut template = SequenceTemplate::new(
        StrapSequenceKind::NormalReboot,
        Duration::from_millis(TEMPLATE_COOLDOWN_MS),
        None,
    );

    let mut power_press = StrapStep::timed(
        StrapLineId::Power,
        StrapAction::AssertLow,
        Duration::from_millis(POWER_PRESS_MS),
    );
    power_press.constraints = TimingConstraintSet {
        min_hold: Some(Duration::from_millis(
            POWER_PRESS_MS - POWER_PRESS_TOLERANCE_MS,
        )),
        max_hold: Some(Duration::from_millis(
            POWER_PRESS_MS + POWER_PRESS_TOLERANCE_MS,
        )),
        ..TimingConstraintSet::default()
    };
    template
        .phases
        .push(power_press)
        .expect("normal reboot template overflow");

    let mut power_release = StrapStep::timed(
        StrapLineId::Power,
        StrapAction::ReleaseHigh,
        Duration::from_millis(POWER_RELEASE_COOLDOWN_MS),
    );
    power_release.constraints.min_hold = Some(Duration::from_millis(POWER_RELEASE_COOLDOWN_MS));
    template
        .phases
        .push(power_release)
        .expect("normal reboot template overflow");

    let mut reset_assert = StrapStep::timed(
        StrapLineId::Reset,
        StrapAction::AssertLow,
        Duration::from_millis(RESET_ASSERT_MS),
    );
    reset_assert.constraints.min_hold = Some(Duration::from_millis(RESET_ASSERT_MS));
    template
        .phases
        .push(reset_assert)
        .expect("normal reboot template overflow");

    let reset_release = StrapStep::timed(
        StrapLineId::Reset,
        StrapAction::ReleaseHigh,
        Duration::from_millis(0),
    );
    template
        .phases
        .push(reset_release)
        .expect("normal reboot template overflow");

    template
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_reboot_template_matches_spec() {
        let template = normal_reboot_template();
        assert_eq!(template.kind, StrapSequenceKind::NormalReboot);
        assert_eq!(template.max_retries, None);
        assert_eq!(
            template.cooldown,
            Duration::from_millis(TEMPLATE_COOLDOWN_MS)
        );

        let phases = template.phases.as_slice();
        assert_eq!(phases.len(), 4);

        let step = &phases[0];
        assert_eq!(step.line, StrapLineId::Power);
        assert_eq!(step.action, StrapAction::AssertLow);
        assert_eq!(step.hold_for, Duration::from_millis(POWER_PRESS_MS));
        assert_eq!(
            step.constraints.min_hold,
            Some(Duration::from_millis(
                POWER_PRESS_MS - POWER_PRESS_TOLERANCE_MS
            ))
        );
        assert_eq!(
            step.constraints.max_hold,
            Some(Duration::from_millis(
                POWER_PRESS_MS + POWER_PRESS_TOLERANCE_MS
            ))
        );

        let step = &phases[1];
        assert_eq!(step.line, StrapLineId::Power);
        assert_eq!(step.action, StrapAction::ReleaseHigh);
        assert_eq!(
            step.hold_for,
            Duration::from_millis(POWER_RELEASE_COOLDOWN_MS)
        );
        assert_eq!(
            step.constraints.min_hold,
            Some(Duration::from_millis(POWER_RELEASE_COOLDOWN_MS))
        );

        let step = &phases[2];
        assert_eq!(step.line, StrapLineId::Reset);
        assert_eq!(step.action, StrapAction::AssertLow);
        assert_eq!(step.hold_for, Duration::from_millis(RESET_ASSERT_MS));
        assert_eq!(
            step.constraints.min_hold,
            Some(Duration::from_millis(RESET_ASSERT_MS))
        );

        let step = &phases[3];
        assert_eq!(step.line, StrapLineId::Reset);
        assert_eq!(step.action, StrapAction::ReleaseHigh);
        assert_eq!(step.hold_for, Duration::from_millis(0));
    }
}
