# Bench Instrumentation Checklist

- **Recorded by**: Nathan Howell
- **Last verified**: 2025-10-25
- **Bench location**: Firmware lab bench 1

## Core Instruments

- [x] Logic analyzer — Saleae Logic Pro 8  
  - Channels 0–3 wired to straps (`RESET*`, `REC*`, `PWR*`, `APO`); channels 4–5 monitor `VBUS` and `3V3`; channel 0 doubles as the USB D+ trigger  
  - Firmware v2.4.13; sampling rate set to 100 MS/s for USB and strap transitions  
  - Laptop capture directory: `/Volumes/logic/sessions/orin-controller/`

- [x] Oscilloscope — Rigol DS1104Z Plus  
  - CH1 monitors `VBUS`; CH2 monitors `5V_SYS`; CH3 monitors `SWCLK`; CH4 free for probe-out  
  - Passive probes compensated 2025-10-21; scope warm-up 20 min before calibration  
  - Reference waveform `orin-controller-vbus-ref.wfm` stored on scope USB key

- [x] SWD probe — Segger J-Link  
  - Connected via 20-pin ribbon; adapter board provides 0.1" breakout to DUT  
  - Target voltage sense set to 3.3 V; NRST wired and verified with reset output  
  - Host tooling: `probe-rs 0.21.0`, `JLinkCommander 7.96a` (offline installers cached)

## Readiness Checks

- [x] Isolation transformer energized and leakage indicator nominal
- [x] ESD mat and wrist strap tested (<10 MΩ)
- [x] Dedicated USB hub powered and enumerated on host (`lsusb` shows Bus 020 Device 004)

## Notes

- Keep logic analyzer ground clip on shared star point behind DUT to avoid ground loops.
- Replace oscilloscope probe tip covers after use to preserve compensation settings.
- Quick strap sanity check: assert RESET* via firmware CLI and confirm channels 0/1 toggle (`RESET*` low, `REC*` steady high) before recording sequences.
