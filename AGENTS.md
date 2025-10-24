# orin-nano-controller Development Guidelines

Auto-generated from all feature plans. Last updated: 2025-10-23

## Active Technologies
- Rust stable (edition 2024, 1.82.0+) + `embassy-executor 0.9.1`, `embassy-stm32 0.4.0` (`time-driver-tim1`, `stm32g0b1ke`), `embassy-time 0.5.0`, `embassy-usb 0.3.0`, `defmt`, `embassy-sync`, `logos`, `winnow`, `panic-halt` (001-build-orin-controller)

## Project Structure

```text
firmware/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── usb/
│   ├── repl/
│   ├── straps/
│   ├── bridge/
│   └── telemetry/
└── tests/         # host-side unit tests (planned)
```

## Commands

- `cargo test` (host-side parser/recovery unit tests)
- `cargo clippy --all-targets`

## Code Style

Rust stable (edition 2024): Follow standard embedded Rust conventions (`#![no_std]`, Clippy clean)

## Recent Changes
- 001-build-orin-controller: Added Rust stable w/ Embassy stack, `logos`, and `winnow` for REPL grammar

<!-- MANUAL ADDITIONS START -->
<!-- MANUAL ADDITIONS END -->
