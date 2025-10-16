Here’s the Rev-B.2 complete BOM with your requested swaps:
	•	ESD → Nexperia IP4220CZ6 (TSOP-6, flow-through)
	•	LDO → Analog Devices ADP1715ARMZ-3.3 (MSOP-8)
	•	Straps → SN74LVC07APWR (hex open-drain), replacing the four discrete 2N7002s
	•	Footprints: 0805 everywhere for comfort, except 0603 for MCU decouplers and USB series/CC parts

⸻

ICs & Connectors

Ref	Qty	Manufacturer	MPN	Description	Package / Footprint
U1	1	ST	STM32G0B1KET6	Cortex-M0+, USB FS device, LQFP-32	LQFP-32 / LQFP-32_7x7mm_P0.80mm
U2	1	Analog Devices	ADP1715ARMZ-3.3-R7	3.3 V LDO, 500 mA, ceramic-stable (min 2.2 µF in/out)	MSOP-8 / MSOP-8
U3	1	TI	SN74LVC07APWR	Hex open-drain buffer, 1.65–5.5 V, Ioff	TSSOP-14 / TSSOP-14_4.4x5mm_P0.65mm
J1	1	GCT	USB4110-GF-A	USB-C 2.0 receptacle (16-pin)	USB-C RA SMT
D1	1	Nexperia	IP4220CZ6,125	Dual-line USB ESD, TSOP-6, flow-through	TSOP-6 / TSOT-23-6 (SOT-457)
J2	1	Samtec	SSW-112-02-F-S	1×12 socket, 2.54 mm (mates to Orin J14)	TH / PinSocket_1x12_P2.54mm_Vertical
J3	1	Samtec	FTSH-105-01-F-DV-K	SWD 2×5, 1.27 mm	PinHeader_2x05_P1.27mm_Vertical


⸻

USB & Power Passives

Ref	Qty	Value / Spec	Example MPN	Notes	Pkg / FP
RCC1,RCC2	2	5.1 kΩ ±1%	Yageo RC0603FR-075K1L	USB-C CC pull-downs (UFP)	0603
RDP,RDM	2	22 Ω ±1%	Yageo RC0603FR-0722RL	USB D± series (place near MCU)	0603
C_VBUS	1	1 µF, 10 V, X7R/X5R	Murata GRM21BR71A105KA01L	Local VBUS cap (keep total VBUS < 10 µF)	0805
C_3V3_BULK	1	10 µF, 6.3 V, X7R	Murata GRM21BR70J106KE76L	3.3 V bulk	0805
C_U2_IN, C_U2_OUT	2	2.2 µF, 6.3 V, X7R	Murata GRM21BR70J225KA73L	ADP1715 requires ≥ 2.2 µF on IN/OUT	0805


⸻

MCU Decoupling & Straps (control-side)

Ref	Qty	Value / Spec	Example MPN	Notes	Pkg
C_VDD1..C_VDD3	3	100 nF, 16 V, X7R	Murata GRM188R71C104KA01D	One per VDD, as tight as possible	0603
C_VDDA	1	100 nF	Murata GRM188R71C104KA01D	If your SKU uses VDDA	0603
C_VDDA_BULK	1	1 µF	Murata GRM21BR71A105KA01L	Analog bulk (DNP if not used)	0805
R_NRST	1	100 kΩ	Yageo RC0805FR-07100KL	NRST pull-up	0805
C_NRST	1	100 nF	Murata GRM21BR71C104KA01L	NRST RC to GND	0805
R_BOOT0	1	100 kΩ	Yageo RC0805FR-07100KL	BOOT0 pull-down	0805


⸻

Strap Driver (SN74LVC07A implementation)

Ref	Qty	Value / Spec	Example MPN	Notes	Pkg
C_U3_VCC	1	100 nF	Murata GRM188R71C104KA01D	Decouple U3 VCC (pin 14)	0603
R_A1..R_A4 (opt)	4	100 kΩ	Yageo RC0805FR-07100KL	Optional input pulldowns on A1..A4 (default-OFF)	0805

Outputs (Y1..Y4) → J14 straps: Y1→8 (RESET*), Y2→10 (REC*), Y3→12 (PWR*), Y4→5 (APO).
U3 GND → J14 ground; VCC = 3.3 V.

⸻

PC_LED+ Sense (status readout)

Ref	Qty	Value	Example MPN	Notes	Pkg
R_LED_HI	1	200 kΩ	Yageo RC0805FR-07200KL	J14-2 → ADC (≈1/3 scale)	0805
R_LED_LO	1	100 kΩ	Yageo RC0805FR-07100KL	ADC node → GND	0805
C_LED_FILT	1	1 nF	Murata GRM21BR71H102KA01L	RC filter at ADC node	0805


⸻

Local Buttons (parallel to straps)

Ref	Qty	Manufacturer	MPN	Wiring	Pkg
SW_RST	1	E-Switch	TL3342F260QG	Across J14-8 ↔ J14-7	SMD tact
SW_PWR	1	E-Switch	TL3342F260QG	Across J14-12 ↔ J14-11	SMD tact

(Any similar 6×3.5 mm low-profile tact is fine.)

⸻

