use core::time::Duration;

use controller_core::sequences::recovery::{
    RECOVERY_COOLDOWN, RECOVERY_POST_RESET_HOLD, RECOVERY_PRE_RESET_HOLD, recovery_entry_template,
    recovery_immediate_template,
};
use controller_core::sequences::{StepCompletion, StrapAction, StrapId, StrapSequenceKind};

#[test]
fn recovery_entry_rec_hold_windows_are_enforced() {
    let template = recovery_entry_template();
    let steps = template.steps();

    assert_eq!(template.kind, StrapSequenceKind::RecoveryEntry);
    assert_eq!(
        steps.len(),
        5,
        "expected REC entry sequence to have 5 steps"
    );

    let pre_hold = &steps[0];
    assert_eq!(pre_hold.line, StrapId::Rec);
    assert_eq!(pre_hold.action, StrapAction::AssertLow);
    assert_eq!(pre_hold.hold_duration(), RECOVERY_PRE_RESET_HOLD);
    assert_eq!(
        pre_hold.constraints.min_hold_duration(),
        Some(RECOVERY_PRE_RESET_HOLD)
    );

    let post_hold = &steps[3];
    assert_eq!(post_hold.line, StrapId::Rec);
    assert_eq!(post_hold.action, StrapAction::AssertLow);
    assert_eq!(post_hold.hold_duration(), RECOVERY_POST_RESET_HOLD);
    assert_eq!(
        post_hold.constraints.min_hold_duration(),
        Some(RECOVERY_POST_RESET_HOLD)
    );

    let release = &steps[4];
    assert_eq!(release.line, StrapId::Rec);
    assert_eq!(release.action, StrapAction::ReleaseHigh);
    assert_eq!(release.completion, StepCompletion::AfterDuration);

    assert_eq!(template.cooldown_duration(), RECOVERY_COOLDOWN);
}

#[test]
fn recovery_immediate_waits_for_bridge_activity_before_releasing_rec() {
    let template = recovery_immediate_template();
    let steps = template.steps();

    assert_eq!(template.kind, StrapSequenceKind::RecoveryImmediate);
    assert_eq!(
        steps.len(),
        6,
        "expected REC immediate sequence to have 6 steps"
    );

    // REC is asserted before reset and stays low until bridge activity is observed.
    let initial = &steps[0];
    assert_eq!(initial.line, StrapId::Rec);
    assert_eq!(initial.action, StrapAction::AssertLow);

    let wait_step = &steps[4];
    assert_eq!(wait_step.line, StrapId::Rec);
    assert_eq!(wait_step.action, StrapAction::AssertLow);
    assert_eq!(wait_step.completion, StepCompletion::OnBridgeActivity);
    assert_eq!(
        wait_step.hold_duration(),
        Duration::ZERO,
        "bridge wait holds indefinitely until activity"
    );

    let release = &steps[5];
    assert_eq!(release.line, StrapId::Rec);
    assert_eq!(release.action, StrapAction::ReleaseHigh);
    assert_eq!(release.completion, StepCompletion::AfterDuration);
}
