---
description: "Task list template for feature implementation"
---

# Tasks: Build Orin Controller Firmware

**Input**: Design documents from `/specs/001-build-orin-controller/`
**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/

**Validation**: Include hardware-in-the-loop (HIL) / bench evidence capture tasks for strap, power, and USB behaviors. Firmware tests are optional unless specified in the feature spec.

**Organization**: Tasks are grouped by user story so each story can be implemented and tested independently.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: User story label (US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

- Firmware: `firmware/src/`, `firmware/Cargo.toml`, `firmware/.cargo/`
- Bench/HIL docs & evidence: `.specify/`, `pcb/orin-nano-controller/README.md`, `specs/001-build-orin-controller/`
- Evidence captures: store under `specs/001-build-orin-controller/evidence/`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Confirm documentation, tooling, and bench instrumentation prerequisites before firmware work begins.

- [x] T001 Update strap pin cross-reference to match current harness in `pcb/orin-nano-controller/README.md`.
- [x] T002 [P] Record bench instrumentation checklist (logic analyzer, oscilloscope, SWD probe) in `specs/001-build-orin-controller/evidence/bench-setup.md`.
- [x] T003 [P] Add `thumbv6m-none-eabi` default target configuration to `firmware/.cargo/config.toml`.
- [x] T004 [P] Pin the workspace toolchain to Rust 1.90.0 in `rust-toolchain.toml`.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Establish baseline modules and async scaffolding required by all user stories. Complete before starting story-specific work.

- [x] T005 Create strap data structures (`StrapLine`, `StrapSequenceKind`, etc.) per data-model in `firmware/src/straps/mod.rs`.
- [x] T006 [P] Implement `StrapOrchestrator` skeleton with `SequenceRun` state machine and command queue stubs in `firmware/src/straps/orchestrator.rs`.
- [x] T007 [P] Set up `TelemetryRecord` ring buffer and defmt logging hooks with strap-transition timestamps and elapsed timing capture in `firmware/src/telemetry/mod.rs`.
- [x] T041 Implement queued-command telemetry markers (pending and start events) in `firmware/src/telemetry/mod.rs` and wire emission through `firmware/src/straps/orchestrator.rs` so FR-005 evidence is available for validation.
- [x] T008 [P] Define bounded USB↔UART bridge channels and activity monitor placeholders in `firmware/src/bridge/mod.rs`.
- [x] T009 [P] Add REPL session scaffolding (lexer, parser stubs, command dispatcher traits) in `firmware/src/repl/mod.rs`, bound exclusively to the CDC0 USB host link.
- [x] T010 [P] Create dual-CDC composite USB builder skeleton exposing REPL and bridge interfaces in `firmware/src/usb/mod.rs`.
- [x] T011 Wire Embassy executor init, peripheral setup, and task spawning for straps, bridge, USB, and REPL in `firmware/src/main.rs`.
- [x] T032 [P] Implement brown-out detection and strap retry handling in `firmware/src/straps/orchestrator.rs`, using ADC or voltage sense to pause sequences until VDD_3V3 stabilizes and logging the retry count via `defmt`.
- [x] T033 [P] Handle USB cable disconnect safe-state transitions by surfacing an error over the REPL, releasing straps safely, and logging the condition across `firmware/src/bridge/mod.rs` and `firmware/src/straps/orchestrator.rs`.

---

## Phase 3: User Story 1 - Lab operator reboots Jetson safely (Priority: P1) 🎯 MVP

**Goal**: Provide a REPL-triggered normal reboot sequence that honors strap timing windows for RESET and PWR lines.

**Independent Test**: Issue `reboot now` over the REPL and verify the Jetson console reaches the Linux boot banner with correct strap timing telemetry.

### Tasks

- [x] T012 [US1] Implement `NormalReboot` sequence template (timings, cooldown) in `firmware/src/straps/sequences.rs`.
- [x] T013 [P] [US1] Integrate queue-driven execution for `NormalReboot` in `firmware/src/straps/orchestrator.rs`.
- [x] T014 [P] [US1] Implement `reboot` command (with optional `delay`) that enqueues `NormalReboot` in `firmware/src/repl/commands.rs`, confirming it is reachable only via the local CDC0 REPL.
- [x] T015 [P] [US1] Emit timestamped strap telemetry and success events for normal reboot in `firmware/src/telemetry/mod.rs`.
- [ ] T039 [US1] Validate queued command serialization by issuing overlapping REPL requests and capturing defmt logs that show pending status and execution order in `specs/001-build-orin-controller/evidence/queue-serialization.log`.
- [ ] T016 [US1] Capture logic analyzer trace for normal reboot and store it as `specs/001-build-orin-controller/evidence/normal-reboot.sal`.
- [ ] T017 [US1] Document reboot timing evidence and observability notes in `specs/001-build-orin-controller/spec.md`.

---

## Phase 4: User Story 2 - Engineer enters recovery mode for flashing (Priority: P2)

**Goal**: Provide recovery commands that assert REC appropriately, hold until console activity, and expose recovery flows over the REPL.

**Independent Test**: Execute `recovery now` and confirm the Jetson enumerates as an Nvidia APX device within 10 seconds while telemetry logs REC timing.

### Tasks

- [ ] T018 [US2] Add `RecoveryEntry` and `RecoveryImmediate` templates enforcing pre/post REC windows in `firmware/src/straps/sequences.rs`.
- [ ] T019 [P] [US2] Implement REC hold/release waiting on bridge activity in `firmware/src/straps/orchestrator.rs`.
- [ ] T020 [P] [US2] Publish UART console activity events for recovery release in `firmware/src/bridge/mod.rs`.
- [ ] T021 [P] [US2] Implement `recovery enter|exit|now` command handling with grammar validation in `firmware/src/repl/commands.rs`, keeping command ingress limited to the USB REPL.
- [ ] T022 [US2] Capture recovery logic analyzer trace and USB host log at `specs/001-build-orin-controller/evidence/recovery-entry.sal` and `specs/001-build-orin-controller/evidence/recovery-usb.log`.
- [ ] T023 [US2] Update recovery validation notes, timing callouts, and evidence links in `specs/001-build-orin-controller/spec.md`.

---

## Phase 5: User Story 3 - Field technician performs fault recovery (Priority: P3)

**Goal**: Deliver a fault recovery workflow that asserts APO, retries up to three times, and restores normal boot with telemetry coverage.

**Independent Test**: Trigger `fault recover` with a simulated hang, observe APO assertion, and confirm the Jetson completes a normal boot within 90 seconds with logged retries.

### Tasks

- [ ] T024 [US3] Define `FaultRecovery` sequence with APO pre-hold and retry budget in `firmware/src/straps/sequences.rs`.
- [ ] T025 [P] [US3] Extend telemetry to log fault recovery reason codes, retry counts, and elapsed timings in `firmware/src/telemetry/mod.rs`.
- [ ] T026 [P] [US3] Implement `fault recover` command with `retries=` argument validation in `firmware/src/repl/commands.rs`.
- [ ] T027 [US3] Capture fault recovery strap trace and SWD recovery checklist in `specs/001-build-orin-controller/evidence/fault-recovery.sal` and `specs/001-build-orin-controller/evidence/fault-recovery-notes.md`.
- [ ] T028 [US3] Document field recovery procedure and SWD fallback steps in `specs/001-build-orin-controller/quickstart.md`.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Final validation, documentation, and linting across all stories.

- [ ] T029 Run `cargo clippy --all-targets` and resolve lints across `firmware/`.
- [ ] T030 [P] Create evidence index summarizing captures in `specs/001-build-orin-controller/evidence/README.md`.
- [ ] T031 [P] Record final Constitution check outcomes and references in `specs/001-build-orin-controller/plan.md`.
- [ ] T034 [P] Capture brown-out retry manual test results by recording defmt logs that show detection, pause, and successful retry, storing artifacts in `specs/001-build-orin-controller/evidence/brownout-retry.log`.
- [ ] T035 [P] Capture USB disconnect safe-state behavior by documenting the REPL-visible error and strap posture after unplugging during recovery, saving logs to `specs/001-build-orin-controller/evidence/usb-disconnect.log`.
- [ ] T036 Run manual integration tests for SC-001 and SC-002, issuing normal reboot and recovery commands, counting successes via defmt logs alongside Jetson console output, and summarizing the tallies in `specs/001-build-orin-controller/evidence/integration-results.md`.
- [ ] T037 Verify SC-004 by confirming telemetry events appear in defmt logs immediately after each command during manual integration runs, summarizing observed timestamps in `specs/001-build-orin-controller/evidence/integration-results.md`.
- [ ] T038 Capture annotated oscilloscope screenshots demonstrating <50 mVpp VDD_3V3 ripple during strap activity and store them under `specs/001-build-orin-controller/evidence/vdd33-ripple.png`.
- [ ] T040 [P] Record firmware memory footprint by running `cargo size --release` (or equivalent) and documenting flash/RAM usage versus the 512 kB/144 kB budgets—including stretch goal progress—in `specs/001-build-orin-controller/evidence/binary-footprint.md`.

---

## Dependencies & Execution Order

- **Phase sequencing**: Phase 1 → Phase 2 → (Phase 3 ∥ Phase 4 ∥ Phase 5 as staffing allows) → Phase 6.
- **User story dependencies**:
  - US1 depends on Phase 2 completion.
  - US2 depends on Phase 2 completion; optionally reuses US1 telemetry patterns but can proceed independently afterward.
  - US3 depends on Phase 2 completion; ensure APO control validated before merging with other stories.
- **Task-level notes**:
  - T012 precedes all other US1 tasks; telemetry updates (T015) may proceed parallel with REPL work (T014) once sequences compile.
  - Recovery bridge integration (T020) must finish before REC release logic (T019) is fully testable.
  - Fault telemetry (T025) should be complete before capturing evidence (T027) to avoid rework.
  - Brown-out and USB disconnect handling (T032, T033) must be validated before running manual success-metric tests (T036–T037) to ensure sequences reflect edge-case behavior.
  - Queue serialization validation (T039) depends on the orchestrator and telemetry scaffolding (T006, T007, T013, T015) and should complete before story-specific command additions proceed to evidence capture.
  - T041 must land before T039 so the validation exercise captures the queued-command telemetry required by FR-005.
  - Binary footprint recording (T040) depends on a release-buildable firmware image and should run alongside final polish once optimizations are in place.

---

## Parallel Execution Examples

- **US1**: Implement REPL command (T014) and telemetry logging (T015) in parallel while sequence logic (T012) is under review.
- **US2**: Develop bridge activity publishing (T020) concurrently with REPL command parsing (T021) once sequence templates (T018) are staged.
- **US3**: Update telemetry (T025) and REPL command handling (T026) simultaneously after the fault sequence (T024) compiles.

---

## Implementation Strategy

Follow the Implementation Strategy in `specs/001-build-orin-controller/plan.md` (§Implementation Strategy, steps 1–6) when scheduling and executing tasks so queued-command validation and binary-footprint checks stay in scope.
