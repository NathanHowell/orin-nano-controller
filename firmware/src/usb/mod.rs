//! Dual-CDC USB composite builder scaffolding.
//!
//! This module wires the USB device topology described in the feature plan:
//! one CDC ACM interface dedicated to the local operator REPL (CDC0) and a
//! second CDC ACM interface that provides a transparent UART bridge to the
//! Jetson console.  It exposes a small builder wrapper so the rest of the
//! firmware can request port handles without knowing the underlying Embassy
//! USB bookkeeping.

#![allow(dead_code)]

/// Logical identifier for each USB CDC interface exposed by the controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsbPortKind {
    /// Operator REPL exposed over CDC0.
    Repl,
    /// Transparent UART bridge exported over CDC1.
    Bridge,
}

#[cfg(target_os = "none")]
pub const MAX_PACKET_SIZE: u16 = 64;

#[cfg(target_os = "none")]
const CONTROL_BUFFER_LEN: usize = 64;
#[cfg(target_os = "none")]
const CONFIG_DESCRIPTOR_LEN: usize = 256;
#[cfg(target_os = "none")]
const BOS_DESCRIPTOR_LEN: usize = 256;
#[cfg(target_os = "none")]
const MSOS_DESCRIPTOR_LEN: usize = 256;

/// User-visible strings advertised in the USB descriptors.
#[derive(Clone, Copy, Debug)]
pub struct UsbDeviceStrings {
    /// Manufacturer string descriptor.
    pub manufacturer: &'static str,
    /// Product string descriptor.
    pub product: &'static str,
    /// Unique serial number string descriptor (optional).
    pub serial_number: Option<&'static str>,
    /// Label for the REPL interface.
    pub repl_interface: &'static str,
    /// Label for the UART bridge interface.
    pub bridge_interface: &'static str,
}

impl Default for UsbDeviceStrings {
    fn default() -> Self {
        Self {
            manufacturer: "Orin Controller",
            product: "Jetson Strap Manager",
            serial_number: None,
            repl_interface: "Operator REPL",
            bridge_interface: "Jetson UART Bridge",
        }
    }
}

/// Backing storage for the Embassy USB builder and CDC ACM classes.
#[cfg(target_os = "none")]
pub struct UsbDeviceStorage {
    control_buf: [u8; CONTROL_BUFFER_LEN],
    config_descriptor: [u8; CONFIG_DESCRIPTOR_LEN],
    bos_descriptor: [u8; BOS_DESCRIPTOR_LEN],
    msos_descriptor: [u8; MSOS_DESCRIPTOR_LEN],
    repl_state: embassy_usb::class::cdc_acm::State<'static>,
    bridge_state: embassy_usb::class::cdc_acm::State<'static>,
}

#[cfg(target_os = "none")]
impl UsbDeviceStorage {
    /// Creates a fresh storage bundle for the USB composite device.
    pub fn new() -> Self {
        Self {
            control_buf: [0; CONTROL_BUFFER_LEN],
            config_descriptor: [0; CONFIG_DESCRIPTOR_LEN],
            bos_descriptor: [0; BOS_DESCRIPTOR_LEN],
            msos_descriptor: [0; MSOS_DESCRIPTOR_LEN],
            repl_state: embassy_usb::class::cdc_acm::State::new(),
            bridge_state: embassy_usb::class::cdc_acm::State::new(),
        }
    }
}

/// Split handles for a CDC ACM interface.
#[cfg(target_os = "none")]
pub struct CdcAcmHandle<D: embassy_usb::driver::Driver<'static>> {
    kind: UsbPortKind,
    pub sender: embassy_usb::class::cdc_acm::Sender<'static, D>,
    pub receiver: embassy_usb::class::cdc_acm::Receiver<'static, D>,
    pub control: embassy_usb::class::cdc_acm::ControlChanged<'static>,
}

#[cfg(target_os = "none")]
impl<D> CdcAcmHandle<D>
where
    D: embassy_usb::driver::Driver<'static>,
{
    /// Returns the logical identity of this CDC ACM port.
    pub fn kind(&self) -> UsbPortKind {
        self.kind
    }

    /// Waits until the host enables both IN and OUT endpoints for this port.
    pub async fn wait_ready(&mut self) {
        embassy_futures::join::join(
            self.sender.wait_connection(),
            self.receiver.wait_connection(),
        )
        .await;
    }

    /// Returns `true` when the host has asserted DTR on this interface.
    pub fn dtr(&self) -> bool {
        self.sender.dtr()
    }
}

