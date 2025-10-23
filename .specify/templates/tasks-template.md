---

description: "Task list template for feature implementation"
---

# Tasks: [FEATURE NAME]

**Input**: Design documents from `/specs/[###-feature-name]/`
**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/

**Validation**: Include hardware-in-the-loop (HIL) / bench evidence capture tasks for any strap, power, or USB change. Firmware tests are OPTIONAL unless specified in the feature spec.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

- Firmware: `firmware/src/`, `firmware/Cargo.toml`, `firmware/.cargo/`
- Bench/HIL scripts & docs: `.specify/`, `pcb/orin-nano-controller/BASELINE.md`, feature docs under `specs/`
- Logic analyzer captures, photos, and evidence: store under `specs/[###-feature-name]/evidence/`
- Update additional directories as required by the implementation plan

<!-- 
  ============================================================================
  IMPORTANT: The tasks below are SAMPLE TASKS for illustration purposes only.
  
  The /speckit.tasks command MUST replace these with actual tasks based on:
  - User stories from spec.md (with their priorities P1, P2, P3...)
  - Feature requirements from plan.md
  - Entities from data-model.md
  - Endpoints from contracts/
  
  Tasks MUST be organized by user story so each story can be:
  - Implemented independently
  - Tested independently
  - Delivered as an MVP increment
  
  DO NOT keep these sample tasks in the generated tasks.md file.
  ============================================================================
-->

## Phase 1: Contracts & Tooling (Shared Infrastructure)

**Purpose**: Confirm documentation, tooling, and instrumentation prerequisites before firmware work begins.

- [ ] T001 Update `pcb/orin-nano-controller/BASELINE.md` with any connector or strap details referenced by this feature.
- [ ] T002 [P] Re-run the plan's Constitution Check answers and document them in `specs/[###-feature-name]/plan.md`.
- [ ] T003 [P] Verify `firmware/.cargo/config.toml` target configuration and run `cargo check --target thumbv6m-none-eabi`.
- [ ] T004 Prepare bench instrumentation (logic analyzer, oscilloscope, SWD probe) and capture setup notes in `specs/[###-feature-name]/evidence/bench-setup.md`.

---

## Phase 2: Firmware Foundations (Blocking Prerequisites)

**Purpose**: Establish baseline modules and diagnostics required by all user stories.

**⚠️ CRITICAL**: No user story work can start until these items are complete.

- [ ] T010 Create/refresh `firmware/src/hw/pin_contract.rs` (or equivalent) to mirror updated pin mappings.
- [ ] T011 [P] Implement or adjust the base Embassy task scheduler for strap control in `firmware/src/main.rs`.
- [ ] T012 Integrate or update observability hooks (Defmt/RTT, SWO, diagnostic GPIO) and document usage in `specs/[###-feature-name]/spec.md`.
- [ ] T013 Ensure recovery procedures are documented and validated (e.g., SWD reflash checklist).

**Checkpoint**: Foundation ready – user stories can now be pursued independently.

---

## Phase 3: User Story 1 - [Title] (Priority: P1) 🎯 MVP

**Goal**: [Brief description of what this story delivers]

**Independent Test**: [How to verify this story works on its own]

### Bench Validation for User Story 1 (MANDATORY for strap/power changes)

- [ ] T020 Capture [logic analyzer / oscilloscope] trace demonstrating the documented state machine.
- [ ] T021 Record measurement notes or photos and store them under `specs/[###-feature-name]/evidence/`.

### Implementation for User Story 1

- [ ] T022 [P] [US1] Implement feature module in `firmware/src/[module].rs` adhering to pin contract abstractions.
- [ ] T023 [US1] Update Embassy task wiring in `firmware/src/main.rs` (or orchestrator) to include the new behavior.
- [ ] T024 [US1] Extend observability outputs to emit diagnostics for this story.

### Documentation & Sign-off

- [ ] T025 Update `specs/[###-feature-name]/spec.md` and plan with actual timings and measurement references.
- [ ] T026 Link evidence files and recovery procedure updates in the feature documentation.

**Checkpoint**: User Story 1 is functional, validated on hardware, and documented.

---

## Phase 4: User Story 2 - [Title] (Priority: P2)

**Goal**: [Brief description of what this story delivers]

**Independent Test**: [How to verify this story works on its own]

### Bench Validation for User Story 2

- [ ] T030 Re-run or extend bench captures focusing on the new behavior (identify unique signals to monitor).
- [ ] T031 Document differences from User Story 1 evidence and archive results in `/evidence/`.

### Implementation for User Story 2

- [ ] T032 [P] [US2] Implement supporting module or driver in `firmware/src/[module].rs`.
- [ ] T033 [US2] Adjust power/strap sequencing module to coordinate with existing stories.
- [ ] T034 [US2] Update diagnostics to differentiate failure modes introduced by this story.

### Documentation & Sign-off

- [ ] T035 Update hardware interface tables and add new recovery steps if required.
- [ ] T036 Confirm feature-specific tasks in `specs/[###-feature-name]/tasks.md` are closed with evidence attached.

