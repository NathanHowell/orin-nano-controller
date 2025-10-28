//! VREFINT sampling helpers for the STM32G0 brown-out monitor.
//!
//! This module wires the Embassy ADC driver into the shared power monitor
//! abstractions owned by `controller-core`. The helper exposes a simple
//! [`VrefintSampleProvider`] implementation that performs calibrated reads of
//! the internal voltage reference so the firmware can classify the 3.3 V rail.

#![cfg(target_os = "none")]
#![allow(dead_code)]

use core::ptr;

use controller_core::orchestrator::{VrefintSample, VrefintSampleProvider};
use embassy_stm32::adc::{Adc, SampleTime, VrefInt};
use embassy_stm32::peripherals::ADC1;
use embassy_time::Instant as EmbassyInstant;

use crate::straps::FirmwareInstant;

/// Factory-programmed calibration constant sampled at 3.0 V.
const VREFINT_CAL_ADDR: *const u16 = 0x1FFF_75AA as *const u16;

/// Reads the factory-trimmed VREFINT calibration constant.
pub fn read_vrefint_calibration() -> u16 {
    unsafe { ptr::read_volatile(VREFINT_CAL_ADDR) }
}

/// Embassy ADC wrapper that produces successive VREFINT samples.
pub struct VrefintAdc<'d> {
    adc: Adc<'d, ADC1>,
    channel: VrefInt,
    discard_next: bool,
}

impl<'d> VrefintAdc<'d> {
    /// Constructs a new helper and enables the internal voltage reference.
    pub fn new(mut adc: Adc<'d, ADC1>) -> Self {
        adc.set_sample_time(SampleTime::CYCLES160_5);
        let channel = adc.enable_vrefint();
        Self {
            adc,
            channel,
            discard_next: true,
        }
    }

    fn read_once(&mut self) -> u16 {
        self.adc.blocking_read(&mut self.channel)
    }
}

impl<'d> VrefintSampleProvider for VrefintAdc<'d> {
    type Instant = FirmwareInstant;

    fn next_sample(&mut self) -> Option<VrefintSample<Self::Instant>> {
        if self.discard_next {
            let _ = self.read_once();
            self.discard_next = false;
        }

        let reading = self.read_once();
        let timestamp = FirmwareInstant::from(EmbassyInstant::now());
        Some(VrefintSample::new(timestamp, reading))
    }
}
