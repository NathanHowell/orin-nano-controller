# Implementation Plan: Build Orin Controller Firmware

**Branch**: `001-build-orin-controller` | **Date**: 2025-10-23 | **Spec**: `specs/001-build-orin-controller/spec.md`
**Input**: Feature specification from `specs/001-build-orin-controller/spec.md`

## Summary

Implement the STM32G0B1-based controller firmware that drives Jetson Orin straps with strict timing, exposes dual USB CDC ACM ports (bridge + operator REPL), and provides telemetry plus auto-release recovery handling. Code will be structured with Embassy async tasks: one composite USB stack, discrete strap orchestration, UART bridge queues, and an interactive REPL with tab completion backed by a lightweight lexer/parser. Evidence logging, ADC-based power sensing, and recovery sequencing must align with the spec and updated hardware notes in `pcb/orin-nano-controller/README.md`.

## Technical Context

**Language/Version**: Rust stable (targeting 1.82.0+) with nightly fallback only if Embassy USB requires it *(confirm installed toolchain version)*  
**Primary Dependencies**: Embassy (`embassy-executor`, `embassy-stm32`, `embassy-time`, `embassy-usb`, `embassy-sync`), `heapless` ring buffers, `logos` + `winnow` for REPL grammar, `defmt`/`defmt-rtt` for diagnostics  
**Storage**: N/A (`no_std`, volatile peripherals only)  
**Testing**: `cargo test` (host) for parser/logic modules using `std` harness; hardware validation via probe-run plus bench equipment traces  
**Target Platform**: STM32G0B1KETx (Cortex-M0+) on Orin controller PCB, USB FS device mode, powering Jetson strap drivers via TLV75533PDBV  
**Project Type**: Firmware (embedded async, single binary in `firmware/`)  
**Performance Goals**: Meet strap timing windows (RESET ≥20 ms, REC prehold 100 ms, power button 200±20 ms); sustain UART bridge throughput at 115200 bps without overflow; REPL command latency <50 ms once line submitted  
**Constraints**: `#![no_std]`, no allocator, dual CDC enumeration, fixed-size Embassy channels/queues, VDD_3V3 rail budget <500 mA, serialized strap sequences (no overlap)  
**Scale/Scope**: Single controller per device; REPL queue depth 4; no multi-host coordination required

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- Confirm the impacted connector/pin data in `pcb/orin-nano-controller/README.md` is current and cross-referenced here (Principle I).  
- Provide the deterministic strap orchestration FSM + Embassy task layout (USB composite, REPL, UART bridge, strap executor, monitors) with timing budgets derived from the spec (Principle II).  
- Define bench validation: logic analyzer capture of strap timings, oscilloscope on VDD_3V3 ripple, USB enumeration logs, and SWD recovery walkthrough (Principle III).  
- Specify observability: `defmt-rtt` streaming over SWD plus REPL status output, including how telemetry is captured during tests (Principle V).  
- Affirm firmware remains `#![no_std]` Rust on STM32G0 with Embassy; any deviation (e.g., enabling allocator or RTIC) requires review (Principle IV).

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
