# Implementation Plan: Build Orin Controller Firmware

**Branch**: `001-build-orin-controller` | **Date**: 2025-10-23 | **Spec**: `specs/001-build-orin-controller/spec.md`
**Input**: Feature specification from `specs/001-build-orin-controller/spec.md`

## Summary

Stand up a shared `controller-core` crate that owns strap orchestration, queueing semantics, telemetry events, and the REPL grammar while remaining portable across MCU and host builds. Bind it to the `firmware` crate for STM32G0B1 hardware control and to a new `emulator` crate that exposes the same REPL contract on a host PC, ensuring feature parity. Maintain strict strap timing, recovery automation, and telemetry capture in line with `pcb/README.md`, and validate both targets with evidence logging plus ADC-based power sensing.

## Technical Context

**Language/Version**: Rust stable 1.90.0 (edition 2024) pinned via `rust-toolchain.toml`; no nightly fallback expected.  
**Primary Dependencies**: Workspace split into `controller-core` (business logic, REPL grammar via `logos` + `winnow`, async traits via `embassy-sync`), `firmware` (Embassy `embassy-executor`, `embassy-stm32`, `embassy-time`, `embassy-usb`, `defmt`), and `emulator` (host-side REPL harness using `controller-core` plus standard library concurrency).  
**Storage**: N/A (`no_std`, volatile peripherals only)  
**Testing**: `cargo test -p controller-core` (host) for parser/logic modules; `cargo test` (workspace) to include any future host harnesses; hardware validation via probe-run and bench traces; manual integration capturing `defmt` logs plus emulator parity runs.  
**Target Platform**: STM32G0B1KETx (Cortex-M0+) via `firmware` crate; host PC (macOS/Linux) via `emulator` crate; both consume the same `controller-core` APIs.  
**Project Type**: Embedded firmware + host CLI (multi-crate workspace)  
**Performance Goals**: Meet strap timing windows (RESET ≥20 ms, REC prehold 100 ms, power button 200±20 ms); sustain UART bridge throughput at 115200 bps without overflow; REPL command latency <50 ms once line submitted; release binary footprint within 512 kB flash / 144 kB RAM (stretch ≤64 kB flash)  
**Constraints**: `controller-core` must stay `#![no_std]` compatible (guarded by `alloc` feature toggles if needed); `firmware` remains allocator-free with dual CDC enumeration; `emulator` may use `std` but must not diverge behavior; VDD_3V3 rail budget <500 mA; serialized strap sequences (no overlap).  
**Cargo Config**: Workspace `.cargo/config.toml` defaults builds to `thumbv6m-none-eabi` and invokes `probe-rs run --chip stm32g0b1ketx`; install the target via `rustup target add thumbv6m-none-eabi` and `probe-rs-tools` on host systems to satisfy the runner.  
**Scale/Scope**: Single controller per device; REPL queue depth 4; no multi-host coordination required

## Constitution Check

*Gate status*: Re-validated after introducing the controller-core/firmware/emulator split; no violations detected.

- **Principle I – Unified Pin Contracts**: `controller-core` defines the canonical strap catalog (`StrapLine`) so every target references the same logical pins; `firmware` binds those names to STM32G0B1 pins and SN74LVC07 channels documented in `pcb/README.md`; `emulator` mirrors the same strap identifiers when emitting parity logs to keep host tooling aligned with hardware naming.
- **Principle II – Deterministic Boot Stewardship**: `controller-core` owns the strap FSM and timing budgets, exposing trait hooks for pin drivers and timers; `firmware` wires those hooks to Embassy tasks (`usb::composite`, `repl::session`, `bridge::uart_task`, `straps::orchestrator`, `telemetry::flusher`) so the MCU enforces RESET/REC/PWR/APO windows; `emulator` executes the same orchestrator against host timers to prove command sequencing and cooldown logic before hardware runs.
- **Principle III – Hardware-in-the-Loop Assurance**: `controller-core` emits telemetry events that describe strap timings and bridge activity; `firmware` captures bench evidence (logic analyzer traces, defmt logs, SWD recovery notes) per tasks T002/T016/T022/T027 into `specs/001-build-orin-controller/evidence/`; `emulator` records REPL transcripts and parity logs so every workflow can be rehearsed on the host before moving to silicon.
- **Principle IV – Composable Runtime Architecture**: `controller-core` stays `#![no_std]` with optional `alloc`, exporting traits that compile for `thumbv6m-none-eabi` and host targets; `firmware` implements those traits with Embassy peripherals without introducing dynamic allocation; `emulator` links the same crate under `std`, adding only host I/O facades so behavior remains identical while keeping flash/RAM impact confined to MCU builds.
- **Principle V – Built-in Observability & Recovery**: `controller-core` defines telemetry enums and payloads surfaced through the orchestrator; `firmware` routes them through `defmt`, SWD recovery guides, and ADC-based power monitoring to meet FR-005/SC-002 evidence requirements; `emulator` streams the same telemetry over its diagnostics channel, ensuring documentation and troubleshooting steps cover both targets.

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
6. During Phase 6 polish, consolidate telemetry/evidence summaries, confirm SC-001/002/004 metrics using combined hardware + emulator runs, and document Constitution gate outcomes alongside flash/RAM measurements. Add a VREFINT-backed power monitor so firmware can detect brown-outs via ADC1 channel 10 using the factory calibration constant at `0x1FFF_75AA`, and fold the new sampling logic into the shared `PowerMonitor` abstraction.
