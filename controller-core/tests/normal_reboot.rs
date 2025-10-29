use core::time::Duration;

use controller_core::sequences::normal::{
    NORMAL_REBOOT_COOLDOWN, POWER_PRESS, POWER_PRESS_MAX, POWER_PRESS_MIN, POWER_RELEASE_SETTLE,
    POWER_RELEASE_SETTLE_MAX, POWER_RELEASE_SETTLE_MIN, RESET_PULSE_MIN,
};
use controller_core::sequences::{StrapAction, StrapId, StrapSequenceKind, normal_reboot_template};

#[test]
fn normal_reboot_step_durations_match_spec() {
    let template = normal_reboot_template();
    let steps = template.steps();

    assert_eq!(template.kind, StrapSequenceKind::NormalReboot);
    assert_eq!(steps.len(), 4);

    let press = &steps[0];
    assert_eq!(press.line, StrapId::Pwr);
    assert_eq!(press.action, StrapAction::AssertLow);
    assert_eq!(press.hold_duration(), POWER_PRESS);

    let settle = &steps[1];
    assert_eq!(settle.line, StrapId::Pwr);
    assert_eq!(settle.action, StrapAction::ReleaseHigh);
    assert_eq!(settle.hold_duration(), POWER_RELEASE_SETTLE);

    let reset_pulse = &steps[2];
    assert_eq!(reset_pulse.line, StrapId::Reset);
    assert_eq!(reset_pulse.action, StrapAction::AssertLow);
    assert_eq!(reset_pulse.hold_duration(), RESET_PULSE_MIN);

    let reset_release = &steps[3];
    assert_eq!(reset_release.line, StrapId::Reset);
    assert_eq!(reset_release.action, StrapAction::ReleaseHigh);
    assert_eq!(reset_release.hold_duration(), Duration::ZERO);

    assert_eq!(template.cooldown_duration(), NORMAL_REBOOT_COOLDOWN);
}

#[test]
fn normal_reboot_constraints_honor_timing_windows() {
    let template = normal_reboot_template();
    let steps = template.steps();

    let press = &steps[0];
    assert!(press.constraints.allows_hold(POWER_PRESS_MIN));
    assert!(press.constraints.allows_hold(POWER_PRESS_MAX));
    assert!(
        !press
            .constraints
            .allows_hold(POWER_PRESS_MIN - Duration::from_millis(1))
    );
    assert!(
        !press
            .constraints
            .allows_hold(POWER_PRESS_MAX + Duration::from_millis(1))
    );
    assert_eq!(press.constraints.min_hold_duration(), Some(POWER_PRESS_MIN));
    assert_eq!(press.constraints.max_hold_duration(), Some(POWER_PRESS_MAX));

    let settle = &steps[1];
    assert!(settle.constraints.allows_hold(POWER_RELEASE_SETTLE_MIN));
    assert!(settle.constraints.allows_hold(POWER_RELEASE_SETTLE_MAX));
    assert!(
        !settle
            .constraints
            .allows_hold(POWER_RELEASE_SETTLE_MIN - Duration::from_millis(1))
    );
    assert!(
        !settle
            .constraints
            .allows_hold(POWER_RELEASE_SETTLE_MAX + Duration::from_millis(1))
    );
    assert_eq!(
        settle.constraints.min_hold_duration(),
        Some(POWER_RELEASE_SETTLE_MIN)
    );
    assert_eq!(
        settle.constraints.max_hold_duration(),
        Some(POWER_RELEASE_SETTLE_MAX)
    );

    let reset_pulse = &steps[2];
    assert!(reset_pulse.constraints.allows_hold(RESET_PULSE_MIN));
    assert_eq!(
        reset_pulse.constraints.min_hold_duration(),
        Some(RESET_PULSE_MIN)
    );
    assert_eq!(reset_pulse.constraints.max_hold_duration(), None);
}
