# Component Checklist - Orin Nano Controller

This checklist tracks component implementation status against the BASELINE.md specification.

## Legend
- ✅ Complete and correct
- ⚠️ Partially complete (e.g., component present but missing footprint)
- ❌ Missing or incorrect
- 🔍 Needs verification

---

## ICs & Connectors

| Status | Ref | Qty | Manufacturer | MPN | Description | Package | Location |
|--------|-----|-----|--------------|-----|-------------|---------|----------|
| ⚠️ | U1 | 1 | ST | STM32G0B1KET6 | MCU Cortex-M0+ | LQFP-32 | controller.kicad_sch |
| ❌ | U2 | 1 | Analog Devices | ADP1715ARMZ-3.3-R7 | LDO 3.3V | MSOP-8 | power.kicad_sch (value error) |
| ❌ | U3 | 1 | TI | SN74LVC07APWR | Hex buffer | TSSOP-14 | **MISSING** (using 2x dual instead) |
| ⚠️ | J1 | 1 | GCT | USB4110-GF-A | USB-C receptacle | USB-C RA | usb.kicad_sch (generic type) |
| ⚠️ | D1 | 1 | Nexperia | IP4220CZ6,125 | USB ESD | TSOP-6 | usb.kicad_sch |
| ⚠️ | J2 | 1 | Samtec | SSW-112-02-F-S | 1×12 socket | TH 2.54mm | controller.kicad_sch (J3 in sch) |
| ⚠️ | J3 | 1 | Samtec | FTSH-105-01-F-DV-K | SWD 2×5 | 1.27mm | controller.kicad_sch (J2 in sch) |

**Notes:**
- U2 value field shows "a" instead of part number
- U3 (hex buffer) missing; currently using IC1/IC2 (dual buffers)
- Connector references J2/J3 are swapped between baseline and schematic

---

## USB & Power Passives

| Status | Ref | Qty | Value | Spec | Notes | Pkg | Location |
|--------|-----|-----|-------|------|-------|-----|----------|
| ⚠️ | RCC1, RCC2 | 2 | 5.1 kΩ | ±1% 100mW | USB-C CC pull-downs | 0603 | usb.kicad_sch |
| ❌ | RDP, RDM | 2 | 22 Ω | ±1% 100mW | USB D± series | 0603 | **MISSING** |
| ⚠️ | C_VBUS | 1 | 1 µF | 10V X7R | VBUS local cap | 0805 | usb.kicad_sch (C_VBUS1) |
| ❌ | C_3V3_BULK | 1 | 10 µF | 6.3V X7R | 3.3V bulk cap | 0805 | **MISSING** |
| ⚠️ | C_U2_IN | 1 | 2.2 µF | 6.3V X7R | LDO input cap | 0805 | power.kicad_sch (C8) |
| ⚠️ | C_U2_OUT | 1 | 2.2 µF | 6.3V X7R | LDO output cap | 0805 | power.kicad_sch (C9) |

**Notes:**
- All components need footprint assignments
- RDP, RDM missing (should be near MCU on D+/D- lines)
- C_3V3_BULK missing (critical for power stability)

---

## MCU Decoupling & Reset

| Status | Ref | Qty | Value | Spec | Notes | Pkg | Location |
|--------|-----|-----|-------|------|-------|-----|----------|
| ⚠️ | C_VDD1-3 | 3 | 100 nF | 16V X7R | VDD decoupling | 0603 | controller.kicad_sch (C1-C3?) |
| 🔍 | C_VDDA | 1 | 100 nF | 16V X7R | VDDA decoupling | 0603 | **NEEDS VERIFICATION** |
| 🔍 | C_VDDA_BULK | 1 | 1 µF | 10V X7R | VDDA bulk | 0805 | **NEEDS VERIFICATION** |
| 🔍 | R_NRST | 1 | 100 kΩ | ±1% | NRST pull-up | 0805 | **NEEDS VERIFICATION** |
| 🔍 | C_NRST | 1 | 100 nF | 16V X7R | NRST RC to GND | 0805 | **NEEDS VERIFICATION** |
| 🔍 | R_BOOT0 | 1 | 100 kΩ | ±1% | BOOT0 pull-down | 0805 | **NEEDS VERIFICATION** |

**Notes:**
- C1-C6 exist but need verification of which VDD pins they serve
- VDDA caps may not be needed if ADC/analog features unused
- Reset and BOOT0 circuits need manual verification in schematic

---

## Strap Driver (Buffer IC)

| Status | Ref | Qty | Value | Spec | Notes | Pkg | Location |
|--------|-----|-----|-------|------|-------|-----|----------|
| ❌ | C_U3_VCC | 1 | 100 nF | 16V X7R | U3 VCC decoupling | 0603 | **MISSING** |
| ❌ | R_A1-A4 | 4 | 100 kΩ | ±1% (optional) | Input pulldowns | 0805 | **MISSING** |

