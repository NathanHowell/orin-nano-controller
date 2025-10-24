# Feature Specification: Orin Controller Hardware Platform

**Feature Branch**: 001-build-orin-controller  
**Created**: 2025-10-23  
**Status**: Draft  
**Input**: User description: "we are building a hardware device to control the Nvidia Jetson Orin"

## Hardware Interface Contracts *(mandatory)*

### Pin & Signal Map

- J14 strap outputs to Jetson carrier (reference `pcb/orin-nano-controller/README.md`, "Strap Driver (SN74LVC07A implementation)")  
  - J14-8 `RESET*` (open-drain) ← net `STRAP_RESET_L` (Y1); 0–3.3 V, sink ≤ 24 mA; drives Jetson reset.  
  - J14-10 `REC*` (open-drain) ← net `STRAP_RECOVERY_L` (Y2); 0–3.3 V; selects flashing/recovery mode.  
  - J14-12 `PWR*` (open-drain) ← net `STRAP_POWER_BTN_L` (Y3); 0–3.3 V; emulates front-panel power button.  
  - J14-5 `APO` (open-drain) ← net `STRAP_APO_L` (Y4); 0–3.3 V; forces Jetson shutdown when asserted.  
- USB-C receptacle J1 provides 5 V bus input and USB FS data (see "USB & Power Passives"); `VBUS` must remain within 4.75–5.25 V, D+/D− idle until Jetson drives host negotiation.  
- USB-C device firmware enumerates as a composite USB CDC ACM device exposing **two** virtual COM ports:  
  - `CDC0` → Interactive REPL for strap commands and diagnostics.  
  - `CDC1` → Transparent UART bridge to the Jetson console.  
  Both present as standard ACM interfaces; no HID or vendor-specific endpoints are exposed.  
- SWD header J3 exposes MCU programming pins (`SWDIO`, `SWCLK`, `NRST`, `3V3`, `GND`) consistent with `pcb/orin-nano-controller/README.md` "Test Points".  
- TLV75533PDBV LDO (U2) supplies regulated 3.3 V (net `VDD_3V3`) to MCU, strap buffer U3, and sense circuitry; total continuous load must stay below 500 mA with 20% headroom.  
- PC_LED+ divider (R_LED_HI/R_LED_LO) feeds MCU ADC channel `PC_LED_MON`, providing 0–3.3 V scaled telemetry for Jetson state indication.

### Timing & Voltage Windows

- Hold `RESET*` low for ≥20 ms during power-up; release no sooner than 10 ms after `VDD_3V3` exceeds 2.97 V; ±1 ms tolerance verified via logic analyzer.  
- For recovery entry, assert `REC*` low ≥100 ms before releasing `RESET*`, maintain low for 500 ms post-release, then return high.  
- Simulate power-button presses by driving `PWR*` low for 200 ms ±20 ms, enforcing a minimum 1 s cool-down before subsequent presses.  
- Prevent active driving of USB D+/D−; controller-side transceivers remain high-impedance until Jetson initiates host negotiation.

### Bench Validation & Evidence

- Capture logic analyzer traces (minimum 8 channels) covering `VDD_3V3`, `RESET*`, `REC*`, `PWR*`, and `APO`; store `.sal` files under `specs/001-build-orin-controller/evidence/`.  
- Measure 3.3 V rail ripple under strap activity with an oscilloscope, saving annotated screenshots demonstrating <50 mVpp ripple.  
- Document SWD reflashing procedure with step-by-step photos or screenshots that confirm MCU recovery path.  
- Record USB enumeration logs from host PC confirming Jetson detection after controller-driven sequences; archive text exports alongside trace data.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Lab operator reboots Jetson safely (Priority: P1)

A lab operator needs to reboot the Jetson Orin into normal boot mode without unplugging cables.

**Why this priority**: Enables daily development workflows; without it, firmware testing stalls.

**Independent Test**: Command the controller to execute the normal reboot sequence and observe Jetson console for successful boot banner.

**Acceptance Scenarios**:

1. **Given** the controller is connected to J14 and powered, **When** the operator triggers a normal reboot command, **Then** the controller cycles `PWR*` and `RESET*` within defined timings and the Jetson boots to the Linux prompt.  
2. **Given** a normal reboot command has been issued, **When** the sequence completes, **Then** observability records show timestamps for each strap transition via the live defmt telemetry stream.

---

### User Story 2 - Engineer enters recovery mode for flashing (Priority: P2)

An embedded engineer must place the Jetson in USB recovery mode to flash new firmware.

**Why this priority**: Recovery flashing is required for low-level updates and must be predictable.

**Independent Test**: Invoke the recovery command and verify the Jetson enumerates as an Nvidia APX device on USB within 10 seconds.

**Acceptance Scenarios**:

1. **Given** the controller is idle, **When** the engineer selects recovery mode, **Then** `REC*` remains asserted throughout reset and the Jetson appears in recovery on the host PC.  
2. **Given** recovery mode has been entered, **When** validation evidence is captured, **Then** logic traces confirm adherence to the 100 ms pre-reset hold.

---

### User Story 3 - Field technician performs fault recovery (Priority: P3)

