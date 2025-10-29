//! Recovery sequence templates shared by firmware and host targets.
//!
//! Two variants are provided:
//! - `RecoveryEntry` (triggered by `recovery enter`) enforces the REC pre/post
//!   windows around a reset pulse and then releases the strap.
//! - `RecoveryImmediate` (triggered by `recovery now`) holds REC asserted until
//!   Jetson console activity appears on the UART bridge, fulfilling FR-005.

use core::time::Duration;

use super::{
    SequenceTemplate, StepCompletion, StrapAction, StrapId, StrapSequenceKind, StrapStep,
    TimingConstraintSet,
};

/// Minimum period REC must be asserted prior to toggling reset.
pub const RECOVERY_PRE_RESET_HOLD: Duration = Duration::from_millis(100);
/// Minimum period REC remains asserted after reset is released.
pub const RECOVERY_POST_RESET_HOLD: Duration = Duration::from_millis(500);
/// Cooldown enforced after a recovery sequence completes.
pub const RECOVERY_COOLDOWN: Duration = Duration::from_millis(1_000);
/// Minimum duration RESET must remain asserted during recovery.
pub const RECOVERY_RESET_PULSE_MIN: Duration = Duration::from_millis(20);

const REC_ASSERT_PRE_STEP: StrapStep = StrapStep::new(
    StrapId::Rec,
    StrapAction::AssertLow,
    RECOVERY_PRE_RESET_HOLD,
    TimingConstraintSet::with_hold_range(Some(RECOVERY_PRE_RESET_HOLD), None),
    StepCompletion::AfterDuration,
);

const RESET_ASSERT_STEP: StrapStep = StrapStep::new(
    StrapId::Reset,
    StrapAction::AssertLow,
    RECOVERY_RESET_PULSE_MIN,
    TimingConstraintSet::with_hold_range(Some(RECOVERY_RESET_PULSE_MIN), None),
    StepCompletion::AfterDuration,
);

const RESET_RELEASE_STEP: StrapStep = StrapStep::new(
    StrapId::Reset,
    StrapAction::ReleaseHigh,
    Duration::ZERO,
    TimingConstraintSet::unrestricted(),
    StepCompletion::AfterDuration,
);

const REC_POST_HOLD_STEP: StrapStep = StrapStep::new(
    StrapId::Rec,
    StrapAction::AssertLow,
    RECOVERY_POST_RESET_HOLD,
    TimingConstraintSet::with_hold_range(Some(RECOVERY_POST_RESET_HOLD), None),
    StepCompletion::AfterDuration,
);

const REC_RELEASE_STEP: StrapStep = StrapStep::new(
    StrapId::Rec,
    StrapAction::ReleaseHigh,
    Duration::ZERO,
    TimingConstraintSet::unrestricted(),
    StepCompletion::AfterDuration,
);

const REC_WAIT_FOR_ACTIVITY_STEP: StrapStep = StrapStep::new(
    StrapId::Rec,
    StrapAction::AssertLow,
    Duration::ZERO,
    TimingConstraintSet::unrestricted(),
    StepCompletion::OnBridgeActivity,
);

/// Ordered steps for the `RecoveryEntry` sequence.
pub const RECOVERY_ENTRY_STEPS: [StrapStep; 5] = [
    REC_ASSERT_PRE_STEP,
    RESET_ASSERT_STEP,
    RESET_RELEASE_STEP,
    REC_POST_HOLD_STEP,
    REC_RELEASE_STEP,
];

/// Ordered steps for the `RecoveryImmediate` sequence.
pub const RECOVERY_IMMEDIATE_STEPS: [StrapStep; 6] = [
    REC_ASSERT_PRE_STEP,
    RESET_ASSERT_STEP,
    RESET_RELEASE_STEP,
    REC_POST_HOLD_STEP,
    REC_WAIT_FOR_ACTIVITY_STEP,
    REC_RELEASE_STEP,
];

/// Template describing the RecoveryEntry sequence.
pub const RECOVERY_ENTRY_TEMPLATE: SequenceTemplate = SequenceTemplate::new(
    StrapSequenceKind::RecoveryEntry,
    &RECOVERY_ENTRY_STEPS,
    RECOVERY_COOLDOWN,
    None,
);

/// Template describing the RecoveryImmediate sequence.
pub const RECOVERY_IMMEDIATE_TEMPLATE: SequenceTemplate = SequenceTemplate::new(
    StrapSequenceKind::RecoveryImmediate,
    &RECOVERY_IMMEDIATE_STEPS,
    RECOVERY_COOLDOWN,
    None,
);

/// Returns the RecoveryEntry template.
pub const fn recovery_entry_template() -> SequenceTemplate {
    RECOVERY_ENTRY_TEMPLATE
}

/// Returns the RecoveryImmediate template.
pub const fn recovery_immediate_template() -> SequenceTemplate {
    RECOVERY_IMMEDIATE_TEMPLATE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_entry_enforces_rec_windows() {
        assert_eq!(
            RECOVERY_ENTRY_TEMPLATE.kind,
            StrapSequenceKind::RecoveryEntry
        );
        assert_eq!(RECOVERY_ENTRY_TEMPLATE.step_count(), 5);

        let pre_hold = &RECOVERY_ENTRY_STEPS[0];
        assert_eq!(pre_hold.line, StrapId::Rec);
        assert_eq!(pre_hold.action, StrapAction::AssertLow);
        assert_eq!(pre_hold.hold_for, RECOVERY_PRE_RESET_HOLD);
        assert_eq!(pre_hold.constraints.min_hold, Some(RECOVERY_PRE_RESET_HOLD));
        assert_eq!(pre_hold.constraints.max_hold, None);

        let post_hold = &RECOVERY_ENTRY_STEPS[3];
        assert_eq!(post_hold.line, StrapId::Rec);
        assert_eq!(post_hold.hold_for, RECOVERY_POST_RESET_HOLD);
        assert_eq!(
            post_hold.constraints.min_hold,
            Some(RECOVERY_POST_RESET_HOLD)
        );
        assert_eq!(post_hold.constraints.max_hold, None);

        let release = &RECOVERY_ENTRY_STEPS[4];
        assert_eq!(release.line, StrapId::Rec);
        assert_eq!(release.action, StrapAction::ReleaseHigh);
    }

    #[test]
    fn recovery_immediate_waits_for_bridge_activity() {
        assert_eq!(
            RECOVERY_IMMEDIATE_TEMPLATE.kind,
            StrapSequenceKind::RecoveryImmediate
        );
        assert_eq!(RECOVERY_IMMEDIATE_TEMPLATE.step_count(), 6);

        let wait_step = &RECOVERY_IMMEDIATE_STEPS[4];
        assert_eq!(wait_step.line, StrapId::Rec);
        assert_eq!(wait_step.action, StrapAction::AssertLow);
        assert_eq!(wait_step.completion, StepCompletion::OnBridgeActivity);

        let release = &RECOVERY_IMMEDIATE_STEPS[5];
        assert_eq!(release.line, StrapId::Rec);
        assert_eq!(release.action, StrapAction::ReleaseHigh);
        assert_eq!(release.completion, StepCompletion::AfterDuration);
    }
}
