# Orin Nano Controller PCB Notes

## Jetson J14 Strap Control (`J2`)

Four Jetson boot/control straps land on the 12-pin right-angle socket (`J2`). They are driven by two SN74LVC2G07 dual open-drain buffers: `IC1` handles the APO and RESET lanes, `IC2` handles RECOVERY and POWER. The STM32 drives the buffer inputs on nets `OAPO`, `ORST`, `OREC`, and `OPWR`; each input has an on-board 100 kΩ pull-down (`RA0–RA3`) so the straps idle de-asserted while the MCU boots. The bench harness fans out to the Jetson front-panel header (`J14`, 2x7) using the cross-reference below.

| Jetson J14 pin | Jetson label | Controller J2 pin | Driver channel | MCU pin (net) | Active level | Harness notes |
| --- | --- | --- | --- | --- | --- | --- |
| 5 | `APO` (force shutdown) | 5 | `IC1` 1Y | PA5 (`OAPO`) | Assert by pulling low (MCU drives input high) | Ground return on J14-7 / J2-7 (GND) |
| 8 | `RESET*` | 8 | `IC1` 2Y | PA4 (`ORST`) | Assert low | Ground return on J14-9 / J2-9 (GND) |
| 10 | `REC*` (force recovery) | 10 | `IC2` 1Y | PA3 (`OREC`) | Assert low | Ground return on J14-11 / J2-11 (GND) |
| 12 | `PWR*` (power button) | 12 | `IC2` 2Y | PA2 (`OPWR`) | Assert low (200 ms pulse) | Shares ground return on J14-11 / J2-11 (GND) |

- Open-drain outputs only sink current; the Jetson carrier (or cable) must provide the pull-ups.
- Supply pin 5 of both buffers ties to the local `+3V3` rail.

## USB-C Front-End (`J3`)

- Receptacle: GCT USB4085-series (`J3`), wired as a USB 2.0 UFP. Shield (`S1`) bleeds to GND through `R1` (1 MΩ) while staying isolated from signal ground.
- CC pins: Both CC1 (`A5`) and CC2 (`B5`) populate 5.1 kΩ pull-downs (`RCC1`, `RCC0`) to advertise Rd as an upstream-facing device.
- Data path: D+ (A6/B6) and D− (A7/B7) run straight through the flow-through TVS array `D1` (Nexperia IP4220CZ6) before continuing on nets `/Controller/USB_DP` and `/Controller/USB_DM` into the STM32 pins PA12 and PA11. There are no discrete series resistors; route lengths rely on the ESD IC sitting directly behind the connector.
- VBUS: Pins A4/A9/B4/B9 feed `/Frontend/VBUS_RAW`, then through net tie `NT1` onto the `+5V` rail that powers the LDO. Place `D1` as close to the receptacle as possible to keep the stubs under ~1 mm, preserving the intended flow-through layout.

## Power Tree

- Source: USB VBUS (after `NT1`) becomes the `+5V` net. That net feeds the TLV75533 LDO (`U3`) input (pin 1) and its enable (pin 3), so the regulator is enabled whenever VBUS is present.
- Regulation: `U3` outputs the local 3.3 V rail on pin 5 (labelled `+3V3`, functionally the board’s `VDD_3V3`). Output decoupling is a 4.7 µF bulk cap (`C5`) plus local 100 nF/1 µF/100 nF caps (`C6`, `C7`, `C8`). Input decoupling uses 1 µF capacitors (`C2`, `C4`) on `+5V`.
- Load guidance: TLV75533P supports up to 500 mA continuous, but keep total draw (MCU, strap drivers, USB peripherals, downstream headers) comfortably below that to maintain headroom on USB-powered systems. The controller itself only needs on the order of a few tens of milliamps; reserve the remainder for anything you hang off J14.

## PC\_LED Sense Path

- The satellite harness brings the host “PC\_LED±” differential pair onto J14 pins 1–2 (`J2` pins 1–2). These drive the LED input of optocoupler `U1` (TLP185).
- The optocoupler transistor (pins 4/6) pulls STM32 pin PB2 low when lit. PB2 has a 10 kΩ pull-up to `+3V3` (`R3`), so:
  - LED off ⇒ PB2 ≈ 3.3 V (logic-high level for ADC/GPIO).
  - LED on with ≥1 mA LED current ⇒ PB2 pulled near ground (~0.2 V), sinking ≈0.33 mA through `R3`.
- There is no additional RC filtering; add digital filtering in firmware if the host LED toggles rapidly.

## SWD Header (`J1`, Arm 10-pin)

| Pin | Signal | Notes |
| --- | --- | --- |
| 1 | `+3V3` (VTref) | Target supply reference |
| 2 | `SWDIO` | To STM32 PA13 |
| 3 | GND | |
| 4 | `SWCLK` | To STM32 PA14 |
| 5 | GND | |
| 6 | `SWO` | To STM32 PB3 |
| 7 | KEY | No pin |
| 8 | NC/TDI | Not connected |
| 9 | GNDDetect | Hard-tied to GND |
| 10 | `~RESET` | To MCU `NRST` |

Take VTref from the on-board 3.3 V rail so external debuggers track the correct I/O voltage.
