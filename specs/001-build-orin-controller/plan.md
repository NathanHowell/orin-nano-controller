# Implementation Plan: Build Orin Controller Firmware

**Branch**: `001-build-orin-controller` | **Date**: 2025-10-23 | **Spec**: `specs/001-build-orin-controller/spec.md`
**Input**: Feature specification from `specs/001-build-orin-controller/spec.md`

## Summary

Stand up a shared `controller-core` crate that owns strap orchestration, queueing semantics, telemetry events, and the REPL grammar while remaining portable across MCU and host builds. Bind it to the `firmware` crate for STM32G0B1 hardware control and to a new `emulator` crate that exposes the same REPL contract on a host PC, ensuring feature parity. Maintain strict strap timing, recovery automation, and telemetry capture in line with `pcb/orin-nano-controller/README.md`, and validate both targets with evidence logging plus ADC-based power sensing.

## Technical Context

**Language/Version**: Rust stable 1.82.0 (edition 2024) pinned via `rust-toolchain.toml`; no nightly fallback expected.  
**Primary Dependencies**: Workspace split into `controller-core` (business logic, REPL grammar via `logos` + `winnow`, async traits via `embassy-sync`), `firmware` (Embassy `embassy-executor`, `embassy-stm32`, `embassy-time`, `embassy-usb`, `defmt`), and `emulator` (host-side REPL harness using `controller-core` plus standard library concurrency).  
**Storage**: N/A (`no_std`, volatile peripherals only)  
**Testing**: `cargo test -p controller-core` (host) for parser/logic modules; `cargo test` (workspace) to include any future host harnesses; hardware validation via probe-run and bench traces; manual integration capturing `defmt` logs plus emulator parity runs.  
**Target Platform**: STM32G0B1KETx (Cortex-M0+) via `firmware` crate; host PC (macOS/Linux) via `emulator` crate; both consume the same `controller-core` APIs.  
**Project Type**: Embedded firmware + host CLI (multi-crate workspace)  
**Performance Goals**: Meet strap timing windows (RESET ≥20 ms, REC prehold 100 ms, power button 200±20 ms); sustain UART bridge throughput at 115200 bps without overflow; REPL command latency <50 ms once line submitted; release binary footprint within 512 kB flash / 144 kB RAM (stretch ≤64 kB flash)  
**Constraints**: `controller-core` must stay `#![no_std]` compatible (guarded by `alloc` feature toggles if needed); `firmware` remains allocator-free with dual CDC enumeration; `emulator` may use `std` but must not diverge behavior; VDD_3V3 rail budget <500 mA; serialized strap sequences (no overlap).  
**Scale/Scope**: Single controller per device; REPL queue depth 4; no multi-host coordination required

## Constitution Check

*Gate status*: satisfied ahead of Phase 0; re-run after Phase 1 design updates to confirm nothing drifted.