A field technician must hard-stop a wedged Jetson and restore normal operation using the controller.

**Why this priority**: Ensures remote serviceability without manual cable access.

**Independent Test**: Simulate a fault by keeping the Jetson unresponsive, execute the fault recovery sequence (APO assert plus reboot), and confirm system returns to normal boot.

**Acceptance Scenarios**:

1. **Given** the Jetson is unresponsive, **When** the technician invokes the fault recovery workflow, **Then** `APO` asserts, power cycles, and the Jetson restarts within 90 seconds.  
2. **Given** fault recovery completes, **When** diagnostics logs are reviewed, **Then** they include the reason code and evidence of SWD availability.

---

### Edge Cases

- Jetson supply brown-out causing `VDD_3V3` to dip below threshold mid-sequence; controller must retry after rails stabilize.  
- USB cable disconnect during recovery entry; system must surface an error and hold Jetson in a safe state.  
- Conflicting host requests (e.g., repeated recovery commands during an active sequence); controller must serialize execution so additional commands queue until the active sequence finishes and emit defmt telemetry marking the queued command as pending and noting when it begins.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Controller MUST provide selectable sequences for normal reboot, recovery entry, and fault recovery, each aligning with defined timing windows.  
- **FR-002**: *Retired requirement placeholder.* No additional behavior; retained for historical traceability after deprecating on-device storage telemetry.
- **FR-003**: Operators MUST trigger sequences via the directly connected maintenance host over the USB-C CDC0 REPL; the firmware MUST NOT expose alternate command transports.  
- **FR-004**: Controller MUST emit timestamped telemetry for each strap transition via defmt logging so operators can capture evidence live.
- **FR-005**: System MUST prevent conflicting strap states by enforcing serialized execution; while a sequence runs, additional requests queue until the active sequence completes, and the controller MUST surface telemetry indicating queued requests and their eventual start times.
- **FR-006**: The REPL MUST expose a `recovery now` command that (a) asserts the RECOVERY strap, (b) reboots the Jetson, and (c) holds RECOVERY asserted until Jetson UART activity is detected on the bridge CDC port, after which the strap is released automatically.

### Boot & Strap Sequence Requirements *(mandatory when behavior changes)*

- **BS-001**: Normal reboot sequence MUST assert `PWR*` low for 200 ms, wait 1 s, assert `RESET*` low for 20 ms, and release both while keeping `REC*` high.  
- **BS-002**: Recovery sequence MUST assert `REC*` low 100 ms before `RESET*`, maintain it for 500 ms post-release, and block normal reboot until `REC*` returns high.  
- **BS-003**: Fault recovery sequence MUST drive `APO` low for 250 ms before performing the normal reboot sequence and log any retries up to three attempts.

### Observability & Recovery Requirements *(mandatory)*

- **OR-001**: Provide documented SWD reflashing steps, including jumper or strap requirements, enabling MCU recovery without Jetson disassembly.  
- **OR-002**: Derive per-run timing validation results from the live telemetry streams and capture them in host-accessible summaries so reviewers can confirm strap compliance without replaying raw logs.  
- **OR-003**: Archive all bench evidence (logic traces, scope captures, USB logs) in `specs/001-build-orin-controller/evidence/` with metadata linking to test cases.

### Non-Functional Requirements

- **NFR-001**: The release-mode firmware image MUST fit within the STM32G0B1KETx limits of 512 kB flash and 144 kB RAM; configurations achieving ≤64 kB flash usage SHOULD be prioritized to preserve headroom for potential lower-cost pin-compatible parts.

### Key Entities *(include if feature involves data)*

- **StrapControl Module**: Coordinates open-drain outputs for `RESET*`, `REC*`, `PWR*`, and `APO`, enforcing timing budgets and sequencing rules.  
- **EvidenceLogger**: Streams telemetry events via defmt logging only, tagging each command with timestamps so the host tooling can capture evidence live without on-device storage.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 95% of normal reboot commands complete with Jetson boot banner observed within 60 seconds.  
- **SC-002**: Recovery mode entry succeeds in 10/10 bench trials, with host detecting the Jetson in recovery within 8 seconds.  
- **SC-004**: Telemetry evidence (logic trace plus live log output) is emitted for 100% of executed sequences and visible to the host during the active session.

## Assumptions & Dependencies

- Jetson Orin carrier exposes the 12-pin control header compatible with the J14 pinout described in `pcb/orin-nano-controller/README.md`.  
- Host control software will be delivered separately and can issue high-level sequence commands to the controller.  
- Lab equipment (logic analyzer, oscilloscope, USB sniffing tools) is available for validation and ongoing diagnostics.  
- Power input to the controller is a regulated 5 V source capable of supplying at least 1 A headroom for transients.

## Clarifications

### Session 2025-10-23

- Q: What command access policy should the controller enforce for host-triggered sequences? → A: Accept commands only from the directly connected host via wired link; reject remote clients.
- Q: What host communication interface should the controller expose for sequence commands? → A: USB-C CDC virtual COM port.
- Q: Which physical control interface should the hardware expose for triggering sequences? → A: No buttons; host control only.
