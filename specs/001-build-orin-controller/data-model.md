# Data Model

## Entities

### `StrapLine`
- **Fields**: `name` (`RESET`, `REC`, `PWR`, `APO`), `mcu_pin` (`PA4`, `PA3`, `PA2`, `PA5`), `driver_output` (`SN74LVC07` channel `2Y`, `1Y`, `2Y`, `1Y` respectively), `j14_pin` (`8`, `10`, `12`, `5`), `polarity` (`ActiveLow`), `default_state` (`ReleasedHigh`).
- **Relationships**: Referenced by every `StrapStep`; owned by `StrapOrchestrator`.
- **Validation rules**: Mapping must remain in sync with `pcb/orin-nano-controller.kicad_sch` nets `ORST`, `OREC`, `OPWR`, `OAPO` → `/Controller/*_STRAP`.

### `StrapSequenceKind`
- **Fields**: Enumeration values `NormalReboot`, `RecoveryEntry`, `RecoveryImmediate`, `FaultRecovery`.
- **Relationships**: Chosen inside `SequenceCommand`; mapped to a `SequenceTemplate`.
- **Validation rules**: Exhaustive match in firmware; new variants require spec/plan update.

### `StrapStep`
- **Fields**: `line: StrapLine`, `action: StrapAction` (`AssertLow`, `ReleaseHigh`), `hold_for: Milliseconds`, `constraints: TimingConstraintSet`, `completion: StepCompletion`.
- **Relationships**: Ordered list inside a `SequenceTemplate`.
- **Validation rules**: `hold_for` must satisfy spec windows (`PWR*` 200±20 ms, `RESET*` ≥20 ms, `REC*` pre/post windows, `APO` 250 ms); `completion` captures whether advancement is duration-based or triggered by external signals.

### `StepCompletion`
- **Fields**: `variant StepCompletion { AfterDuration, OnBridgeActivity, OnEvent(TelemetryEventKind) }`.
- **Relationships**: Referenced by `StrapStep` to represent non-time-based transitions.
- **Validation rules**: `OnBridgeActivity` allowed only for REC strap steps in `RecoveryImmediate`; other variants must map to telemetry events emitted by the orchestrator.

### `SequenceTemplate`
- **Fields**: `kind: StrapSequenceKind`, `phases: Vec<StrapStep>`, `cooldown: Milliseconds`, `max_retries: Option<u8>`.
- **Relationships**: Owned by `StrapOrchestrator`; referenced when instantiating a `SequenceRun`.
- **Validation rules**: `cooldown` ≥1000 ms for `PWR*`; `max_retries` = 3 for `FaultRecovery`, `None` otherwise.

### `SequenceCommand`
- **Fields**: `kind: StrapSequenceKind`, `requested_at: Instant`, `source: CommandSource` (`UsbHost` only per policy), `flags` (`force_recovery` boolean reserved).
- **Relationships**: Pushed into `CommandQueue`; yields a `SequenceRun`.
- **Validation rules**: Reject if any other `SequenceRun` is active; report `BUSY` to host when queue is full.

### `SequenceRun`
- **Fields**: `command: SequenceCommand`, `state: SequenceState`, `emitted_events: Vec<EventId>`, `retry_count: u8`, `waiting_on_bridge: bool`.
- **Relationships**: Managed by `StrapOrchestrator`; emits telemetry events over `defmt` for each transition.
- **Validation rules**: `retry_count` ≤ template `max_retries`; transitions follow deterministic FSM (Idle → Arming → Executing → Cooldown → Complete/Error); `waiting_on_bridge` flag only set for `RecoveryImmediate` runs until console traffic is detected.

### `CommandQueue`
- **Fields**: `channel: embassy_sync::channel::Channel<ThreadModeRawMutex, SequenceCommand, 4>`.
- **Relationships**: Producers = `ReplSession`; consumer = `StrapOrchestrator`.
- **Validation rules**: Capacity fixed at 4 to honor "fixed size" requirement and prevent unbounded host queueing; `try_send` errors surface to host as `BUSY`.

### `JetsonPowerMonitor`
- **Fields**: `adc: embassy_stm32::adc::Adc<'static, ADC1>`, `channel: PcLedChannel`, `sample_interval: Milliseconds`, `threshold: u16`.
- **Relationships**: Samples the PC_LED_MON divider to infer Jetson front-panel LED state; reports status via `defmt` telemetry events.
- **Validation rules**: Apply simple hysteresis around the configured threshold; debounce reporting (≥5 ms) to avoid chatter when the LED PWM updates.

