//! Strap sequence data structures shared by firmware and host targets.
//!
//! The orchestrator uses these definitions to drive asynchronous strap
//! state machines without embedding any MCU-specific knowledge. Everything
//! in this module is `no_std` friendly so the same data can be compiled for
//! both the STM32 firmware and the host-side emulator.

use core::time::Duration;

use crate::telemetry::TelemetryEventKind;

pub mod fault;
pub mod normal;
pub mod recovery;

pub use fault::{FAULT_RECOVERY_TEMPLATE, fault_recovery_template};
pub use normal::{NORMAL_REBOOT_TEMPLATE, normal_reboot_template};
pub use recovery::{
    RECOVERY_ENTRY_TEMPLATE, RECOVERY_IMMEDIATE_TEMPLATE, recovery_entry_template,
    recovery_immediate_template,
};

/// Longest sequence we expect to encode (FaultRecovery) plus one step of headroom.
pub const MAX_SEQUENCE_STEPS: usize = 8;

/// Identifier for the logical strap lines exposed by the controller.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StrapId {
    Reset,
    Rec,
    Pwr,
    Apo,
}

impl StrapId {
    /// Deterministic index for lookups into [`ALL_STRAPS`].
    pub const fn as_index(self) -> usize {
        match self {
            StrapId::Reset => 0,
            StrapId::Rec => 1,
            StrapId::Pwr => 2,
            StrapId::Apo => 3,
        }
    }

    /// Attempts to construct a [`StrapId`] from a raw index.
    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(StrapId::Reset),
            1 => Some(StrapId::Rec),
            2 => Some(StrapId::Pwr),
            3 => Some(StrapId::Apo),
            _ => None,
        }
    }
}

/// Strap polarity as wired on the STM32 and SN74LVC07 driver.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StrapPolarity {
    ActiveLow,
    ActiveHigh,
}

/// Logical idle state for a strap line.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StrapIdleState {
    ReleasedHigh,
    ReleasedLow,
    AssertedHigh,
    AssertedLow,
}

/// Metadata describing how a strap line is routed on the board.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct StrapLine {
    pub id: StrapId,
    pub name: &'static str,
    pub mcu_pin: &'static str,
    pub driver_output: &'static str,
    pub j14_pin: u8,
    pub polarity: StrapPolarity,
    pub default_state: StrapIdleState,
}

impl StrapLine {
    pub const fn new(
        id: StrapId,
        name: &'static str,
        mcu_pin: &'static str,
        driver_output: &'static str,
        j14_pin: u8,
        polarity: StrapPolarity,
        default_state: StrapIdleState,
    ) -> Self {
        Self {
            id,
            name,
            mcu_pin,
            driver_output,
            j14_pin,
            polarity,
            default_state,
        }
    }
}

/// Compile-time catalog of every strap line.
pub const ALL_STRAPS: [StrapLine; 4] = [
    StrapLine::new(
        StrapId::Reset,
        "RESET*",
        "PA4",
        "SN74LVC07-2Y",
        8,
        StrapPolarity::ActiveLow,
        StrapIdleState::ReleasedHigh,
    ),
    StrapLine::new(
        StrapId::Rec,
        "REC*",
        "PA3",
        "SN74LVC07-1Y",
        10,
        StrapPolarity::ActiveLow,
        StrapIdleState::ReleasedHigh,
    ),
    StrapLine::new(
        StrapId::Pwr,
        "PWR*",
        "PA2",
        "SN74LVC07-2Y",
        12,
        StrapPolarity::ActiveLow,
        StrapIdleState::ReleasedHigh,
    ),
    StrapLine::new(
        StrapId::Apo,
        "APO",
        "PA5",
        "SN74LVC07-1Y",
        5,
        StrapPolarity::ActiveLow,
        StrapIdleState::ReleasedHigh,
    ),
];

/// Retrieve strap metadata by identifier.
pub const fn strap_by_id(id: StrapId) -> StrapLine {
    ALL_STRAPS[id.as_index()]
}

/// Action taken on a strap during a step.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StrapAction {
    AssertLow,
    ReleaseHigh,
}

/// Optional timing guardrails associated with a step.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TimingConstraintSet {
    pub min_hold: Option<Duration>,
    pub max_hold: Option<Duration>,
    pub pre_assert_delay: Option<Duration>,
    pub post_release_delay: Option<Duration>,
}

impl TimingConstraintSet {
    /// Constraints with no additional limits.
    pub const fn unrestricted() -> Self {
        Self {
            min_hold: None,
            max_hold: None,
            pre_assert_delay: None,
            post_release_delay: None,
        }
    }

    /// Create a constraint that bounds the hold duration.
    pub const fn with_hold_range(min_hold: Option<Duration>, max_hold: Option<Duration>) -> Self {
        Self {
            min_hold,
            max_hold,
            pre_assert_delay: None,
            post_release_delay: None,
        }
    }

