//! Shared status surface for the REPL.
//!
//! The firmware and emulator implement [`StatusProvider`] so that the REPL
//! can surface live strap, power, and debugger state through the `status`
//! command without duplicating platform logic. [`StatusFormatter`] keeps the
//! textual rendering consistent across front-ends.

use core::fmt;
use core::time::Duration;

use crate::sequences::{strap_by_id, StrapId};

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
