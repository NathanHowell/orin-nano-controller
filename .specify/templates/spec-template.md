# Feature Specification: [FEATURE NAME]

**Feature Branch**: `[###-feature-name]`  
**Created**: [DATE]  
**Status**: Draft  
**Input**: User description: "$ARGUMENTS"

## Hardware Interface Contracts *(mandatory)*

### Pin & Signal Map

- [List each impacted connector/pin with direction, voltage level, and the matching KiCad net name]
- [Reference the relevant entries in `pcb/orin-nano-controller/BASELINE.md` or schematic sheets]

### Timing & Voltage Windows

- [Document required delays, sequencing, or ramp rates for straps, resets, USB attach, or power rails]
- [State acceptable tolerances and measurement expectations]

### Bench Validation & Evidence

- [Describe planned hardware-in-the-loop or bench validation, instruments, and capture artifacts]
- [Outline how evidence (logic traces, measurements, photos) will be stored alongside this spec]

## User Scenarios & Testing *(mandatory)*

<!--
  IMPORTANT: User stories should be PRIORITIZED as user journeys ordered by importance.
  Each user story/journey must be INDEPENDENTLY TESTABLE - meaning if you implement just ONE of them,
  you should still have a viable MVP (Minimum Viable Product) that delivers value.
  
  Assign priorities (P1, P2, P3, etc.) to each story, where P1 is the most critical.
  Think of each story as a standalone slice of functionality that can be:
  - Developed independently
  - Tested independently
  - Deployed independently
  - Demonstrated to users independently
-->

### User Story 1 - [Brief Title] (Priority: P1)

[Describe this user journey in plain language]

**Why this priority**: [Explain the value and why it has this priority level]

**Independent Test**: [Describe how this can be tested independently - e.g., "Can be fully tested by [specific action] and delivers [specific value]"]

**Acceptance Scenarios**:

1. **Given** [initial state], **When** [action], **Then** [expected outcome]
2. **Given** [initial state], **When** [action], **Then** [expected outcome]

---

### User Story 2 - [Brief Title] (Priority: P2)

[Describe this user journey in plain language]

**Why this priority**: [Explain the value and why it has this priority level]

**Independent Test**: [Describe how this can be tested independently]

**Acceptance Scenarios**:

1. **Given** [initial state], **When** [action], **Then** [expected outcome]

---

### User Story 3 - [Brief Title] (Priority: P3)

[Describe this user journey in plain language]

**Why this priority**: [Explain the value and why it has this priority level]

**Independent Test**: [Describe how this can be tested independently]

**Acceptance Scenarios**:

1. **Given** [initial state], **When** [action], **Then** [expected outcome]

---

[Add more user stories as needed, each with an assigned priority]

### Edge Cases

<!--
  ACTION REQUIRED: The content in this section represents placeholders.
  Fill them out with the right edge cases.
-->

- What happens when [boundary condition]?
- How does system handle [error scenario]?

## Requirements *(mandatory)*

<!--
  ACTION REQUIRED: The content in this section represents placeholders.
  Fill them out with the right functional requirements.
-->

### Functional Requirements

- **FR-001**: Firmware MUST [drive specific straps/IOs] according to the documented state machine.
- **FR-002**: System MUST [enforce timing/voltage constraint], matching the Hardware Interface Contracts section.  
- **FR-003**: Operators MUST be able to [trigger boot/recovery mode] via [button/command/interface].
- **FR-004**: Firmware MUST [publish diagnostics or telemetry] through [Defmt/RTT/SWO/diagnostic GPIO].
- **FR-005**: System MUST [protect Jetson/board] by [specific safeguard].
- **FR-006**: Shared logic MUST reside in `controller-core`, compile for both firmware and host targets, and expose APIs consumed by `firmware` and `emulator` crates.
- **FR-007**: The `emulator` crate MUST surface the REPL commands that exercise the new behavior and record how parity with hardware will be validated.

*Example of marking unclear requirements:*

- **FR-008**: Firmware MUST manage strap timing with [NEEDS CLARIFICATION: exact delay window not specified]
- **FR-009**: System MUST expose diagnostics through [NEEDS CLARIFICATION: instrumentation method undecided]

### Boot & Strap Sequence Requirements *(mandatory when behavior changes)*

- **BS-001**: Sequence MUST [e.g., hold RESET low for X ms, wait Y ms, assert strap].
- **BS-002**: Sequence MUST abort if [fault condition] occurs and [recovery step].
- **BS-003**: Sequence MUST log [event/timestamp] through [instrumentation path].

### Observability & Recovery Requirements *(mandatory)*

- **OR-001**: Provide a recovery path via [SWD flashing, strap override, etc.] and document operator steps.
- **OR-002**: Emit diagnostics for [specific error] through [Defmt/SWO/LED pattern].
- **OR-003**: Record where evidence (logic traces, photos) will be stored after validation.

### Key Entities *(include if feature involves data)*

- **[Entity 1]**: [Hardware abstraction module or driver, e.g., StrapController, including pins and constraints]
- **[Entity 2]**: [Supporting component, e.g., UsbPowerDomain, including interactions and dependencies]

## Success Criteria *(mandatory)*

<!--
  ACTION REQUIRED: Define measurable success criteria.
  These must be technology-agnostic and measurable.
-->

### Measurable Outcomes

- **SC-001**: [Measurable metric, e.g., "Jetson enters recovery mode within 3 s of command with 100% repeatability"]
- **SC-002**: [Measurable metric, e.g., "USB enumerates successfully in 10/10 bench trials after strap change"]
- **SC-003**: [Quality metric, e.g., "No strap timing violation observed across ±10% supply variation"]
- **SC-004**: [Operational metric, e.g., "Diagnostics capture identifies failure in under 5 minutes of bench time"]