- **Principle I – Unified Pin Contracts**: Verified the J14 strap mapping against `pcb/orin-nano-controller/README.md` (“Jetson J14 Strap Control (`J2`)”). The spec’s Hardware Interface Contracts cite the same nets (`STRAP_RESET_L`, `STRAP_RECOVERY_L`, `STRAP_POWER_BTN_L`, `STRAP_APO_L`), so the firmware abstractions will mirror those names.
- **Principle II – Deterministic Boot Stewardship**: Firmware will schedule explicit Embassy tasks: `usb::composite()` for the dual CDC device, `repl::session()` on CDC0, `bridge::uart_task()` on CDC1, `straps::orchestrator()` as the FSM, and a `telemetry::flusher()`. Each sequence uses `embassy-time` timers with spec-derived budgets (RESET 20 ms low, REC pre-hold 100 ms / post-hold 500 ms, PWR pulse 200 ms ±20 ms, APO pre-hold 250 ms, and REC auto-release on UART activity). Command ingress exists only on the directly attached USB CDC port, so no remote transport can issue sequences. The orchestrator flow is documented in `specs/001-build-orin-controller/boot_state_machine.dot` for gate review.
- **Principle III – Hardware-in-the-Loop Assurance**: Phase 1 task T002 inventories bench gear; Phases 3–5 (T016, T022, T027) capture logic analyzer traces, USB logs, and SWD recovery checklists. All evidence will land under `specs/001-build-orin-controller/evidence/` as required.
- **Principle IV – Composable Runtime Architecture**: `controller-core` will host the strap orchestrator, queueing logic, REPL grammar, and telemetry events behind hardware-agnostic traits that compile for both `thumbv6m-none-eabi` and host targets. The `firmware` crate implements those traits with Embassy peripherals while staying `#![no_std]`, and the `emulator` crate provides host-side shims so command parity can be validated without MCU peripherals. Each dependency addition is reviewed against flash/RAM budgets (512 kB / 144 kB, stretch ≤64 kB flash).
- **Principle V – Built-in Observability & Recovery**: Telemetry buffers plus `defmt` logging (T007, T015, T025, T037, T039) record strap transitions, queued-command markers, and elapsed timing metrics. SWD reflashing steps and troubleshooting guidance are updated via T027/T028, ensuring recoverability stays documented.

## Project Structure

### Documentation (this feature)

```text
specs/[###-feature]/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/           # Phase 1 output (/speckit.plan command)
└── tasks.md             # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (repository root)
```text
controller-core/
├── Cargo.toml
├── src/
│   ├── lib.rs               # Trait surfaces + orchestration state machines
│   ├── sequences/           # Normal/recovery/fault templates shared by all targets
│   ├── repl/                # Lexer/parser, command dispatch, command queue
│   └── telemetry/           # Event definitions consumed by firmware + emulator

firmware/
├── Cargo.toml
├── src/
│   ├── main.rs              # Embassy init, task spawning
│   ├── hw/                  # Board-specific pin bindings + trait impls
│   ├── usb/                 # Composite USB + CDC class setup
│   ├── bridge/              # UART ↔ CDC bridge tasks, activity monitor
│   └── telemetry/           # Defmt adapters hooking controller-core events

emulator/
├── Cargo.toml
└── src/
    ├── main.rs              # Host REPL entrypoint reusing controller-core
    └── transport/           # Host UART stub / logging adapter

tests/
└── host/                    # Planned host-side unit/integration tests
```

**Structure Decision**: Adopt a three-crate workspace where `controller-core` exports shared traits and orchestration logic, `firmware` binds those traits to STM32G0 peripherals, and `emulator` reuses the same APIs for host REPL parity while collecting validation evidence.

## Complexity Tracking

No Constitution violations anticipated; track here if scope changes.

## Implementation Strategy

1. Establish the `controller-core` crate with strap orchestration, sequencing templates, REPL grammar, and telemetry events behind trait abstractions; prove host compatibility with `cargo test -p controller-core` on the default toolchain.
2. Refactor the `firmware` crate to implement those traits using Embassy peripherals, ensuring the `thumbv6m-none-eabi` build stays `#![no_std]`, dual CDC enumeration remains intact, and memory timing budgets are enforced.
3. Introduce the `emulator` crate that links against `controller-core`, mirrors the REPL surface, and provides CLI tooling to drive parity tests on a host PC; script smoke tests to exercise normal, recovery, and fault sequences through the shared API.
4. Execute Phase 1–2 groundwork (documentation sync, instrumentation prep, async scaffolding) across all three crates before tackling user stories.
5. Deliver Phases 3–5 user stories, validating each with both hardware evidence (`specs/001-build-orin-controller/evidence/`) and emulator parity captures, while updating documentation as sequences evolve.
6. During Phase 6 polish, consolidate telemetry/evidence summaries, confirm SC-001/002/004 metrics using combined hardware + emulator runs, and document Constitution gate outcomes alongside flash/RAM measurements.
