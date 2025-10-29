//! Shared status surface for the REPL.
//!
//! The firmware and emulator implement [`StatusProvider`] so that the REPL
//! can surface live strap, power, and debugger state through the `status`
//! command without duplicating platform logic. [`StatusFormatter`] keeps the
//! textual rendering consistent across front-ends.

use core::fmt;
use core::time::Duration;

use crate::sequences::{StrapId, strap_by_id};

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

/// Trait describing the instant arithmetic required for status tracking.
pub trait StatusInstant: Copy {
    /// Returns the elapsed time between `earlier` and `now`, if `now >= earlier`.
    fn duration_since(now: Self, earlier: Self) -> Option<Duration>;
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

/// Tracks bridge activity timestamps and waiting state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BridgeActivityTracker<Instant> {
    waiting_for_activity: bool,
    last_rx: Option<Instant>,
    last_tx: Option<Instant>,
}

impl<Instant: Copy> BridgeActivityTracker<Instant> {
    /// Creates a new tracker with no recorded activity.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            waiting_for_activity: false,
            last_rx: None,
            last_tx: None,
        }
    }

    /// Updates the waiting flag.
    pub fn set_waiting_for_activity(&mut self, waiting: bool) {
        self.waiting_for_activity = waiting;
    }

    /// Records the instant of the most recent Jetson→USB frame.
    pub fn record_rx(&mut self, instant: Instant) {
        self.last_rx = Some(instant);
    }

    /// Records the instant of the most recent USB→Jetson frame.
    pub fn record_tx(&mut self, instant: Instant) {
        self.last_tx = Some(instant);
    }

    /// Clears the tracked history.
    pub fn reset(&mut self) {
        self.waiting_for_activity = false;
        self.last_rx = None;
        self.last_tx = None;
    }

    /// Builds a snapshot of the tracked bridge activity.
    #[must_use]
    pub fn snapshot(&self, now: Instant) -> BridgeActivitySnapshot
    where
        Instant: StatusInstant,
    {
        BridgeActivitySnapshot::new(
            self.waiting_for_activity,
            self.last_tx
                .and_then(|instant| Instant::duration_since(now, instant)),
            self.last_rx
                .and_then(|instant| Instant::duration_since(now, instant)),
        )
    }
}

/// Accumulates strap, power, and bridge information for snapshotting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusAccumulator<Instant> {
    strap_mask: u8,
    vdd_mv: Option<u16>,
    control_link_attached: bool,
    debug_link: DebugLinkState,
    bridge: BridgeActivityTracker<Instant>,
}

impl<Instant: Copy> StatusAccumulator<Instant> {
    /// Creates a new accumulator with unknown readings.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            strap_mask: 0,
            vdd_mv: None,
            control_link_attached: false,
            debug_link: DebugLinkState::Unknown,
            bridge: BridgeActivityTracker::new(),
        }
    }

    /// Returns the current strap bitmask.
    #[must_use]
    pub const fn strap_mask(&self) -> u8 {
        self.strap_mask
    }

    /// Overwrites the strap bitmask.
    pub fn set_strap_mask(&mut self, mask: u8) {
        self.strap_mask = mask;
    }

    /// Updates the cached level for a single strap line.
    pub fn set_strap(&mut self, id: StrapId, asserted: bool) {
        let bit = strap_bit(id);
        if asserted {
            self.strap_mask |= bit;
        } else {
            self.strap_mask &= !bit;
        }
    }

    /// Clears all strap bits.
    pub fn reset_straps(&mut self) {
        self.strap_mask = 0;
    }

    /// Stores the latest VDD sample in millivolts.
    pub fn set_vdd_sample(&mut self, millivolts: Option<u16>) {
        self.vdd_mv = millivolts;
    }

    /// Updates the control-link attachment flag.
    pub fn set_control_link_attached(&mut self, attached: bool) {
        self.control_link_attached = attached;
    }

    /// Updates the cached debug-link state.
    pub fn set_debug_link(&mut self, state: DebugLinkState) {
        self.debug_link = state;
    }

    /// Returns a mutable handle to the bridge tracker.
    #[must_use]
    pub fn bridge_tracker(&mut self) -> &mut BridgeActivityTracker<Instant> {
        &mut self.bridge
    }

    /// Builds a [`StatusSnapshot`] from the stored readings.
    #[must_use]
    pub fn snapshot(&self, now: Instant) -> StatusSnapshot
    where
        Instant: StatusInstant,
    {
        StatusSnapshot {
            strap_levels: strap_samples_from_mask(self.strap_mask),
            vdd_mv: self.vdd_mv,
            bridge: self.bridge.snapshot(now),
            debug_link: self.debug_link,
            control_link_attached: self.control_link_attached,
        }
    }
}

impl<Instant: Copy> Default for StatusAccumulator<Instant> {
    fn default() -> Self {
        Self::new()
    }
}

/// Converts a strap bitmask into individual samples.
#[must_use]
pub fn strap_samples_from_mask(mask: u8) -> [StrapSample; 4] {
    [
        strap_sample_from_mask(mask, StrapId::Reset),
        strap_sample_from_mask(mask, StrapId::Rec),
        strap_sample_from_mask(mask, StrapId::Pwr),
        strap_sample_from_mask(mask, StrapId::Apo),
    ]
}

fn strap_sample_from_mask(mask: u8, id: StrapId) -> StrapSample {
    let asserted = mask & strap_bit(id) != 0;
    StrapSample::new(id, StrapLevel::from_asserted(asserted))
}