Test Points (suggested)

Ref (suggested)	Nets
TP_VBUS, TP_3V3, TP_GND, TP_D+, TP_D−	
TP_NRST, TP_SWDIO, TP_SWCLK	
TP_UART_TX (to J14-3), TP_UART_RX (from J14-4)	
TP_PCLED_ADC	
TP_Y1..TP_Y4 (on LVC07 outputs, optional)	

Footprint: Keystone 5019 (Micro SMD test point) or 1.0 mm SMD pads.

⸻

CSV (drop-in)

Designator,Quantity,Manufacturer,MPN,Description,Package/Footprint,Notes
U1,1,ST,STM32G0B1KET6,MCU Cortex-M0+ USB FS,LQFP-32_7x7mm_P0.80mm,
U2,1,Analog Devices,ADP1715ARMZ-3.3-R7,LDO 3.3V 500mA ceramic-stable,MSOP-8,Use 2.2uF on IN/OUT
U3,1,Texas Instruments,SN74LVC07APWR,Hex open-drain buffer 1.65–5.5V,TSSOP-14,Outputs to J14 straps
J1,1,GCT,USB4110-GF-A,USB-C 2.0 receptacle 16-pin,USB-C RA SMT,
D1,1,Nexperia,IP4220CZ6,125,Dual USB ESD,TSOP-6 (SOT-457),Flow-through
J2,1,Samtec,SSW-112-02-F-S,Socket 1x12 2.54mm,TH,
J3,1,Samtec,FTSH-105-01-F-DV-K,Header 2x5 1.27mm SWD,TH/SMT,
RCC1 RCC2,2,Yageo,RC0603FR-075K1L,Res 5.1k 1% 100mW,0603,USB-C CC pull-downs
RDP RDM,2,Yageo,RC0603FR-0722RL,Res 22R 1% 100mW,0603,USB D+/D- series
C_VBUS,1,Murata,GRM21BR71A105KA01L,Cap 1uF 10V X7R,0805,VBUS local
C_3V3_BULK,1,Murata,GRM21BR70J106KE76L,Cap 10uF 6.3V X7R,0805,3V3 bulk
C_U2_IN C_U2_OUT,2,Murata,GRM21BR70J225KA73L,Cap 2.2uF 6.3V X7R,0805,ADP1715 input/output
C_VDD1 C_VDD2 C_VDD3,3,Murata,GRM188R71C104KA01D,Cap 100nF 16V X7R,0603,MCU decouplers
C_VDDA,1,Murata,GRM188R71C104KA01D,Cap 100nF 16V X7R,0603,If used
C_VDDA_BULK,1,Murata,GRM21BR71A105KA01L,Cap 1uF 10V X7R,0805,If used
R_NRST,1,Yageo,RC0805FR-07100KL,Res 100k 1%,0805,NRST pull-up
C_NRST,1,Murata,GRM21BR71C104KA01L,Cap 100nF 16V X7R,0805,NRST RC to GND
R_BOOT0,1,Yageo,RC0805FR-07100KL,Res 100k 1%,0805,BOOT0 pull-down
C_U3_VCC,1,Murata,GRM188R71C104KA01D,Cap 100nF 16V X7R,0603,LVC07 VCC decoupler
R_A1 R_A2 R_A3 R_A4,4,Yageo,RC0805FR-07100KL,Res 100k 1%,0805,Optional LVC07 input pulldowns
R_LED_HI,1,Yageo,RC0805FR-07200KL,Res 200k 1%,0805,PC_LED+ divider high
R_LED_LO,1,Yageo,RC0805FR-07100KL,Res 100k 1%,0805,PC_LED+ divider low
C_LED_FILT,1,Murata,GRM21BR71H102KA01L,Cap 1nF 50V X7R,0805,ADC RC filter
SW_RST,1,E-Switch,TL3342F260QG,Tact switch SPST SMD,SW_SPST_TL3342,Across J14-8↔7
SW_PWR,1,E-Switch,TL3342F260QG,Tact switch SPST SMD,SW_SPST_TL3342,Across J14-12↔11
TP_VBUS TP_3V3 TP_GND TP_D+ TP_D- TP_NRST TP_SWDIO TP_SWCLK TP_UART_TX TP_UART_RX TP_PCLED_ADC TP_Y1 TP_Y2 TP_Y3 TP_Y4,15,Keystone,5019,Micro SMD test point,TestPoint_Pad_D1.0mm,


⸻

Notes / reminders
	•	Place D1 (IP4220) immediately behind the USB-C and route D± straight through it; keep the stubs < 1 mm.
	•	ADP1715: keep 2.2 µF MLCCs right at IN/OUT pins; it’s happy with standard X7R/X5R ceramics.
	•	SN74LVC07A: one 100 nF at VCC; optionally add 100 k pulldowns on A1..A4 or enable MCU internal pulldowns for “default-OFF.”
	•	J14 straps: Y1→8↔GND(7), Y2→10↔GND(9), Y3→12↔GND(11), Y4→5→GND(6) (adjacent grounds minimize loop area).

If you want, I can also regenerate the KiCad schematic as Rev-B.2 with these exact parts/footprints dropped in so you can go straight to placement.