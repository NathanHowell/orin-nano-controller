<!--
Sync Impact Report
Version: 1.0.0 → 1.1.0
Modified Principles:
- IV. Lean Firmware Architecture → IV. Composable Runtime Architecture
Added Sections:
- None
Removed Sections:
- None
Templates requiring updates:
- ✅ .specify/templates/plan-template.md
- ✅ .specify/templates/spec-template.md
- ✅ .specify/templates/tasks-template.md
Follow-up TODOs:
- None
-->

# Orin Nano Controller Constitution

## Core Principles

### I. Unified Pin Contracts
- MUST document every connector pin, strap net, voltage rail, and reference designator change in `pcb/orin-nano-controller/README.md` before merging any hardware-affecting change.
- MUST keep KiCad net labels, BOM entries, and firmware abstractions (constants, type aliases, documentation comments) synchronized in the same change set when pinouts shift.
- MUST block feature kick-off until the plan/spec explicitly cites the affected contract section and confirms the documentation is up to date.
Rationale: A single, traceable source of truth keeps firmware behavior aligned with the physical board and prevents Jetson bring-up faults.

### II. Deterministic Boot Stewardship
- MUST express boot, reset, USB attach, and strap drive sequences as explicit Embassy tasks or finite state machines with bounded timing.
- MUST use `embassy-time` timers (or equivalent non-blocking delays) for any wait exceeding a few microseconds; busy loops are forbidden outside critical sections.
- MUST capture allowed voltage ramps and strap timing windows in the plan/spec whenever behavior changes, and review them before implementation starts.
Rationale: Deterministic orchestration protects the Jetson carrier interface and ensures reproducible power-up behavior.

### III. Hardware-in-the-Loop Assurance
- MUST define hardware-in-the-loop or bench validation tasks (logic analyzer trace, multimeter measurement, or equivalent) before implementation for any change that toggles straps, power rails, or USB.
- MUST record empirical evidence (trace captures, photos, or measurement notes) in the feature documentation or checklist before closing the work.
- MUST version any scripts, fixtures, or harness firmware required to reproduce validation inside this repository under `.specify/` or `firmware/`.
Rationale: Physical feedback is the only trustworthy confirmation that board and firmware changes work together safely.

### IV. Composable Runtime Architecture
- MUST keep the `firmware` crate `#![no_std]`, embed on the Embassy executor, and review memory/performance impact before adding dependencies.
- MUST implement strap orchestration, business rules, and the REPL grammar in a shared `controller-core` crate that compiles for both `thumbv6m-none-eabi` and host targets without MCU peripherals.
- MUST treat the `firmware` crate as the hardware binding layer that satisfies `controller-core` traits for pins, timers, storage, and telemetry while keeping board specifics isolated.
- MUST provide an `emulator` crate that links against `controller-core`, offers a host-side REPL, and exercises the same command surface used on hardware so behavior stays in lock-step.
Rationale: A clear core-versus-binding split enables shared testing, host tooling parity, and safe reuse of validated control logic across targets.

### V. Built-in Observability & Recovery
- MUST provide at least one recovery path (SWD flashing, physical strap override, or equivalent) documented for every new behavior that could wedge the Jetson.
- MUST instrument strap transitions and error conditions with Defmt/RTT logging, SWO, or a documented diagnostic GPIO pattern, and include how to read it in the plan/spec.
- MUST update troubleshooting guidance whenever observability hooks or recovery steps evolve.
Rationale: Embedded bring-up relies on clear introspection and safe fallbacks when the board misbehaves.

## Engineering Constraints

- Firmware targets the STM32G0B1 and builds with `cargo` for `thumbv6m-none-eabi`; releases must remain `no_std` and panic with `panic-halt`.
- Embassy crates (`embassy-executor`, `embassy-stm32`, `embassy-time`) and their enabled features are considered locked; proposals to change them require governance approval.
- Workspace MUST include the crates `controller-core` (shared logic/API), `firmware` (board bindings), and `emulator` (host REPL). Changes that drift their public surfaces out of sync MUST be reviewed together.
- PCB design lives in KiCad; revisions MUST update `pcb/orin-nano-controller/*.kicad_*` files alongside `pcb/orin-nano-controller/README.md` and note the revision in feature docs.
- USB-C interface follows the BOM in `pcb/orin-nano-controller/README.md`; capacitance and strap resistor values listed there are normative and deviations require explicit sign-off.
- Mechanical interfaces (Samtec J14, SWD header) must preserve current footprint orientation unless a migration plan is documented.
- Use async/await patterns with Embassy tasks for all I/O operations; blocking calls are forbidden outside critical sections.

## Workflow & Quality Gates

- Specs MUST include a `Hardware Interface Contracts` section mapping impacted pins, voltage domains, and timing budgets, citing `pcb/orin-nano-controller/README.md` line items.
- Implementation plans MUST answer the Constitution Check gate list, including the boot state machine diagram, validation strategy, and observability hooks.
- Plans MUST state how `controller-core`, `firmware`, and `emulator` are affected, confirming APIs stay aligned and host tooling remains functional before coding begins.
- Task breakdowns MUST include hardware-in-the-loop validation work, evidence capture, and documentation updates before marking stories complete.
- Before starting a new implementation phase or marking any task complete, contributors MUST run `just check` at the repository root (which invokes rustfmt, Clippy, and tests) and resolve all failures.
- Code reviews MUST verify that firmware modules isolate board-specific details and that instrumentation/recovery updates are documented.
- Monthly (or feature-level) retrospectives MUST schedule a compliance review to confirm principles remain enforceable and update templates when drift is detected.

## Governance

- This constitution supersedes ad-hoc practices for firmware or PCB work; conflicts are resolved in favor of these rules.
- Amendments require a documented proposal summarizing motivation, risk, and migration steps, plus recorded approval from maintainers of both firmware and PCB domains.
- Constitution versions follow semantic versioning: MAJOR for incompatible principle changes, MINOR for new principles/sections or materially expanded mandates, PATCH for clarifications and non-substantive edits.
- Every amendment PR MUST update the Sync Impact Report, increment the version, and set `Last Amended` to the merge date.
- Compliance reviews occur quarterly at minimum; findings and remediation tasks are tracked in `.specify/` artifacts or issue trackers linked from feature docs.

**Version**: 1.1.0 | **Ratified**: 2025-10-23 | **Last Amended**: 2025-10-25
