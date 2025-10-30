# Quickstart

## Prerequisites
- Rust toolchain `1.91.0` with the 2024 edition (`rustup default stable`).
- Target support: `rustup target add thumbv6m-none-eabi`.
- [`probe-rs`](https://probe.rs) tooling (`cargo install probe-rs-tools --locked`) or `cargo-embed`.
- ST-LINK/V3 or equivalent SWD probe wired to header J1 (VTref → 3V3, SWDIO, SWCLK, NRST, GND).
- Logic analyzer (8+ channels) and oscilloscope for validation captures.

## Build & Flash
1. **Fetch dependencies**
   ```bash
   cargo fetch
   ```
2. **Compile firmware (debug)**
   ```bash
   cargo build --target thumbv6m-none-eabi -p orin-nano-controller
   ```
   The workspace `.cargo/config.toml` already defaults to `thumbv6m-none-eabi`, so the explicit `--target` flag is optional as long as the cross target has been installed.
3. **Flash & stream defmt logs**
   ```bash
   probe-rs run --chip STM32G0B1KETx \
     --defmt \
     --speed 24000 \
     -- elf target/thumbv6m-none-eabi/debug/orin-nano-controller
   ```
   Logs show strap transitions, voltage checks, and command responses.
4. **Release build for deployment**
   ```bash
   cargo build --release --target thumbv6m-none-eabi -p orin-nano-controller
   probe-rs download --chip STM32G0B1KETx target/thumbv6m-none-eabi/release/orin-nano-controller
   ```

## USB CDC Control Channel
1. Connect the controller USB-C (J3) to a host PC; it enumerates as **two** CDC ACM devices (e.g., `/dev/ttyACM0` for the REPL, `/dev/ttyACM1` for the bridge).
2. Attach to the REPL port (`ttyACM0` in this example) with your preferred terminal program at `115200-N-8-1`.
3. Press `Tab` to list available commands. Example session:
   ```
   > help
   reboot [now|delay <duration>]
   recovery [enter|exit|now]
   fault recover [retries=<1-3>]
  status
   ```
4. Execute a normal reboot:
   ```
   > reboot now
   OK reboot duration=1.22s
   ```
5. Status messages stream as the sequence runs:
   ```
   reboot strap=RESET* asserted t=+1.20s
   reboot strap=REC* released t=+1.22s
   ```
6. Trigger an immediate recovery reboot that releases once console traffic appears:
   ```
   > recovery now
   OK recovery waiting-for-console
   # later...
   recovery console-activity detected=ttyACM1
   ```
7. Tab completion works at every position (`reco<Tab>` → `recovery`); repeated Tab shows all matches while keeping the prompt parked on the bottom line.
8. Invalid characters never land in the buffer—the REPL emits a terminal BEL and ignores them. Well-formed but unsupported commands respond generically:
   ```
   > recovery foo
   ERR syntax expected one of: enter, exit, now
   ```
9. `status` prints the live strap levels, power rail reading, and how long it has been since bridge RX/TX activity:
   ```
   > status
   straps RESET*=released REC*=released PWR*=released APO=released
   power vdd=3300mV control-link=attached
   bridge waiting=false rx=n/a tx=n/a
   ```

## UART Bridge
- The firmware launches two async tasks:
  - USB→Jetson writer fed by `usb_to_ttl` channel (capacity 4×64 B frames).
  - Jetson→USB reader backed by `ttl_to_usb` channel.
- Bridge defaults to 115200 baud on `USART2` (PB0/PB1). Attach to the second CDC port (`ttyACM1`) for a transparent Jetson console; no REPL commands are required to enable it.

## Evidence Capture
1. Strap sequence validation:
   - Probe `RESET*`, `REC*`, `PWR*`, `APO`, and `VDD_3V3` with a logic analyzer.
   - Export `.sal` traces to `specs/001-build-orin-controller/evidence/`.
2. Power ripple:
   - Scope the 3.3 V rail while running the recovery sequence.
   - Capture screenshots showing <50 mVpp ripple.
3. USB enumeration:
   - Record `dmesg`/`usbmon` logs during normal boot and recovery entry; archive text output beside traces.

## Troubleshooting
- Brown-out aborts: Check 5 V input and confirm TLV75533 output ≥3.1 V; firmware will stay in `Error` until voltage stabilizes.
- USB not enumerating: Ensure Jetson host initiated VBUS; controller stays in UFP mode with CC 5.1 kΩ pulldowns.
- SWD recovery: The STM32G0B1 multiplexes `BOOT0` onto the SWCLK pin; follow the datasheet procedure (set `nBOOT_SEL`/`nSWBOOT0` option bits or hold SWCLK high during reset) before reflashing with probe-rs.