const fn strap_bit(id: StrapId) -> u8 {
    1 << id.as_index()
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

/// Helper that renders a [`StatusSnapshot`] into human-readable lines.
#[derive(Clone, Copy, Debug)]
pub struct StatusFormatter<'a> {
    snapshot: &'a StatusSnapshot,
}

impl<'a> StatusFormatter<'a> {
    /// Creates a new formatter for the provided snapshot.
    #[must_use]
    pub const fn new(snapshot: &'a StatusSnapshot) -> Self {
        Self { snapshot }
    }

    /// Writes the strap-state line (e.g. `straps RESET*=released ...`).
    pub fn write_straps_line<W: fmt::Write>(&self, writer: &mut W) -> fmt::Result {
        writer.write_str("straps")?;
        for sample in self.snapshot.strap_levels.iter() {
            let name = strap_by_id(sample.id).name;
            let state = if sample.level.is_asserted() {
                "asserted"
            } else {
                "released"
            };
            write!(writer, " {}={}", name, state)?;
        }
        Ok(())
    }

    /// Writes the power/debug line (e.g. `power vdd=3300mV control-link=attached debug=connected`).
    pub fn write_power_line<W: fmt::Write>(&self, writer: &mut W) -> fmt::Result {
        writer.write_str("power vdd=")?;
        match self.snapshot.vdd_mv {
            Some(mv) => write!(writer, "{}mV", mv)?,
            None => writer.write_str("unknown")?,
        }

        writer.write_str(" control-link=")?;
        writer.write_str(if self.snapshot.control_link_attached {
            "attached"
        } else {
            "lost"
        })?;

        writer.write_str(" debug=")?;
        writer.write_str(match self.snapshot.debug_link {
            DebugLinkState::Connected => "connected",
            DebugLinkState::Disconnected => "disconnected",
            DebugLinkState::Unknown => "unknown",
        })?;

        Ok(())
    }

    /// Writes the bridge line (e.g. `bridge waiting=false rx=+1.2s tx=n/a`).
    pub fn write_bridge_line<W: fmt::Write>(&self, writer: &mut W) -> fmt::Result {
        writer.write_str("bridge waiting=")?;
        writer.write_str(if self.snapshot.bridge.waiting_for_activity {
            "true"
        } else {
            "false"
        })?;

        writer.write_char(' ')?;
        writer.write_str("rx=")?;
        write_duration(writer, self.snapshot.bridge.jetson_to_usb_idle)?;

        writer.write_char(' ')?;
        writer.write_str("tx=")?;
        write_duration(writer, self.snapshot.bridge.usb_to_jetson_idle)?;

        Ok(())
    }
}

fn write_duration<W: fmt::Write>(writer: &mut W, duration: Option<Duration>) -> fmt::Result {
    match duration {
        None => writer.write_str("n/a"),
        Some(value) if value >= Duration::from_secs(1) => {
            let millis = value.as_millis() as u64;
            let seconds = millis / 1_000;
            let tenths = (millis % 1_000) / 100;
            write!(writer, "+{seconds}.{tenths}s")
        }
        Some(value) if value >= Duration::from_millis(1) => {
            write!(writer, "+{}ms", value.as_millis())
        }
        Some(value) => write!(writer, "+{}us", value.as_micros()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    struct FakeInstant(u64);

    impl StatusInstant for FakeInstant {
        fn duration_since(now: Self, earlier: Self) -> Option<Duration> {
            if now.0 >= earlier.0 {
                Some(Duration::from_micros(now.0 - earlier.0))
            } else {
                None
            }
        }
    }

    #[test]
    fn strap_mask_conversion_matches_expected_levels() {
        let mut mask = 0;
        mask |= super::strap_bit(StrapId::Reset);
        mask |= super::strap_bit(StrapId::Pwr);

        let samples = strap_samples_from_mask(mask);

        assert_eq!(samples.len(), 4);
        assert!(samples[0].level.is_asserted());
        assert!(!samples[1].level.is_asserted());
        assert!(samples[2].level.is_asserted());
        assert!(!samples[3].level.is_asserted());
    }

    #[test]
    fn accumulator_builds_snapshot() {
        let mut accumulator = StatusAccumulator::<FakeInstant>::new();
        accumulator.set_strap(StrapId::Reset, true);
        accumulator.set_vdd_sample(Some(3300));
        accumulator.set_control_link_attached(true);
        accumulator.set_debug_link(DebugLinkState::Connected);

        {
            let bridge = accumulator.bridge_tracker();
            bridge.set_waiting_for_activity(true);
            bridge.record_rx(FakeInstant(5));
            bridge.record_tx(FakeInstant(7));
        }

        let snapshot = accumulator.snapshot(FakeInstant(10));
        assert_eq!(snapshot.vdd_mv, Some(3300));
        assert!(snapshot.control_link_attached);
        assert_eq!(snapshot.debug_link, DebugLinkState::Connected);
        assert!(snapshot.strap_levels[0].level.is_asserted());
        assert_eq!(
            snapshot.bridge.jetson_to_usb_idle,
            Some(Duration::from_micros(5))
        );
        assert_eq!(
            snapshot.bridge.usb_to_jetson_idle,
            Some(Duration::from_micros(3))
        );
        assert!(snapshot.bridge.waiting_for_activity);
    }
}
