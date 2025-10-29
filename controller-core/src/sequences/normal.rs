//! Normal reboot sequence template shared by firmware and host targets.
//!
//! The sequence enforces the timing windows captured in BS-001: press the
//! Jetson power button strap for 200 ms (±20 ms), allow rails to settle for
//! roughly one second, and then pulse reset for at least 20 ms while keeping
//! the recovery strap released.

use core::time::Duration;

use super::{
    SequenceTemplate, StepCompletion, StrapAction, StrapId, StrapSequenceKind, StrapStep,
    TimingConstraintSet,
};

/// Duration the power strap remains asserted to mimic a front-panel press.
pub const POWER_PRESS: Duration = Duration::from_millis(200);
/// Minimum duration for the power button press.
pub const POWER_PRESS_MIN: Duration = Duration::from_millis(180);
/// Maximum duration for the power button press.
pub const POWER_PRESS_MAX: Duration = Duration::from_millis(220);
/// Minimum settling time after releasing the power strap before toggling reset.
pub const POWER_RELEASE_SETTLE: Duration = Duration::from_millis(1_000);
/// Minimum delay while holding the power strap released.
pub const POWER_RELEASE_SETTLE_MIN: Duration = Duration::from_millis(900);
/// Maximum delay while holding the power strap released.
pub const POWER_RELEASE_SETTLE_MAX: Duration = Duration::from_millis(1_100);
/// Minimum duration the reset strap must remain asserted.
pub const RESET_PULSE_MIN: Duration = Duration::from_millis(20);
/// Cooldown enforced after completing the normal reboot sequence.
pub const NORMAL_REBOOT_COOLDOWN: Duration = Duration::from_millis(1_000);

/// Ordered strap steps that implement the `NormalReboot` sequence.
pub const NORMAL_REBOOT_STEPS: [StrapStep; 4] = [
    // Assert the power strap to simulate the front-panel button press.
    StrapStep::new(
        StrapId::Pwr,
        StrapAction::AssertLow,
        POWER_PRESS,
        TimingConstraintSet::with_hold_range(Some(POWER_PRESS_MIN), Some(POWER_PRESS_MAX)),
        StepCompletion::AfterDuration,
    ),
    // Release the power strap and hold the idle state long enough for rails to settle.
    StrapStep::new(
        StrapId::Pwr,
        StrapAction::ReleaseHigh,
        POWER_RELEASE_SETTLE,
        TimingConstraintSet::with_hold_range(
            Some(POWER_RELEASE_SETTLE_MIN),
            Some(POWER_RELEASE_SETTLE_MAX),
        ),
        StepCompletion::AfterDuration,
    ),
    // Pulse reset low to complete the reboot handoff.
    StrapStep::new(
        StrapId::Reset,
        StrapAction::AssertLow,
        RESET_PULSE_MIN,
        TimingConstraintSet::with_hold_range(Some(RESET_PULSE_MIN), None),
        StepCompletion::AfterDuration,
    ),
    // Return reset to its idle level.
    StrapStep::new(
        StrapId::Reset,
        StrapAction::ReleaseHigh,
        Duration::ZERO,
        TimingConstraintSet::unrestricted(),
        StepCompletion::AfterDuration,
    ),
];

/// Sequence template describing the `NormalReboot` workflow.
pub const NORMAL_REBOOT_TEMPLATE: SequenceTemplate = SequenceTemplate::new(
    StrapSequenceKind::NormalReboot,
    &NORMAL_REBOOT_STEPS,
    NORMAL_REBOOT_COOLDOWN,
    None,
);

/// Returns the shared `NormalReboot` template.
#[must_use]
pub const fn normal_reboot_template() -> SequenceTemplate {
    NORMAL_REBOOT_TEMPLATE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_reboot_matches_spec_timings() {
        assert_eq!(NORMAL_REBOOT_TEMPLATE.kind, StrapSequenceKind::NormalReboot);
        assert_eq!(NORMAL_REBOOT_TEMPLATE.step_count(), 4);

        let press = &NORMAL_REBOOT_STEPS[0];
        assert_eq!(press.line, StrapId::Pwr);
        assert_eq!(press.action, StrapAction::AssertLow);
        assert_eq!(press.hold_for, POWER_PRESS);
        assert_eq!(press.constraints.min_hold, Some(POWER_PRESS_MIN));
        assert_eq!(press.constraints.max_hold, Some(POWER_PRESS_MAX));

        let settle = &NORMAL_REBOOT_STEPS[1];
        assert_eq!(settle.line, StrapId::Pwr);
        assert_eq!(settle.action, StrapAction::ReleaseHigh);
        assert_eq!(settle.hold_for, POWER_RELEASE_SETTLE);
        assert_eq!(settle.constraints.min_hold, Some(POWER_RELEASE_SETTLE_MIN));
        assert_eq!(settle.constraints.max_hold, Some(POWER_RELEASE_SETTLE_MAX));

        let reset_pulse = &NORMAL_REBOOT_STEPS[2];
        assert_eq!(reset_pulse.line, StrapId::Reset);
        assert_eq!(reset_pulse.action, StrapAction::AssertLow);
        assert_eq!(reset_pulse.hold_for, RESET_PULSE_MIN);
        assert_eq!(reset_pulse.constraints.min_hold, Some(RESET_PULSE_MIN));
        assert_eq!(reset_pulse.constraints.max_hold, None);

        assert_eq!(NORMAL_REBOOT_TEMPLATE.cooldown, NORMAL_REBOOT_COOLDOWN);
        assert_eq!(NORMAL_REBOOT_TEMPLATE.max_retries, None);
    }
}
