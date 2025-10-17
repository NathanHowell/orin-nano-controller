# Schematic Hierarchy and Signal Flow

Visual reference for understanding the Orin Nano Controller design structure.

---

## 📐 Schematic Sheet Hierarchy

```
orin-nano-controller.kicad_sch (Root Sheet - Page 1)
│
├─── usb.kicad_sch (Frontend - Page 2)
│    │
│    ├── J1: USB-C Connector
│    ├── D1: ESD Protection (IP4220CZ6)
│    ├── RCC1, RCC2: CC Pull-downs (5.1kΩ)
│    └── C_VBUS1: VBUS Decoupling (1µF)
│
├─── controller.kicad_sch (Controller - Page 3)
│    │
│    ├── U1: STM32G0B1KETx (MCU)
│    ├── IC1, IC2: SN74LVC2G07DBVR (Strap Buffers)
│    ├── J2: SWD Debug Connector (10-pin)
│    ├── J3: Orin Nano Connector (12-pin)
│    ├── C1-C6: Decoupling Capacitors
│    └── RA2-RA5: Pull-down/Pull-up Resistors (100kΩ)
│
├─── power.kicad_sch (Power - Page 4)
│    │
│    ├── U2: ADP1715ARMZ-3.3-R7 (LDO)
│    ├── C7: Decoupling (10nF)
│    ├── C8: Input Cap (2.2µF)
│    └── C9: Output Cap (2.2µF)
│
└─── straps.kicad_sch (Straps - Page 5)
     └── [Currently Empty]
```

---

## 🔌 Signal Flow Diagram

### USB Data Path
```
USB-C Connector (J1)
    │
    ├── VBUS ──────────────┬─── C_VBUS1 (1µF) ──> +5V Rail
    │                      │
    │                      └─── U2 (LDO) ──> +3.3V Rail
    │
    ├── D+ ────── D1 (ESD) ────── [MISSING: 22Ω] ────── U1 (MCU)
    │                                                    USB_DP
    │
    ├── D- ────── D1 (ESD) ────── [MISSING: 22Ω] ────── U1 (MCU)
    │                                                    USB_DM
    │
    ├── CC1 ──── RCC1 (5.1kΩ) ──── GND
    │
    └── CC2 ──── RCC2 (5.1kΩ) ──── GND
```

### Power Distribution
```
VBUS (+5V from USB)
    │
    ├── C_VBUS1 (1µF) ──> GND
    │
    └── U2 (ADP1715) LDO Regulator
         │
         ├── IN  ─── C8 (2.2µF) ──> GND
         │
         └── OUT ─── C9 (2.2µF) ──> GND
              │
              ├── [MISSING: C_3V3_BULK 10µF] ──> GND
              │
              └── +3.3V Rail
                   │
                   ├── U1 VDD ─── C1, C2, C3 (100nF each) ──> GND
                   │
                   ├── [VERIFY: U1 VDDA ─── C_VDDA (100nF)] ──> GND
                   │
                   └── IC1, IC2 VCC ─── [MISSING: Decoupling] ──> GND
```

### MCU to Orin Nano Interface
```
STM32G0B1 (U1)
    │
    ├── GPIO Outputs ──> IC1, IC2 (Buffers)
    │                         │
    │                         └── Open-Drain Outputs
    │                              │
    │                              ├── Y1 ──> J3-8  (RESET*)
    │                              ├── Y2 ──> J3-10 (REC*)
    │                              ├── Y3 ──> J3-12 (PWR*)
    │                              └── Y4 ──> J3-5  (APO)
    │
    ├── UART_TX ──> J3-3 (to Orin)
    ├── UART_RX <── J3-4 (from Orin)
    │
    └── ADC ──> [MISSING: PC_LED+ Sense Circuit]
              (should connect to J3-2)
```

### Debug Interface
```
STM32G0B1 (U1)
    │
    ├── SWDIO ──> J2-2 (SWD Data)
    ├── SWCLK ──> J2-4 (SWD Clock)
    ├── NRST  ──> J2-10 (Reset)
    └── GND   ──> J2-3,5,9 (Ground)
```

---

## 🔗 Inter-Sheet Connections

### Signals Crossing Sheets

