use core::time::Duration;

use controller_core::sequences::normal::{
    POWER_PRESS_MAX_MS, POWER_PRESS_MIN_MS, POWER_PRESS_MS, POWER_RELEASE_SETTLE_MAX_MS,
    POWER_RELEASE_SETTLE_MIN_MS, POWER_RELEASE_SETTLE_MS, RESET_PULSE_MIN_MS,
};
use controller_core::sequences::{
    Milliseconds, StrapAction, StrapId, StrapSequenceKind, normal_reboot_template,
};

#[test]
fn normal_reboot_step_durations_match_spec() {
    let template = normal_reboot_template();
    let steps = template.steps();

    assert_eq!(template.kind, StrapSequenceKind::NormalReboot);
    assert_eq!(steps.len(), 4);

    let press = &steps[0];
    assert_eq!(press.line, StrapId::Pwr);
    assert_eq!(press.action, StrapAction::AssertLow);
    assert_eq!(
        press.hold_duration(),
        Duration::from_millis(POWER_PRESS_MS.as_u32() as u64)
    );

    let settle = &steps[1];
    assert_eq!(settle.line, StrapId::Pwr);
    assert_eq!(settle.action, StrapAction::ReleaseHigh);
    assert_eq!(
        settle.hold_duration(),
        Duration::from_millis(POWER_RELEASE_SETTLE_MS.as_u32() as u64)
    );

    let reset_pulse = &steps[2];
    assert_eq!(reset_pulse.line, StrapId::Reset);
    assert_eq!(reset_pulse.action, StrapAction::AssertLow);
    assert_eq!(
        reset_pulse.hold_duration(),
        Duration::from_millis(RESET_PULSE_MIN_MS.as_u32() as u64)
    );

    let reset_release = &steps[3];
    assert_eq!(reset_release.line, StrapId::Reset);
    assert_eq!(reset_release.action, StrapAction::ReleaseHigh);
    assert_eq!(
        reset_release.hold_duration(),
        Duration::from_millis(Milliseconds::ZERO.as_u32() as u64)
    );

    assert_eq!(template.cooldown_duration(), Duration::from_millis(1_000));
}

#[test]
fn normal_reboot_constraints_honor_timing_windows() {
    let template = normal_reboot_template();
    let steps = template.steps();

    let press = &steps[0];
    assert!(press.constraints.allows_hold(POWER_PRESS_MIN_MS));
    assert!(press.constraints.allows_hold(POWER_PRESS_MAX_MS));
    assert!(
        !press
            .constraints
            .allows_hold(Milliseconds::new(POWER_PRESS_MIN_MS.as_u32() - 1))
    );
    assert!(
        !press
            .constraints
            .allows_hold(Milliseconds::new(POWER_PRESS_MAX_MS.as_u32() + 1))
    );
    assert_eq!(
        press.constraints.min_hold_duration(),
        Some(Duration::from_millis(POWER_PRESS_MIN_MS.as_u32() as u64))
    );
    assert_eq!(
        press.constraints.max_hold_duration(),
        Some(Duration::from_millis(POWER_PRESS_MAX_MS.as_u32() as u64))
    );

    let settle = &steps[1];
    assert!(settle.constraints.allows_hold(POWER_RELEASE_SETTLE_MIN_MS));
    assert!(settle.constraints.allows_hold(POWER_RELEASE_SETTLE_MAX_MS));
    assert!(
        !settle
            .constraints
            .allows_hold(Milliseconds::new(POWER_RELEASE_SETTLE_MIN_MS.as_u32() - 1))
    );
    assert!(
        !settle
            .constraints
            .allows_hold(Milliseconds::new(POWER_RELEASE_SETTLE_MAX_MS.as_u32() + 1))
    );
    assert_eq!(
        settle.constraints.min_hold_duration(),
        Some(Duration::from_millis(
            POWER_RELEASE_SETTLE_MIN_MS.as_u32() as u64
        ))
    );
    assert_eq!(
        settle.constraints.max_hold_duration(),
        Some(Duration::from_millis(
            POWER_RELEASE_SETTLE_MAX_MS.as_u32() as u64
        ))
    );

    let reset_pulse = &steps[2];
    assert!(reset_pulse.constraints.allows_hold(RESET_PULSE_MIN_MS));
    assert_eq!(
        reset_pulse.constraints.min_hold_duration(),
        Some(Duration::from_millis(RESET_PULSE_MIN_MS.as_u32() as u64))
    );
    assert_eq!(reset_pulse.constraints.max_hold_duration(), None);
}
