# Implementation Plan: Build Orin Controller Firmware

**Branch**: `001-build-orin-controller` | **Date**: 2025-10-23 | **Spec**: `specs/001-build-orin-controller/spec.md`
**Input**: Feature specification from `specs/001-build-orin-controller/spec.md`

## Summary

Implement the STM32G0B1-based controller firmware that drives Jetson Orin straps with strict timing, exposes dual USB CDC ACM ports (bridge + operator REPL), and provides telemetry plus auto-release recovery handling. Code will be structured with Embassy async tasks: one composite USB stack, discrete strap orchestration, UART bridge queues, and an interactive REPL with tab completion backed by a lightweight lexer/parser. Evidence logging, ADC-based power sensing, and recovery sequencing must align with the spec and updated hardware notes in `pcb/orin-nano-controller/README.md`.

## Technical Context

**Language/Version**: Rust stable 1.90.0 (pinned via `rust-toolchain.toml`) with nightly fallback only if Embassy USB requires it *(confirm installed toolchain version)*  
**Primary Dependencies**: Embassy (`embassy-executor`, `embassy-stm32`, `embassy-time`, `embassy-usb`, `embassy-sync`), `heapless` ring buffers, `logos` + `winnow` for REPL grammar, `defmt`/`defmt-rtt` for diagnostics  
**Storage**: N/A (`no_std`, volatile peripherals only)  
**Testing**: `cargo test` (host) for parser/logic modules using `std` harness; hardware validation via probe-run plus bench equipment traces; manual integration runs capturing `defmt` logs (telemetry) alongside Jetson console checks to tally success metrics  
**Target Platform**: STM32G0B1KETx (Cortex-M0+) on Orin controller PCB, USB FS device mode, powering Jetson strap drivers via TLV75533PDBV  
**Project Type**: Firmware (embedded async, single binary in `firmware/`)  
**Performance Goals**: Meet strap timing windows (RESET ≥20 ms, REC prehold 100 ms, power button 200±20 ms); sustain UART bridge throughput at 115200 bps without overflow; REPL command latency <50 ms once line submitted; release binary footprint within 512 kB flash / 144 kB RAM (stretch ≤64 kB flash)  
**Constraints**: `#![no_std]`, no allocator, dual CDC enumeration, fixed-size Embassy channels/queues, VDD_3V3 rail budget <500 mA, serialized strap sequences (no overlap)  
**Scale/Scope**: Single controller per device; REPL queue depth 4; no multi-host coordination required

## Constitution Check

*Gate status*: satisfied ahead of Phase 0; re-run after Phase 1 design updates to confirm nothing drifted.

- **Principle I – Unified Pin Contracts**: Verified the J14 strap mapping against `pcb/orin-nano-controller/README.md` (“Jetson J14 Strap Control (`J2`)”). The spec’s Hardware Interface Contracts cite the same nets (`STRAP_RESET_L`, `STRAP_RECOVERY_L`, `STRAP_POWER_BTN_L`, `STRAP_APO_L`), so the firmware abstractions will mirror those names.
- **Principle II – Deterministic Boot Stewardship**: Firmware will schedule explicit Embassy tasks: `usb::composite()` for the dual CDC device, `repl::session()` on CDC0, `bridge::uart_task()` on CDC1, `straps::orchestrator()` as the FSM, and a `telemetry::flusher()`. Each sequence uses `embassy-time` timers with spec-derived budgets (RESET 20 ms low, REC pre-hold 100 ms / post-hold 500 ms, PWR pulse 200 ms ±20 ms, APO pre-hold 250 ms, and REC auto-release on UART activity). Command ingress exists only on the directly attached USB CDC port, so no remote transport can issue sequences. The orchestrator flow is documented in `specs/001-build-orin-controller/boot_state_machine.dot` for gate review.
- **Principle III – Hardware-in-the-Loop Assurance**: Phase 1 task T002 inventories bench gear; Phases 3–5 (T016, T022, T027) capture logic analyzer traces, USB logs, and SWD recovery checklists. All evidence will land under `specs/001-build-orin-controller/evidence/` as required.
- **Principle IV – Lean Firmware Architecture**: The crate remains `#![no_std]`, Embassy-based, and confines board-specific pin setup to `firmware/src/straps/` and `firmware/src/usb/`. Memory usage is reviewed before adding dependencies; no allocator or RTIC is introduced, and every change is checked against the 512 kB flash / 144 kB RAM limits (stretch ≤64 kB flash).
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
firmware/
├── Cargo.toml
├── src/
│   ├── main.rs              # Embassy init, task spawning
│   ├── usb/                 # Composite USB + CDC class setup (planned)
│   ├── repl/                # Lexer/parser, line editor, command executor (planned)
│   ├── straps/              # Strap orchestrator, sequence templates, recovery FSM (planned)
│   ├── bridge/              # UART ↔ CDC bridge tasks, activity monitor (planned)
│   └── telemetry/           # Evidence logger, power monitor integration (planned)
└── tests/ (host-side unit tests for parsers & recovery logic - to be created)
```

## Complexity Tracking

No Constitution violations anticipated; track here if scope changes.

## Implementation Strategy

1. Complete Phases 1–2 to secure tooling, documentation, brown-out/USB safe-state handling, and async scaffolding.
2. Deliver MVP by finishing Phase 3 (US1) and validating reboot timing with captured evidence.
3. Layer recovery capabilities (Phase 4) and validate USB/APX enumeration with fresh captures.
4. Add fault recovery workflow (Phase 5), ensuring APO control, retry handling, and sequential queue behavior are documented.
5. Run manual integration campaigns that issue normal reboot and recovery commands, exercise queued-command serialization, capture `defmt` logs to count SC-001/002 successes, verify SC-004 live telemetry visibility, and archive summaries under `specs/001-build-orin-controller/evidence/`.
6. Finish Phase 6 polish: resolve clippy warnings, capture VDD₃V₃ ripple evidence, record release binary size against flash/RAM budgets, index artifacts, and log Constitution outcomes.
