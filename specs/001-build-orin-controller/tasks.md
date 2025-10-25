---
description: "Task list template for feature implementation"
---

# Tasks: Build Orin Controller Firmware

**Input**: Design documents from `/specs/001-build-orin-controller/`
**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/, quickstart.md
**Validation**: Capture hardware strap, power, and USB evidence on the bench and mirror each REPL workflow via the `emulator` crate with archived host transcripts and parity logs.
**Organization**: Tasks are grouped by user story so each story can be implemented and tested independently.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no blocking dependencies)
- **[Story]**: Maps tasks to user stories (US1, US2, US3)
- Always include exact file paths in descriptions

## Path Conventions

- Shared logic/API: `controller-core/src/`, `controller-core/Cargo.toml`
- Firmware target: `firmware/src/`, `firmware/Cargo.toml`, `firmware/.cargo/`
- Host emulator: `emulator/src/`, `emulator/Cargo.toml`
- Bench evidence: `specs/001-build-orin-controller/evidence/`
- Hardware docs: `pcb/orin-nano-controller/README.md`
- Feature documentation: `specs/001-build-orin-controller/*.md`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Align documentation, tooling, and bench instrumentation before restructuring the workspace.

- [X] T001 Update Constitution gate answers for the controller-core/firmware/emulator split in `specs/001-build-orin-controller/plan.md`.
- [ ] T002 [P] Record bench instrumentation readiness (logic analyzer, oscilloscope, SWD probe) in `specs/001-build-orin-controller/evidence/bench-setup.md`.
- [ ] T003 [P] Confirm the Rust toolchain pin (`1.90.0`) in `rust-toolchain.toml` matches the plan requirements.
- [ ] T004 [P] Verify the default `thumbv6m-none-eabi` target configuration in `firmware/.cargo/config.toml` and note any required host setup steps.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Establish the shared controller-core crate, workspace wiring, and crate scaffolding required by every user story.

- [ ] T005 Create the `controller-core` crate skeleton with `#![no_std]` defaults in `controller-core/Cargo.toml` and `controller-core/src/lib.rs`.
- [ ] T006 [P] Implement strap data structures (`StrapLine`, `SequenceTemplate`, `StrapStep`) per the data model in `controller-core/src/sequences/mod.rs`.
- [ ] T007 [P] Define the command queue and `SequenceRun` state machine traits in `controller-core/src/orchestrator/mod.rs`.
- [ ] T008 [P] Implement REPL tokenization and grammar parsing using `logos` + `winnow` in `controller-core/src/repl/grammar.rs`.
- [ ] T009 [P] Publish telemetry event enums and payload structures in `controller-core/src/telemetry/mod.rs`.
- [ ] T010 [P] Register `controller-core` and `emulator` in the workspace manifest and share dependencies in the root `Cargo.toml`.
- [ ] T011 [P] Add the `controller-core` dependency plus trait adapter scaffolding in `firmware/Cargo.toml` and `firmware/src/hw/mod.rs`.
- [ ] T012 [P] Scaffold the `emulator` crate with a host REPL entry point in `emulator/Cargo.toml` and `emulator/src/main.rs`.
- [ ] T013 [P] Add smoke tests that compile `controller-core` for host and `thumbv6m` targets in `controller-core/tests/orchestrator.rs`.

---

## Phase 3: User Story 1 - Lab operator reboots Jetson safely (Priority: P1) 🎯 MVP

**Goal**: Deliver a shared `NormalReboot` sequence accessible from both firmware and emulator REPLs with telemetry proof of strap timing.

**Independent Test**: Issue `reboot now` via firmware and emulator REPLs, confirm Jetson boots to the Linux prompt, and verify telemetry timestamps in captured evidence.

### Tasks

- [ ] T014 [US1] Implement the `NormalReboot` sequence template with timing budgets in `controller-core/src/sequences/normal.rs`.
- [ ] T015 [P] [US1] Wire `NormalReboot` into the orchestrator queue and enforce cooldown logic in `controller-core/src/orchestrator/mod.rs`.
- [ ] T016 [P] [US1] Implement `reboot now|delay` parsing and command dispatch in `controller-core/src/repl/commands.rs`.
- [ ] T017 [P] [US1] Add host-side unit tests covering `NormalReboot` durations in `controller-core/tests/normal_reboot.rs`.
- [ ] T018 [P] [US1] Bind controller-core strap operations to STM32 pins with defmt telemetry in `firmware/src/straps/orchestrator.rs`.
- [ ] T019 [P] [US1] Expose a reboot command in the emulator session and log parity transcripts to `emulator/src/session.rs` and `specs/001-build-orin-controller/evidence/emulator-reboot.log`.
- [ ] T020 [US1] Capture the normal reboot logic analyzer trace at `specs/001-build-orin-controller/evidence/normal-reboot.sal`.
- [ ] T021 [US1] Record queue serialization defmt output for overlapping reboot commands in `specs/001-build-orin-controller/evidence/queue-serialization.log`.
- [ ] T022 [US1] Update reboot timing and telemetry references in `specs/001-build-orin-controller/spec.md`.
- [ ] T023 [US1] Document combined hardware and emulator reboot usage in `specs/001-build-orin-controller/quickstart.md`.

---

## Phase 4: User Story 2 - Engineer enters recovery mode for flashing (Priority: P2)

**Goal**: Provide recovery sequences that assert REC appropriately, release on console activity, and surface identical behavior through firmware and emulator REPLs.

**Independent Test**: Invoke `recovery now` and `recovery enter|exit`, confirm Jetson enumerates in recovery on USB within 10 seconds, and verify parity logs between hardware and emulator runs.

### Tasks

