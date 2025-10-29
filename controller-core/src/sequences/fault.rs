//! Fault recovery sequence template shared by firmware and host targets.
//!
//! The sequence asserts the APO strap long enough to guarantee the Jetson
//! performs a hard power cut, then reuses the normal reboot workflow. The
//! orchestrator may retry up to three times if downstream checks fail.

use core::time::Duration;

use super::{
    SequenceTemplate, StepCompletion, StrapAction, StrapId, StrapSequenceKind, StrapStep,
    TimingConstraintSet,
    normal::{NORMAL_REBOOT_COOLDOWN, NORMAL_REBOOT_STEPS},
};

/// Required duration to keep the APO strap asserted before a reboot attempt.
pub const APO_PRECHARGE: Duration = Duration::from_millis(250);
/// Cooldown enforced after the fault recovery sequence completes.
pub const FAULT_RECOVERY_COOLDOWN: Duration = NORMAL_REBOOT_COOLDOWN;
/// Maximum number of retries permitted by the fault recovery workflow.
pub const FAULT_RECOVERY_MAX_RETRIES: u8 = 3;

const APO_ASSERT_STEP: StrapStep = StrapStep::new(
    StrapId::Apo,
    StrapAction::AssertLow,
    APO_PRECHARGE,
    TimingConstraintSet::with_hold_range(Some(APO_PRECHARGE), Some(APO_PRECHARGE)),
    StepCompletion::AfterDuration,
);

const APO_RELEASE_STEP: StrapStep = StrapStep::new(
    StrapId::Apo,
    StrapAction::ReleaseHigh,
    Duration::ZERO,
    TimingConstraintSet::unrestricted(),
    StepCompletion::AfterDuration,
);

/// Ordered steps describing the fault recovery workflow.
pub const FAULT_RECOVERY_STEPS: [StrapStep; 6] = [
    APO_ASSERT_STEP,
    APO_RELEASE_STEP,
    NORMAL_REBOOT_STEPS[0],
    NORMAL_REBOOT_STEPS[1],
    NORMAL_REBOOT_STEPS[2],
    NORMAL_REBOOT_STEPS[3],
];

/// Sequence template for the fault recovery workflow.
pub const FAULT_RECOVERY_TEMPLATE: SequenceTemplate = SequenceTemplate::new(
    StrapSequenceKind::FaultRecovery,
    &FAULT_RECOVERY_STEPS,
    FAULT_RECOVERY_COOLDOWN,
    Some(FAULT_RECOVERY_MAX_RETRIES),
);

/// Returns the fault recovery template.
pub const fn fault_recovery_template() -> SequenceTemplate {
    FAULT_RECOVERY_TEMPLATE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequences::normal::NORMAL_REBOOT_TEMPLATE;

    #[test]
    fn fault_recovery_prepends_apo_hold_to_normal_reboot() {
        assert_eq!(
            FAULT_RECOVERY_TEMPLATE.kind,
            StrapSequenceKind::FaultRecovery
        );
        assert_eq!(FAULT_RECOVERY_TEMPLATE.step_count(), 6);

        let apo_assert = &FAULT_RECOVERY_STEPS[0];
        assert_eq!(apo_assert.line, StrapId::Apo);
        assert_eq!(apo_assert.action, StrapAction::AssertLow);
        assert_eq!(apo_assert.hold_for, APO_PRECHARGE);
        assert_eq!(apo_assert.constraints.min_hold, Some(APO_PRECHARGE));
        assert_eq!(apo_assert.constraints.max_hold, Some(APO_PRECHARGE));

        let apo_release = &FAULT_RECOVERY_STEPS[1];
        assert_eq!(apo_release.line, StrapId::Apo);
        assert_eq!(apo_release.action, StrapAction::ReleaseHigh);
        assert_eq!(apo_release.hold_for, Duration::ZERO);

        assert_eq!(&FAULT_RECOVERY_STEPS[2..], NORMAL_REBOOT_TEMPLATE.steps());

        assert_eq!(FAULT_RECOVERY_TEMPLATE.cooldown, FAULT_RECOVERY_COOLDOWN);
        assert_eq!(
            FAULT_RECOVERY_TEMPLATE.max_retries,
            Some(FAULT_RECOVERY_MAX_RETRIES)
        );
    }
}