### `UsbCompositeDevice`
- **Fields**: `repl_port: UsbPortHandle`, `bridge_port: UsbPortHandle`, `device_builder: embassy_usb::Builder`.
- **Relationships**: Initializes two CDC ACM classes—one bound to the REPL, the other dedicated to the UART bridge—sharing the same USB peripheral and descriptor set.
- **Validation rules**: Ensure each interface advertises unique descriptors and endpoints; enumerate successfully on Linux/macOS/Windows as `ttyACM*` pairs.

### `CommandGrammar`
- **Fields**: `commands: &'static [CommandSpec]`, where each spec includes mnemonic, parameters, help text, and executor tag.
- **Relationships**: Shared between the lexer/parser and completion engine; authoritatively describes accepted syntax.
- **Validation rules**: Must stay synchronized with the actual executor implementations; unit tests assert round-trip (format → parse → format) correctness.

### `TokenKind`
- **Fields**: `enum TokenKind { Ident, Integer, Duration, Flag, Equals, Comma, Eol, Error }` generated with `regal`.
- **Relationships**: Drives the `winnow` parser; produced by the lexer from raw REPL input.
- **Validation rules**: Mapping covers all grammar terminals; `Error` tokens bubble into parser diagnostics with byte offsets.

### `CompletionEngine`
- **Fields**: `grammar: &'static CommandGrammar`, `scratch: heapless::Vec<&'static str, 16>`.
- **Relationships**: Consulted by the line editor on Tab presses to emit suggestions based on the current token prefix.
- **Validation rules**: Must only offer matches valid in the current grammar context; degrade gracefully when multiple possibilities exist (emit list, keep buffer unchanged).

### `LineEditor`
- **Fields**: `buffer: heapless::Vec<u8, 128>`, `cursor: usize`, `completer: CompletionEngine`.
- **Relationships**: Runs inside the REPL task, echoing characters back over CDC/UART, handling control characters (backspace, Tab, CR/LF) while reserving the bottom line of the terminal as the active input row.
- **Validation rules**: Enforce fixed buffer size; ignore unsupported control sequences; ensure UTF-8 correctness by restricting to ASCII command set and rejecting disallowed bytes with a terminal BEL instead of buffering them; keep the prompt anchored on the last terminal line by using standard VT100 cursor saves/restores so command output appears above the editor.

### `ReplSession`
- **Fields**: `port: UsbPortHandle`, `grammar: &'static CommandGrammar`, `executor: CommandExecutor`, `line_editor: LineEditor`, `lexer: regal::TokenCache<TokenKind, MAX_TOKENS>`.
- **Relationships**: Consumes bytes from the REPL CDC port, produces `SequenceCommand` or administrative requests, and emits textual responses.
- **Validation rules**: Process one command at a time to honor FR-005; rely on the line editor to gate invalid characters so parser errors are reported generically (`ERR <code> <message>`) without caret positioning; integrate with telemetry for observability.

### `CommandExecutor`
- **Fields**: references to `CommandQueue`, `BridgeQueue`, `BridgeActivityMonitor`, and configuration state.
- **Relationships**: Invoked by `ReplSession` once parsing succeeds; translates high-level commands into orchestrator actions or configuration changes.
- **Validation rules**: Must acknowledge command completion/failure; reject illegal combinations before they reach strap logic; `status` composes a snapshot of each strap state plus relative ages for bridge RX/TX activity; `recovery now` registers a bridge listener before enqueuing the `RecoveryImmediate` template.

### `BridgeQueue`
- **Fields**: `usb_to_ttl: embassy_sync::channel::Channel<ThreadModeRawMutex, [u8; 64], 4>`, `ttl_to_usb: embassy_sync::channel::Channel<ThreadModeRawMutex, [u8; 64], 4>`.
- **Relationships**: Shared by `UsbCdcBridgeTask` and `UartBridgeTask`.
- **Validation rules**: Fixed capacity 4; enforce frame size ≤64 bytes; backpressure through `await` to avoid polling.

### `BridgeActivityMonitor`
- **Fields**: `subscriber: embassy_sync::channel::Receiver<'static, ThreadModeRawMutex, [u8; 64], 4>`, `pending: bool`, `last_rx: Option<Instant>`, `last_tx: Option<Instant>`.
- **Relationships**: Observes UART RX frames from `UartBridgeTask` and send completions from `UsbCdcBridgeTask`; notifies `StrapOrchestrator` when Jetson console output appears during `RecoveryImmediate`.
- **Validation rules**: Treat only non-empty payloads as activity; debounce by requiring a minimum inter-event spacing; update RX/TX timestamps on every forwarded frame so the REPL can report `status` relative times; clear `pending` once the orchestrator acknowledges the release.

