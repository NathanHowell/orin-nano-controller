//! Strap sequence data structures shared by firmware and host targets.
//!
//! The orchestrator uses these definitions to drive asynchronous strap
//! state machines without embedding any MCU-specific knowledge. Everything
//! in this module is `no_std` friendly so the same data can be compiled for
//! both the STM32 firmware and the host-side emulator.

use core::time::Duration;

/// Longest sequence we expect to encode (FaultRecovery) plus one step of headroom.
pub const MAX_SEQUENCE_STEPS: usize = 8;

/// Convenience wrapper that keeps millisecond values const-friendly while still
/// allowing callers to convert to [`Duration`] when needed.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Milliseconds(u32);

impl Milliseconds {
    pub const ZERO: Self = Self::new(0);

    /// Creates a new millisecond value.
    pub const fn new(ms: u32) -> Self {
        Self(ms)
    }

    /// Returns the raw millisecond count.
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    /// Converts the value to a [`Duration`].
    pub fn as_duration(self) -> Duration {
        self.into()
    }
}

impl From<u32> for Milliseconds {
    fn from(value: u32) -> Self {
        Self::new(value)
    }
}

impl From<Milliseconds> for Duration {
    fn from(value: Milliseconds) -> Self {
        Duration::from_millis(value.0 as u64)
    }
}

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
    pub min_hold: Option<Milliseconds>,
    pub max_hold: Option<Milliseconds>,
    pub pre_assert_delay: Option<Milliseconds>,
    pub post_release_delay: Option<Milliseconds>,
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
    pub const fn with_hold_range(
        min_hold: Option<Milliseconds>,
        max_hold: Option<Milliseconds>,
    ) -> Self {
        Self {
            min_hold,
            max_hold,
            pre_assert_delay: None,
            post_release_delay: None,
        }
    }

    /// Validate that a hold duration sits within the configured range.
    pub fn allows_hold(&self, hold_for: Milliseconds) -> bool {
        if let Some(min) = self.min_hold {
            if hold_for < min {
                return false;
            }
        }
        if let Some(max) = self.max_hold {
            if hold_for > max {
                return false;
            }
        }
        true
    }

    /// Converts the minimum hold constraint to a [`Duration`], if present.
    pub fn min_hold_duration(&self) -> Option<Duration> {
        self.min_hold.map(Into::into)
    }

    /// Converts the maximum hold constraint to a [`Duration`], if present.
    pub fn max_hold_duration(&self) -> Option<Duration> {
        self.max_hold.map(Into::into)
    }
}

/// Identifier for telemetry events without depending on the concrete enum yet.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TelemetryEventKind(pub u16);

impl TelemetryEventKind {
    pub const fn new(code: u16) -> Self {
        Self(code)
    }
}

impl From<u16> for TelemetryEventKind {
    fn from(value: u16) -> Self {
        Self(value)
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
    pub hold_for: Milliseconds,
    pub constraints: TimingConstraintSet,
    pub completion: StepCompletion,
}

impl StrapStep {
    pub const fn new(
        line: StrapId,
        action: StrapAction,
        hold_for: Milliseconds,
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
        self.hold_for.into()
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
    pub cooldown: Milliseconds,
    pub max_retries: Option<u8>,
}

impl SequenceTemplate {
    pub const fn new(
        kind: StrapSequenceKind,
        phases: &'static [StrapStep],
        cooldown: Milliseconds,
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
        self.cooldown.into()
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
            Some(Milliseconds::new(100)),
            Some(Milliseconds::new(250)),
        );
        assert!(constraints.allows_hold(Milliseconds::new(150)));
        assert!(!constraints.allows_hold(Milliseconds::new(50)));
        assert!(!constraints.allows_hold(Milliseconds::new(300)));
    }

    #[test]
    fn sequence_template_reports_steps_and_cooldown() {
        const STEPS: [StrapStep; 2] = [
            StrapStep::new(
                StrapId::Pwr,
                StrapAction::AssertLow,
                Milliseconds::new(200),
                TimingConstraintSet::unrestricted(),
                StepCompletion::AfterDuration,
            ),
            StrapStep::new(
                StrapId::Reset,
                StrapAction::ReleaseHigh,
                Milliseconds::new(0),
                TimingConstraintSet::unrestricted(),
                StepCompletion::AfterDuration,
            ),
        ];
        const TEMPLATE: SequenceTemplate = SequenceTemplate::new(
            StrapSequenceKind::NormalReboot,
            &STEPS,
            Milliseconds::new(1_000),
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
