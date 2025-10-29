//! Shared status surface for the REPL.
//!
//! The firmware and emulator implement [`StatusProvider`] so that the REPL
//! can surface live strap, power, and debugger state through the `status`
//! command without duplicating platform logic.

use core::time::Duration;

use crate::sequences::StrapId;

/// Logical level reported for a strap line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrapLevel {
    Asserted,
    Released,
}

impl StrapLevel {
    /// Returns `true` when the strap is currently asserted.
    #[must_use]
    pub const fn is_asserted(self) -> bool {
        matches!(self, StrapLevel::Asserted)
    }

    /// Helper converting a boolean asserted flag into a [`StrapLevel`].
    #[must_use]
    pub const fn from_asserted(asserted: bool) -> Self {
        if asserted {
            StrapLevel::Asserted
        } else {
            StrapLevel::Released
        }
    }
}

/// Sampled state for a single strap line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrapSample {
    pub id: StrapId,
    pub level: StrapLevel,
}

impl StrapSample {
    /// Creates a new strap sample.
    #[must_use]
    pub const fn new(id: StrapId, level: StrapLevel) -> Self {
        Self { id, level }
    }
}

/// Recent bridge traffic timings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BridgeActivitySnapshot {
    /// Indicates whether the orchestrator is waiting for new bridge traffic.
    pub waiting_for_activity: bool,
    /// Duration since the controller forwarded traffic to the Jetson.
    pub usb_to_jetson_idle: Option<Duration>,
    /// Duration since the controller received traffic from the Jetson.
    pub jetson_to_usb_idle: Option<Duration>,
}

impl BridgeActivitySnapshot {
    /// Creates a new bridge snapshot.
    #[must_use]
    pub const fn new(
        waiting_for_activity: bool,
        usb_to_jetson_idle: Option<Duration>,
        jetson_to_usb_idle: Option<Duration>,
    ) -> Self {
        Self {
            waiting_for_activity,
            usb_to_jetson_idle,
            jetson_to_usb_idle,
        }
    }
}

/// Debug connector attachment state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugLinkState {
    Unknown,
    Disconnected,
    Connected,
}

/// Snapshot of reusable status information surfaced by the REPL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusSnapshot {
    pub strap_levels: [StrapSample; 4],
    pub vdd_mv: Option<u16>,
    pub bridge: BridgeActivitySnapshot,
    pub debug_link: DebugLinkState,
    pub control_link_attached: bool,
}

impl StatusSnapshot {
    /// Builds a snapshot with no known measurements.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            strap_levels: [
                StrapSample::new(StrapId::Reset, StrapLevel::Released),
                StrapSample::new(StrapId::Rec, StrapLevel::Released),
                StrapSample::new(StrapId::Pwr, StrapLevel::Released),
                StrapSample::new(StrapId::Apo, StrapLevel::Released),
            ],
            vdd_mv: None,
            bridge: BridgeActivitySnapshot::new(false, None, None),
            debug_link: DebugLinkState::Unknown,
            control_link_attached: false,
        }
    }
}

/// Platform hook that supplies live status information.
pub trait StatusProvider<Instant> {
    /// Returns a snapshot if the platform can currently provide one.
    fn snapshot(&mut self, now: Instant) -> Option<StatusSnapshot>;
}

/// Placeholder status provider that never reports snapshots.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoStatusProvider;

impl<Instant> StatusProvider<Instant> for NoStatusProvider {
    fn snapshot(&mut self, _now: Instant) -> Option<StatusSnapshot> {
        None
    }
}