### `UsbCdcBridgeTask`
- **Fields**: `class: embassy_usb::class::cdc_acm::CdcAcmClass<'static, UsbBus>`, `rx_buf: [u8; 64]`.
- **Relationships**: Producer for `usb_to_ttl`; consumer for `ttl_to_usb`; forwards packets exclusively for the UART bridge CDC interface.
- **Validation rules**: Use `wait_connection` and `read_packet` futures; no busy loops; keep framing transparent so the Jetson console sees raw bytes; update the `BridgeActivityMonitor` whenever a packet is forwarded to the Jetson so status timings remain accurate.

### `UartBridgeTask`
- **Fields**: `uart: embassy_stm32::usart::BufferedUart<'static, USART2>`, `bridge_port: UsbPortHandle`, `rx_buf: [u8; 64]`, `tx_buf: [u8; 64]`.
- **Relationships**: Producer for `ttl_to_usb`; consumer for `usb_to_ttl`; forwards traffic through the dedicated bridge CDC port.
- **Validation rules**: Configure at Jetson-compatible baud (default 115200); rely on DMA/interrupt-driven buffered API; resume gracefully on UART errors; keep bridge traffic isolated from the REPL channel; notify `BridgeActivityMonitor` as RX frames arrive so `status` reflects live activity.

### `DiagnosticsFrame`
- **Fields**: `event: TelemetryEventKind`, `timestamp_us: u64`, `jetson_power: Option<bool>`, `notes: heapless::Vec<u8, 96>`.
- **Relationships**: Sent to host via diagnostics stream; optionally mirrored to SWO.
- **Validation rules**: Timestamp monotonic; encoding must stay ≤128 bytes per line; use compact key=value format for REPL consumption.

## State Machines

### `StrapOrchestrator` FSM
- **States**: `Idle`, `Arming`, `Running`, `Cooldown`, `Error`.
- **Transitions**:
  - `Idle` → `Arming` when `CommandQueue` yields the next command and no other run is active.
  - `Arming` → `Running` after pre-sequence checks (e.g., for recovery ensure `REC` asserted 100 ms before `RESET`).
  - `Running` self-advances through `StrapStep` list using `embassy_time::Timer::after`, waiting on external triggers (e.g., bridge activity) when a step's completion requests it.
  - On success, `Running` → `Cooldown` enforcing template `cooldown`.
  - Any brown-out or queue collision pushes to `Error`, logging reason before returning to `Idle`.
- **Validation rules**: Only one active run; `CommandQueue` drained before new run; each transition logs telemetry.

### `JetsonPowerMonitor` Sampling Loop
- Periodic ADC conversions (e.g., every 10 ms) translate divider readings into LED on/off decisions.
- Applies hysteresis around the configured threshold to account for Jetson PWM dimming.
- Publishes events when state changes so telemetry reflects perceived host power status.

### `REPL Loop`
- Waits for CDC connection, then feeds incoming bytes to `ReplSession`.
- On completed line, runs lexer → parser → executor pipeline.
- Tab invokes `CompletionEngine`; command output and telemetry summaries echo back over the same channel.

### `RecoveryImmediate` Flow
- REPL command `recovery now` installs a wait with `BridgeActivityMonitor`, asserts the REC strap, and triggers the standard reboot sequence.
- `StrapOrchestrator` keeps REC asserted while `SequenceRun.waiting_on_bridge` is true.
- When Jetson console bytes arrive, `BridgeActivityMonitor` signals the orchestrator to release REC and mark the sequence complete; a watchdog timeout (e.g., 10 s) ensures REC eventually releases even without activity, emitting a warning telemetry event.

## Relationships Summary
- `StrapOrchestrator` consumes `CommandQueue` and emits strap timing telemetry.
- `ReplSession` sits atop the CDC transport, using `CommandGrammar`, `CompletionEngine`, and `CommandExecutor` to provide an interactive CLI.
- `UsbCdcBridgeTask` and `UartBridgeTask` coordinate through `BridgeQueue` to provide transparent UART bridging while allowing control channel traffic; `BridgeActivityMonitor` taps the RX stream to notify recovery sequences.
- `CommandQueue` ensures serialized operation per FR-005; `SequenceRun` enforces strap timings per BS-001..BS-003.
- Defmt logs provide the audit trail for strap changes, retries, telemetry updates, and queue activity.