| Signal | From Sheet | To Sheet | Notes |
|--------|-----------|----------|-------|
| USB_DP | usb.kicad_sch | controller.kicad_sch | USB D+ data line |
| USB_DM | usb.kicad_sch | controller.kicad_sch | USB D- data line |
| +5V | usb.kicad_sch | power.kicad_sch | VBUS power |
| +3.3V | power.kicad_sch | controller.kicad_sch | Regulated power |
| GND | All sheets | All sheets | Common ground |

---

## 📊 Component Distribution by Sheet

| Sheet | ICs | Passives | Connectors | Total |
|-------|-----|----------|------------|-------|
| usb.kicad_sch | 1 (ESD) | 3 (R,C) | 1 | 5 |
| controller.kicad_sch | 3 (MCU, 2×Buffer) | 10 (R,C) | 2 | 15 |
| power.kicad_sch | 1 (LDO) | 3 (C) | 0 | 4 |
| straps.kicad_sch | 0 | 0 | 0 | 0 |
| **Total** | **5** | **16** | **3** | **24** |

---

## 🎯 J3 Connector Pinout (to Orin Nano J14)

| Pin | Signal | Direction | Function | Source |
|-----|--------|-----------|----------|--------|
| 1 | GND | - | Ground | Common |
| 2 | PC_LED+ | Input | LED status sense | [MISSING CIRCUIT] |
| 3 | UART_TX | Output | Debug serial out | U1 |
| 4 | UART_RX | Input | Debug serial in | U1 |
| 5 | APO | Output | Auto Power Off | IC2 (Y4) |
| 6 | GND | - | Ground | Common |
| 7 | GND | - | Ground | Common |
| 8 | RESET* | Output | Reset control | IC1 (Y1) |
| 9 | GND | - | Ground | Common |
| 10 | REC* | Output | Recovery mode | IC1/2 (Y2) |
| 11 | GND | - | Ground | Common |
| 12 | PWR* | Output | Power control | IC2 (Y3) |

**Note:** Strap signals (RESET*, REC*, PWR*, APO) are active-low open-drain.

---

## ⚠️ Missing Signal Paths

### Critical Missing Connections

1. **USB Series Resistors**
   ```
   [SHOULD BE]:
   D+ ──── D1 ──── RDP (22Ω) ──── U1_USB_DP
   D- ──── D1 ──── RDM (22Ω) ──── U1_USB_DM
   ```

2. **PC_LED+ Sense Circuit**
   ```
   [SHOULD BE]:
   J3-2 ──── R_LED_HI (200kΩ) ──┬──── U1_ADC_IN
                                 │
                           C_LED_FILT (1nF)
                                 │
                           R_LED_LO (100kΩ)
                                 │
                                GND
   ```

3. **Local Push Buttons**
   ```
   [SHOULD BE]:
   SW_RST: J3-8 ↔ J3-7 (parallel to Y1 output)
   SW_PWR: J3-12 ↔ J3-11 (parallel to Y3 output)
   ```

---

## 🔍 Power Budget Estimate

| Component | Typical Current | Max Current |
|-----------|----------------|-------------|
| U1 (STM32G0B1) | ~15 mA | ~50 mA |
| IC1, IC2 (Buffers) | ~1 mA | ~10 mA |
| Total Draw | ~16 mA | ~60 mA |

**LDO Capacity:** ADP1715 can supply 500 mA  
**Margin:** Excellent (8× typical, 8.3× max)

---

## 📌 Design Notes

### USB Data Path
- ESD protection is first component after connector ✅
- Series resistors missing (should be 22Ω near MCU) ❌
- Differential pair routing critical in PCB layout

### Power Supply
- LDO input/output caps meet minimum requirements ✅
- Bulk capacitor missing (10µF recommended) ❌
- Decoupling strategy good but needs completion

### Strap Signals
- Using 2× dual buffers (IC1, IC2) instead of 1× hex buffer
- Configuration provides 4 outputs as needed ✅
- Missing input pulldowns (optional but recommended) ⚠️

### Debug Interface
- Standard ARM 10-pin SWD connector ✅
- All necessary signals present
- Consider adding test points for easier probing

---

*Last Updated: 2025-10-17*
