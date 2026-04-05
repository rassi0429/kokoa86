/// Miscellaneous PC I/O port stubs
///
/// These ports are accessed during early BIOS initialization.
/// Most are simple stubs that prevent "unhandled port" warnings.

use crate::port_bus::PortDevice;

/// Port 0x80: POST (Power-On Self Test) diagnostic code
/// BIOS writes diagnostic codes here during boot. We log them.
pub struct PostPort {
    pub last_code: u8,
}

impl PostPort {
    pub fn new() -> Self {
        Self { last_code: 0 }
    }
}

impl PortDevice for PostPort {
    fn port_in(&mut self, _port: u16, _size: u8) -> u32 {
        self.last_code as u32
    }

    fn port_out(&mut self, _port: u16, _size: u8, val: u32) {
        self.last_code = val as u8;
        log::trace!("POST code: 0x{:02X}", self.last_code);
    }

    fn port_range(&self) -> (u16, u16) {
        (0x80, 0x80)
    }
}

/// Port 0x92: System Control Port A (Fast A20 gate)
/// Bit 1: A20 gate enable
/// Bit 0: Fast reset (write 1 = system reset)
pub struct SystemControlA {
    pub value: u8,
}

impl SystemControlA {
    pub fn new() -> Self {
        Self { value: 0x00 }
    }

    pub fn a20_enabled(&self) -> bool {
        self.value & 0x02 != 0
    }
}

impl PortDevice for SystemControlA {
    fn port_in(&mut self, _port: u16, _size: u8) -> u32 {
        self.value as u32
    }

    fn port_out(&mut self, _port: u16, _size: u8, val: u32) {
        self.value = val as u8 & 0xFE; // ignore bit 0 (reset)
        if val as u8 & 0x01 != 0 {
            log::warn!("System reset requested via port 0x92");
        }
    }

    fn port_range(&self) -> (u16, u16) {
        (0x92, 0x92)
    }
}

/// Port 0x61: System Control Port B (PC Speaker / Timer Gate)
/// Bit 0: Timer 2 gate
/// Bit 1: Speaker enable
/// Bit 4: Refresh detect (toggles)
/// Bit 5: Timer 2 output
pub struct SystemControlB {
    value: u8,
    refresh_toggle: bool,
}

impl SystemControlB {
    pub fn new() -> Self {
        Self {
            value: 0x00,
            refresh_toggle: false,
        }
    }
}

impl PortDevice for SystemControlB {
    fn port_in(&mut self, _port: u16, _size: u8) -> u32 {
        // Toggle refresh bit on each read
        self.refresh_toggle = !self.refresh_toggle;
        let mut v = self.value;
        if self.refresh_toggle {
            v |= 0x10; // refresh detect
        } else {
            v &= !0x10;
        }
        v as u32
    }

    fn port_out(&mut self, _port: u16, _size: u8, val: u32) {
        self.value = val as u8 & 0x0F; // only low 4 bits writable
    }

    fn port_range(&self) -> (u16, u16) {
        (0x61, 0x61)
    }
}

/// DMA Controller stub (8237)
/// Ports 0x00-0x0F (DMA1) and 0xC0-0xDF (DMA2)
/// Also page registers: 0x81-0x8F
/// SeaBIOS initializes these during POST.
pub struct DmaStub {
    regs: [u8; 16],
    page_regs: [u8; 16],
}

impl DmaStub {
    pub fn new() -> Self {
        Self {
            regs: [0; 16],
            page_regs: [0; 16],
        }
    }
}

impl PortDevice for DmaStub {
    fn port_in(&mut self, port: u16, _size: u8) -> u32 {
        match port {
            0x00..=0x0F => self.regs[port as usize] as u32,
            0x81..=0x8F => self.page_regs[(port - 0x81) as usize] as u32,
            0xC0..=0xDF => 0,
            _ => 0,
        }
    }

    fn port_out(&mut self, port: u16, _size: u8, val: u32) {
        match port {
            0x00..=0x0F => self.regs[port as usize] = val as u8,
            0x81..=0x8F => self.page_regs[(port - 0x81) as usize] = val as u8,
            0xC0..=0xDF => {} // ignore
            _ => {}
        }
    }

    fn port_range(&self) -> (u16, u16) {
        (0x00, 0x0F) // Main range; page regs handled separately
    }
}

/// DMA page register handler (0x81-0x8F)
pub struct DmaPageRegs {
    regs: [u8; 16],
}

impl DmaPageRegs {
    pub fn new() -> Self {
        Self { regs: [0; 16] }
    }
}

impl PortDevice for DmaPageRegs {
    fn port_in(&mut self, port: u16, _size: u8) -> u32 {
        self.regs[(port - 0x81) as usize] as u32
    }

    fn port_out(&mut self, port: u16, _size: u8, val: u32) {
        self.regs[(port - 0x81) as usize] = val as u8;
    }

    fn port_range(&self) -> (u16, u16) {
        (0x81, 0x8F)
    }
}

/// DMA2 stub (0xC0-0xDF)
pub struct Dma2Stub;

impl PortDevice for Dma2Stub {
    fn port_in(&mut self, _port: u16, _size: u8) -> u32 { 0 }
    fn port_out(&mut self, _port: u16, _size: u8, _val: u32) {}
    fn port_range(&self) -> (u16, u16) { (0xC0, 0xDF) }
}