    /// Validate that a hold duration sits within the configured range.
    pub fn allows_hold(&self, hold_for: Duration) -> bool {
        if let Some(min) = self.min_hold
            && hold_for < min
        {
            return false;
        }
        if let Some(max) = self.max_hold
            && hold_for > max
        {
            return false;
        }
        true
    }

    /// Converts the minimum hold constraint to a [`Duration`], if present.
    pub fn min_hold_duration(&self) -> Option<Duration> {
        self.min_hold
    }

    /// Converts the maximum hold constraint to a [`Duration`], if present.
    pub fn max_hold_duration(&self) -> Option<Duration> {
        self.max_hold
    }
}

/// How a strap step reports completion back to the orchestrator.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StepCompletion {
    AfterDuration,
    OnBridgeActivity,
    OnEvent(TelemetryEventKind),
}

/// Ordered operation the orchestrator applies to a strap line.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct StrapStep {
    pub line: StrapId,
    pub action: StrapAction,
    pub hold_for: Duration,
    pub constraints: TimingConstraintSet,
    pub completion: StepCompletion,
}

impl StrapStep {
    pub const fn new(
        line: StrapId,
        action: StrapAction,
        hold_for: Duration,
        constraints: TimingConstraintSet,
        completion: StepCompletion,
    ) -> Self {
        Self {
            line,
            action,
            hold_for,
            constraints,
            completion,
        }
    }

    /// Returns the strap metadata associated with this step.
    pub fn strap(&self) -> StrapLine {
        strap_by_id(self.line)
    }

    /// Returns the hold duration as a [`Duration`].
    pub fn hold_duration(&self) -> Duration {
        self.hold_for
    }
}

/// The type of sequence described by a template.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StrapSequenceKind {
    NormalReboot,
    RecoveryEntry,
    RecoveryImmediate,
    FaultRecovery,
}

/// Immutable strap sequence template shared across targets.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SequenceTemplate {
    pub kind: StrapSequenceKind,
    pub phases: &'static [StrapStep],
    pub cooldown: Duration,
    pub max_retries: Option<u8>,
}

impl SequenceTemplate {
    pub const fn new(
        kind: StrapSequenceKind,
        phases: &'static [StrapStep],
        cooldown: Duration,
        max_retries: Option<u8>,
    ) -> Self {
        Self {
            kind,
            phases,
            cooldown,
            max_retries,
        }
    }

    /// Returns the ordered strap steps that make up the sequence.
    pub const fn steps(&self) -> &'static [StrapStep] {
        self.phases
    }

    /// Returns the number of steps contained in the template.
    pub fn step_count(&self) -> usize {
        self.phases.len()
    }

    /// Returns the cooldown interval as a [`Duration`].
    pub fn cooldown_duration(&self) -> Duration {
        self.cooldown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strap_lookup_returns_expected_metadata() {
        let reset = strap_by_id(StrapId::Reset);
        assert_eq!(reset.name, "RESET*");
        assert_eq!(reset.mcu_pin, "PA4");
        assert_eq!(reset.driver_output, "SN74LVC07-2Y");
        assert_eq!(reset.j14_pin, 8);
        assert_eq!(reset.polarity, StrapPolarity::ActiveLow);
    }

    #[test]
    fn timing_constraints_allow_expected_ranges() {
        let constraints = TimingConstraintSet::with_hold_range(
            Some(Duration::from_millis(100)),
            Some(Duration::from_millis(250)),
        );
        assert!(constraints.allows_hold(Duration::from_millis(150)));
        assert!(!constraints.allows_hold(Duration::from_millis(50)));
        assert!(!constraints.allows_hold(Duration::from_millis(300)));
    }

    #[test]
    fn sequence_template_reports_steps_and_cooldown() {
        const STEPS: [StrapStep; 2] = [
            StrapStep::new(
                StrapId::Pwr,
                StrapAction::AssertLow,
                Duration::from_millis(200),
                TimingConstraintSet::unrestricted(),
                StepCompletion::AfterDuration,
            ),
            StrapStep::new(
                StrapId::Reset,
                StrapAction::ReleaseHigh,
                Duration::ZERO,
                TimingConstraintSet::unrestricted(),
                StepCompletion::AfterDuration,
            ),
        ];
        const TEMPLATE: SequenceTemplate = SequenceTemplate::new(
            StrapSequenceKind::NormalReboot,
            &STEPS,
            Duration::from_millis(1_000),
            Some(3),
        );

        assert_eq!(TEMPLATE.kind, StrapSequenceKind::NormalReboot);
        assert_eq!(TEMPLATE.step_count(), 2);
        assert_eq!(TEMPLATE.steps()[0].strap().name, "PWR*");
        assert_eq!(TEMPLATE.steps()[1].line, StrapId::Reset);
        assert_eq!(TEMPLATE.cooldown_duration(), Duration::from_millis(1_000),);
        assert_eq!(TEMPLATE.max_retries, Some(3));
    }
}