- [ ] T024 [US2] Implement `RecoveryEntry` and `RecoveryImmediate` templates with pre/post REC windows in `controller-core/src/sequences/recovery.rs`.
- [ ] T025 [P] [US2] Extend orchestrator logic to hold REC until bridge activity or timeout in `controller-core/src/orchestrator/mod.rs`.
- [ ] T026 [P] [US2] Publish bridge activity telemetry hooks driving REC release in `firmware/src/bridge/mod.rs`.
- [ ] T027 [P] [US2] Implement `recovery enter|exit|now` parsing with grammar validation in `controller-core/src/repl/commands.rs`.
- [ ] T028 [P] [US2] Mirror recovery workflows in the emulator and archive transcripts in `emulator/src/session.rs` and `specs/001-build-orin-controller/evidence/emulator-recovery.log`.
- [ ] T029 [US2] Capture recovery strap trace and USB host log at `specs/001-build-orin-controller/evidence/recovery-entry.sal` and `specs/001-build-orin-controller/evidence/recovery-usb.log`.
- [ ] T030 [US2] Update recovery validation notes and evidence links in `specs/001-build-orin-controller/spec.md`.
- [ ] T031 [P] [US2] Add host tests covering REC hold/release behavior in `controller-core/tests/recovery.rs`.

---

## Phase 5: User Story 3 - Field technician performs fault recovery (Priority: P3)

**Goal**: Deliver an APO-driven fault recovery workflow with bounded retries and documented field-service procedures across firmware and emulator.

**Independent Test**: Execute `fault recover` with retries configured, confirm APO assertion plus reboot retries on hardware, and verify emulator parity logs capture matching telemetry.

### Tasks

- [ ] T032 [US3] Define the `FaultRecovery` sequence with APO pre-hold and retry budget in `controller-core/src/sequences/fault.rs`.
- [ ] T033 [P] [US3] Extend telemetry to log fault recovery reason codes and retry counts in `controller-core/src/telemetry/mod.rs`.
- [ ] T034 [P] [US3] Implement `fault recover retries=` parsing and command dispatch in `controller-core/src/repl/commands.rs`.
- [ ] T035 [P] [US3] Integrate APO control and retry loop handling in `firmware/src/straps/orchestrator.rs`.
- [ ] T036 [P] [US3] Add emulator fault recovery parity logging to `emulator/src/session.rs` and `specs/001-build-orin-controller/evidence/emulator-fault.log`.
- [ ] T037 [US3] Capture fault recovery strap trace and SWD checklist at `specs/001-build-orin-controller/evidence/fault-recovery.sal` and `specs/001-build-orin-controller/evidence/fault-recovery-notes.md`.
- [ ] T038 [US3] Document field recovery workflow and SWD fallback steps in `specs/001-build-orin-controller/quickstart.md` and `specs/001-build-orin-controller/spec.md`.
- [ ] T039 [P] [US3] Add retry exhaustion tests for fault recovery in `controller-core/tests/fault.rs`.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Finalize diagnostics, evidence, and cross-story validation before handoff.

- [ ] T040 Run `cargo clippy --all-targets` from the workspace `Cargo.toml` and resolve lints across controller-core, firmware, and emulator crates.
- [ ] T041 [P] Summarize collected artifacts in `specs/001-build-orin-controller/evidence/README.md`.
- [ ] T042 [P] Capture brown-out retry defmt logs in `specs/001-build-orin-controller/evidence/brownout-retry.log`.
- [ ] T043 [P] Capture USB disconnect safe-state behavior in `specs/001-build-orin-controller/evidence/usb-disconnect.log`.
- [ ] T044 [P] Record SC-001 and SC-002 integration results in `specs/001-build-orin-controller/evidence/integration-results.md`.
- [ ] T045 [P] Document SC-003 and SC-004 telemetry verification in `specs/001-build-orin-controller/evidence/integration-results.md`.
- [ ] T046 [P] Archive VDD_3V3 ripple screenshots under `specs/001-build-orin-controller/evidence/vdd33-ripple.png`.
- [ ] T047 [P] Record release binary footprint metrics in `specs/001-build-orin-controller/evidence/binary-footprint.md`.

---

## Dependencies & Execution Order

- Phase sequencing: Phase 1 (T001–T004) → Phase 2 (T005–T013) → Phase 3 (US1) → Phase 4 (US2) → Phase 5 (US3) → Phase 6 (T040–T047).
- User story prerequisites: US1 (T014–T023) depends on foundational tasks; US2 (T024–T031) requires completion of T006–T013 and queue telemetry from US1; US3 (T032–T039) depends on foundational work plus REC telemetry patterns from US2.
- Evidence tasks (T020, T029, T037, T042–T046) require corresponding implementation tasks to be merged first.
- Emulator parity logging (T019, T028, T036) depends on the emulator scaffold (T012) and relevant controller-core commands (T016, T027, T034).

---

## Parallel Execution Examples

- **US1**: After T014 lands, run T016 and T017 in parallel while firmware integration (T018) proceeds, then capture evidence (T020–T021) once parity logging (T019) is ready.
- **US2**: Develop bridge telemetry (T026) alongside REPL command parsing (T027) after sequences (T024) are ready, leaving emulator parity (T028) to run once firmware signaling stabilizes.
- **US3**: Implement telemetry updates (T033) and REPL parsing (T034) in parallel after the FaultRecovery template (T032), while firmware integration (T035) prepares for evidence capture (T037).

---

## Implementation Strategy

Follow the plan in `specs/001-build-orin-controller/plan.md`:

1. Stand up the shared `controller-core` abstractions (Phase 2).
2. Bind `controller-core` to the firmware hardware layer and prove emulator parity (Phases 3–5 incrementally).
3. Capture bench evidence and telemetry parity for each story before progressing to the next.
4. Close with cross-cutting validation, success criteria measurements, and documentation polish (Phase 6).