**Checkpoint**: User Stories 1 and 2 operate independently and together with documented evidence.

---

## Phase 5: User Story 3 - [Title] (Priority: P3)

**Goal**: [Brief description of what this story delivers]

**Independent Test**: [How to verify this story works on its own]

### Bench Validation for User Story 3

- [ ] T040 Capture additional evidence demonstrating combined strap or USB behavior remains within tolerance.
- [ ] T041 Validate recovery paths after fault injection (if applicable) and document results.

### Implementation for User Story 3

- [ ] T042 [P] [US3] Implement module updates in `firmware/src/[module].rs`.
- [ ] T043 [US3] Refine state machine orchestration to handle ordering with previous stories.
- [ ] T044 [US3] Extend observability or telemetry to cover new edge cases.

### Documentation & Sign-off

- [ ] T045 Summarize combined results and update troubleshooting guidance.
- [ ] T046 Ensure all evidence, measurements, and updated pin contracts are linked in feature docs.

**Checkpoint**: All user stories are independently testable, validated on hardware, and fully documented.

---

[Add more user story phases as needed, following the same pattern]

---

## Phase N: Polish & Cross-Cutting Concerns

**Purpose**: Improvements that affect multiple user stories

- [ ] TXXX [P] Documentation updates in docs/
- [ ] TXXX Code cleanup and refactoring
- [ ] TXXX Performance optimization across all stories
- [ ] TXXX [P] Additional unit tests (if requested) in tests/unit/
- [ ] TXXX Security hardening
- [ ] TXXX Run quickstart.md validation

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies - can start immediately
- **Foundational (Phase 2)**: Depends on Setup completion - BLOCKS all user stories
- **User Stories (Phase 3+)**: All depend on Foundational phase completion
  - User stories can then proceed in parallel (if staffed)
  - Or sequentially in priority order (P1 → P2 → P3)
- **Polish (Final Phase)**: Depends on all desired user stories being complete

### User Story Dependencies

- **User Story 1 (P1)**: Can start after Foundational (Phase 2) - No dependencies on other stories
- **User Story 2 (P2)**: Can start after Foundational (Phase 2) - May integrate with US1 but should be independently testable
- **User Story 3 (P3)**: Can start after Foundational (Phase 2) - May integrate with US1/US2 but should be independently testable

### Within Each User Story

- Bench validation tasks MUST be planned and evidence captured before implementation is marked complete.
- Update pin contract modules before orchestrator logic to keep mapping authoritative.
- Add or adjust observability hooks alongside feature code to avoid uninstrumented behavior.
- Refresh documentation (spec, plan, checklist) immediately after validation so evidence is traceable.
- Close recovery procedure updates before moving to the next story.

### Parallel Opportunities

- All Setup tasks marked [P] can run in parallel (documentation sync, plan updates, cargo checks).
- Foundational instrumentation work (e.g., Defmt integration) can run in parallel, but MUST complete before story work.
- Bench validation can proceed in parallel only if fixtures and operators are available; coordinate evidence storage.
- Firmware module updates in different files can proceed in parallel when they touch disjoint pins/peripherals.
- Documentation and evidence collation can run concurrently with firmware fixes once captures exist.

---

## Parallel Example: User Story 1

```bash
# Capture hardware evidence and firmware updates together (if staffing allows):
Task: "Capture logic analyzer trace for strap sequence (specs/.../evidence/strap-seq.sal)"
Task: "Implement StrapController updates in firmware/src/strap_controller.rs"

# Update observability and documentation in parallel:
Task: "Extend defmt logging for new error states"
Task: "Document recovery steps in specs/.../spec.md#observability"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL - blocks all stories)
3. Complete Phase 3: User Story 1
4. **STOP and VALIDATE**: Run bench/HIL validation and archive evidence
5. Tag firmware build/recovery instructions once validated

### Incremental Delivery

1. Complete Setup + Foundational → Foundation ready
2. Add User Story 1 → Validate on hardware → Publish evidence (MVP!)
3. Add User Story 2 → Validate on hardware → Update evidence set
4. Add User Story 3 → Validate on hardware → Update evidence set
5. Each story adds value without breaking previous stories

### Parallel Team Strategy

With multiple developers:

1. Team completes Setup + Foundational together
2. Once Foundational is done:
   - Developer A: User Story 1 firmware module updates
   - Developer B: User Story 2 observability + documentation
   - Developer C: Bench validation and evidence capture (shared resource)
3. Stories complete and integrate independently with shared hardware time boxed

---

## Notes

- [P] tasks = different files or activities (e.g., firmware vs. documentation) with no blocking dependencies.
- [Story] labels map tasks to user stories for traceability back to specs and evidence.
- Each user story must be independently demonstrable on hardware with archived proof.
- Capture instrumentation and recovery updates while coding—do not defer evidence.
- Commit after each task or logical group; attach bench captures and notes in the same PR.
- Stop at checkpoints to validate hardware behavior before starting the next story.
- Avoid: vague tasks, overlapping file edits causing merge pain, or undocumented hardware changes.
