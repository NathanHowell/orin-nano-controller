#![cfg_attr(not(target_os = "none"), allow(dead_code))]

//! Shared status storage for the firmware target.
//!
//! Lightweight atomics keep track of strap assertions, bridge activity,
//! power samples, and control-link state so the REPL can surface a
//! `StatusSnapshot` without touching shared mutable state directly.

use core::time::Duration;

use controller_core::repl::status::{
    BridgeActivitySnapshot, DebugLinkState, StatusSnapshot, StrapLevel, StrapSample,
};
use controller_core::sequences::StrapId;
use portable_atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};

use crate::straps::FirmwareInstant;

const UNKNOWN_VDD: u32 = 0;

/// Bitmask describing currently asserted straps (1 == asserted).
static STRAP_MASK: AtomicU8 = AtomicU8::new(0);
/// Millivolt reading captured from VDD (0 == unknown).
static VDD_MV: AtomicU32 = AtomicU32::new(UNKNOWN_VDD);
/// Indicates whether a recovery sequence is waiting on bridge activity.
static BRIDGE_WAITING: AtomicBool = AtomicBool::new(false);
/// Timestamp (µs, +1) of the last Jetson→USB frame.
static BRIDGE_RX_MICROS: AtomicU32 = AtomicU32::new(0);
/// Timestamp (µs, +1) of the last USB→Jetson frame.
static BRIDGE_TX_MICROS: AtomicU32 = AtomicU32::new(0);
/// Tracks whether the USB control link is attached.
static CONTROL_LINK_ATTACHED: AtomicBool = AtomicBool::new(true);

fn bit_for(id: StrapId) -> u8 {
    1 << id.as_index()
}

fn encode_micros(micros: u32) -> u32 {
    micros.wrapping_add(1)
}

fn decode_micros(raw: u32) -> Option<u32> {
    if raw == 0 {
        None
    } else {
        Some(raw.wrapping_sub(1))
    }
}

fn micros_from_instant(instant: FirmwareInstant) -> u32 {
    let micros = instant.into_embassy().as_micros();
    if micros >= u32::MAX as u64 {
        u32::MAX - 1
    } else {
        micros as u32
    }
}

fn duration_since(now: FirmwareInstant, raw: u32) -> Option<Duration> {
    let stored = decode_micros(raw)?;
    let now_micros = micros_from_instant(now);
    let delta = now_micros.wrapping_sub(stored);
    Some(Duration::from_micros(u64::from(delta)))
}

fn bridge_activity(now: FirmwareInstant) -> BridgeActivitySnapshot {
    let waiting = BRIDGE_WAITING.load(Ordering::Relaxed);
    let tx_idle = duration_since(now, BRIDGE_TX_MICROS.load(Ordering::Relaxed));
    let rx_idle = duration_since(now, BRIDGE_RX_MICROS.load(Ordering::Relaxed));
    BridgeActivitySnapshot::new(waiting, tx_idle, rx_idle)
}

fn control_link_attached() -> bool {
    CONTROL_LINK_ATTACHED.load(Ordering::Relaxed)
}

/// Records the logical level for a strap line.
pub fn record_strap_asserted(id: StrapId, asserted: bool) {
    let bit = bit_for(id);
    if asserted {
        STRAP_MASK.fetch_or(bit, Ordering::Relaxed);
    } else {
        STRAP_MASK.fetch_and(!bit, Ordering::Relaxed);
    }
}

/// Clears every strap bit, marking them as released.
pub fn reset_strap_states() {
    STRAP_MASK.store(0, Ordering::Relaxed);
}

/// Returns the sampled strap states.
pub fn strap_samples() -> [StrapSample; 4] {
    let mask = STRAP_MASK.load(Ordering::Relaxed);
    [
        sample_from_mask(mask, StrapId::Reset),
        sample_from_mask(mask, StrapId::Rec),
        sample_from_mask(mask, StrapId::Pwr),
        sample_from_mask(mask, StrapId::Apo),
    ]
}

/// Stores the latest millivolt reading (0 marks unknown).
pub fn record_vdd_sample(millivolts: Option<u16>) {
    let stored = millivolts.map(u32::from).unwrap_or(UNKNOWN_VDD);
    VDD_MV.store(stored, Ordering::Relaxed);
}

/// Returns the most recent millivolt reading, if any.
pub fn vdd_sample() -> Option<u16> {
    match VDD_MV.load(Ordering::Relaxed) {
        UNKNOWN_VDD => None,
        value => Some(value as u16),
    }
}

/// Marks whether the orchestrator is waiting on bridge traffic.
pub fn record_bridge_waiting(waiting: bool) {
    BRIDGE_WAITING.store(waiting, Ordering::Relaxed);
}

/// Records the timestamp of the last Jetson→USB frame.
pub fn record_bridge_rx(timestamp: FirmwareInstant) {
    let micros = micros_from_instant(timestamp);
    BRIDGE_RX_MICROS.store(encode_micros(micros), Ordering::Relaxed);
}

/// Records the timestamp of the last USB→Jetson frame.
pub fn record_bridge_tx(timestamp: FirmwareInstant) {
    let micros = micros_from_instant(timestamp);
    BRIDGE_TX_MICROS.store(encode_micros(micros), Ordering::Relaxed);
}

/// Updates the cached control-link attachment flag.
pub fn set_control_link_attached(attached: bool) {
    CONTROL_LINK_ATTACHED.store(attached, Ordering::Relaxed);
}

/// Builds a [`StatusSnapshot`] using the stored metrics.
pub fn snapshot(now: FirmwareInstant, debug_link: DebugLinkState) -> StatusSnapshot {
    StatusSnapshot {
        strap_levels: strap_samples(),
        vdd_mv: vdd_sample(),
        bridge: bridge_activity(now),
        debug_link,
        control_link_attached: control_link_attached(),
    }
}

fn sample_from_mask(mask: u8, id: StrapId) -> StrapSample {
    let asserted = mask & bit_for(id) != 0;
    StrapSample::new(id, StrapLevel::from_asserted(asserted))
}