**Notes:**
- Currently using IC1, IC2 (SN74LVC2G07DBVR) instead of U3 (SN74LVC07APWR)
- Need 4 outputs total: Y1→J14-8, Y2→J14-10, Y3→J14-12, Y4→J14-5
- RA2-RA5 exist (100kΩ) but may not be in correct positions

---

## PC_LED+ Sense Circuit

| Status | Ref | Qty | Value | Spec | Notes | Pkg | Location |
|--------|-----|-----|-------|------|-------|-----|----------|
| ❌ | R_LED_HI | 1 | 200 kΩ | ±1% | J14-2 → ADC | 0805 | **MISSING** |
| ❌ | R_LED_LO | 1 | 100 kΩ | ±1% | ADC → GND | 0805 | **MISSING** |
| ❌ | C_LED_FILT | 1 | 1 nF | 50V X7R | ADC RC filter | 0805 | **MISSING** |

**Notes:**
- Entire circuit missing
- Divider provides ~1/3 scale voltage sense
- Should connect to MCU ADC input

---

## Local Buttons

| Status | Ref | Qty | Manufacturer | MPN | Notes | Pkg | Location |
|--------|-----|-----|--------------|-----|-------|-----|----------|
| ❌ | SW_RST | 1 | E-Switch | TL3342F260QG | Across J14-8 ↔ 7 | SMD tact | **MISSING** |
| ❌ | SW_PWR | 1 | E-Switch | TL3342F260QG | Across J14-12 ↔ 11 | SMD tact | **MISSING** |

**Notes:**
- Both tactile switches missing
- Should be wired parallel to buffer outputs
- Enable manual control without USB connection

---

## Test Points

| Status | Ref | Qty | Nets | Pkg | Location |
|--------|-----|-----|------|-----|----------|
| ❌ | TP_VBUS | 1 | VBUS | SMD pad | **MISSING** |
| ❌ | TP_3V3 | 1 | +3V3 | SMD pad | **MISSING** |
| ❌ | TP_GND | 1 | GND | SMD pad | **MISSING** |
| ❌ | TP_D+ | 1 | USB_DP | SMD pad | **MISSING** |
| ❌ | TP_D- | 1 | USB_DM | SMD pad | **MISSING** |
| ❌ | TP_NRST | 1 | NRST | SMD pad | **MISSING** |
| ❌ | TP_SWDIO | 1 | SWDIO | SMD pad | **MISSING** |
| ❌ | TP_SWCLK | 1 | SWCLK | SMD pad | **MISSING** |
| ❌ | TP_UART_TX | 1 | UART_TX | SMD pad | **MISSING** |
| ❌ | TP_UART_RX | 1 | UART_RX | SMD pad | **MISSING** |
| ❌ | TP_PCLED_ADC | 1 | PC_LED+ sense | SMD pad | **MISSING** |
| ❌ | TP_Y1-Y4 | 4 | Strap outputs | SMD pad | **MISSING (optional)** |

**Notes:**
- All test points missing
- Recommended: Keystone 5019 or 1.0mm SMD pads
- Critical for debugging and production testing

---

## Summary Statistics

### By Status
- ✅ Complete: 0
- ⚠️ Partial: 10
- ❌ Missing: 29
- 🔍 Needs verification: 6

### By Category
- ICs & Connectors: 3/7 partial, 1/7 missing
- Passives: 4/6 partial, 2/6 missing
- MCU Support: 3/6 partial, 3/6 needs verification
- Strap Driver: 0/2 present, 2/2 missing
- LED Sense: 0/3 present, 3/3 missing
- Buttons: 0/2 present, 2/2 missing
- Test Points: 0/15 present, 15/15 missing

### Overall Completion
- **~22% fully complete** (components present with correct values and footprints)
- **~42% partially complete** (components present but missing footprints or have errors)
- **~36% missing** (components not in schematic at all)

---

## Action Items

### Critical (Must Fix)
1. [ ] Fix U2 value field error in power.kicad_sch
2. [ ] Add USB series resistors (RDP, RDM)
3. [ ] Add 3.3V bulk capacitor (C_3V3_BULK)
4. [ ] Assign footprints to all existing components
5. [ ] Resolve strap buffer implementation (IC1/IC2 vs U3)

### High Priority
6. [ ] Add PC_LED+ sense circuit (R_LED_HI, R_LED_LO, C_LED_FILT)
7. [ ] Add tactile switches (SW_RST, SW_PWR)
8. [ ] Verify MCU reset circuit (R_NRST, C_NRST, R_BOOT0)
9. [ ] Verify VDDA decoupling (if using ADC)
10. [ ] Add buffer IC decoupling (C_U3_VCC)

### Medium Priority
11. [ ] Add test points for major nets
12. [ ] Verify all VDD pins have local 100nF caps
13. [ ] Standardize component naming convention
14. [ ] Update J1 to specific USB4110-GF-A part
15. [ ] Add input pulldowns for buffer ICs (R_A1-A4)

### Low Priority
16. [ ] Add mounting holes
17. [ ] Add board outline
18. [ ] Add revision info to schematic
19. [ ] Add optional strap output test points

---

*Last Updated: 2025-10-17*