/// Wrapper that owns the dual CDC ACM interfaces and the resulting USB device.
#[cfg(target_os = "none")]
pub struct UsbComposite<D>
where
    D: embassy_usb::driver::Driver<'static>,
{
    pub device: embassy_usb::UsbDevice<'static, D>,
    repl: Option<CdcAcmHandle<D>>,
    bridge: Option<CdcAcmHandle<D>>,
}

#[cfg(target_os = "none")]
impl<D> UsbComposite<D>
where
    D: embassy_usb::driver::Driver<'static>,
{
    /// Creates the USB composite device exposing REPL and bridge CDC interfaces.
    pub fn new(
        driver: D,
        storage: &'static mut UsbDeviceStorage,
        strings: UsbDeviceStrings,
    ) -> Self {
        let mut config = embassy_usb::Config::new(0x1209, 0x0001);
        config.manufacturer = Some(strings.manufacturer);
        config.product = Some(strings.product);
        config.serial_number = strings.serial_number;
        config.max_packet_size_0 = MAX_PACKET_SIZE as u8;
        config.max_power = 250;
        config.supports_remote_wakeup = true;
        config.device_class = 0xEF;
        config.device_sub_class = 0x02;
        config.device_protocol = 0x01;
        config.composite_with_iads = true;

        let mut builder = embassy_usb::Builder::new(
            driver,
            config,
            &mut storage.config_descriptor,
            &mut storage.bos_descriptor,
            &mut storage.msos_descriptor,
            &mut storage.control_buf,
        );

        let repl = embassy_usb::class::cdc_acm::CdcAcmClass::new(
            &mut builder,
            &mut storage.repl_state,
            MAX_PACKET_SIZE,
        );
        let bridge = embassy_usb::class::cdc_acm::CdcAcmClass::new(
            &mut builder,
            &mut storage.bridge_state,
            MAX_PACKET_SIZE,
        );

        let (repl_tx, repl_rx, repl_ctrl) = repl.split_with_control();
        let (bridge_tx, bridge_rx, bridge_ctrl) = bridge.split_with_control();

        let device = builder.build();

        Self {
            device,
            repl: Some(CdcAcmHandle {
                kind: UsbPortKind::Repl,
                sender: repl_tx,
                receiver: repl_rx,
                control: repl_ctrl,
            }),
            bridge: Some(CdcAcmHandle {
                kind: UsbPortKind::Bridge,
                sender: bridge_tx,
                receiver: bridge_rx,
                control: bridge_ctrl,
            }),
        }
    }

    /// Takes ownership of the REPL CDC interface handles.
    pub fn take_repl(&mut self) -> Option<CdcAcmHandle<D>> {
        self.take_port(UsbPortKind::Repl)
    }

    /// Takes ownership of the bridge CDC interface handles.
    pub fn take_bridge(&mut self) -> Option<CdcAcmHandle<D>> {
        self.take_port(UsbPortKind::Bridge)
    }

    /// Takes ownership of the requested CDC interface handles.
    pub fn take_port(&mut self, kind: UsbPortKind) -> Option<CdcAcmHandle<D>> {
        match kind {
            UsbPortKind::Repl => self.repl.take(),
            UsbPortKind::Bridge => self.bridge.take(),
        }
    }
}

/// Host-side stub so `cargo test` builds without pulling in Embassy USB.
#[cfg(not(target_os = "none"))]
pub struct UsbDeviceStorage;

#[cfg(not(target_os = "none"))]
impl UsbDeviceStorage {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self
    }
}

/// Host-side stub representing the composite USB device.
#[cfg(not(target_os = "none"))]
pub struct UsbComposite<D> {
    pub device: (),
    _marker: core::marker::PhantomData<D>,
}

#[cfg(not(target_os = "none"))]
impl<D> UsbComposite<D> {
    pub fn new(_: D, _: &'static mut UsbDeviceStorage, _: UsbDeviceStrings) -> Self {
        Self {
            device: (),
            _marker: core::marker::PhantomData,
        }
    }

    pub fn take_repl(&mut self) -> Option<CdcAcmHandle<D>> {
        self.take_port(UsbPortKind::Repl)
    }

    pub fn take_bridge(&mut self) -> Option<CdcAcmHandle<D>> {
        self.take_port(UsbPortKind::Bridge)
    }

    pub fn take_port(&mut self, _kind: UsbPortKind) -> Option<CdcAcmHandle<D>> {
        None
    }
}

/// Host-side stub representing a CDC port handle.
#[cfg(not(target_os = "none"))]
pub struct CdcAcmHandle<D> {
    pub sender: (),
    pub receiver: (),
    pub control: (),
    _marker: core::marker::PhantomData<D>,
}

#[cfg(not(target_os = "none"))]
impl<D> CdcAcmHandle<D> {
    pub fn kind(&self) -> UsbPortKind {
        UsbPortKind::Repl
    }
}
