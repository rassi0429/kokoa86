/// PCI Configuration Space
///
/// Ports:
/// - 0xCF8: CONFIG_ADDRESS (32-bit write) — selects bus/device/function/register
///   Format: bit 31 = enable, bits 23:16 = bus, bits 15:11 = device,
///           bits 10:8 = function, bits 7:2 = register, bits 1:0 = 0
/// - 0xCFC-0xCFF: CONFIG_DATA (32-bit read/write) — access selected register

use crate::port_bus::PortDevice;

struct PciDevice {
    bus: u8,
    device: u8,
    function: u8,
    config: [u8; 256],
    /// Write mask for config space: bit=1 means writable.
    /// BAR regions (0x10-0x27) use this to report correct BAR size.
    /// For BARs: mask = ~(size - 1) for the address bits.
    /// A mask of 0 for BAR means "BAR not present".
    writemask: [u8; 256],
}

pub struct PciBus {
    config_address: u32,
    devices: Vec<PciDevice>,
}

impl PciBus {
    pub fn new() -> Self {
        Self {
            config_address: 0,
            devices: Vec::new(),
        }
    }

    pub fn register_device(&mut self, bus: u8, device: u8, function: u8, config: [u8; 256]) {
        // Default writemask: most registers writable, but read-only fields masked
        let mut writemask = [0xFF_u8; 256];
        // Vendor ID (0x00-0x01): read-only
        writemask[0x00] = 0; writemask[0x01] = 0;
        // Device ID (0x02-0x03): read-only
        writemask[0x02] = 0; writemask[0x03] = 0;
        // Status (0x06-0x07): write-1-to-clear for some bits, mostly read-only
        writemask[0x06] = 0; writemask[0x07] = 0;
        // Revision (0x08): read-only
        writemask[0x08] = 0;
        // Class/Subclass/ProgIF (0x09-0x0B): read-only
        writemask[0x09] = 0; writemask[0x0A] = 0; writemask[0x0B] = 0;
        // Header type (0x0E): read-only
        writemask[0x0E] = 0;
        // BARs (0x10-0x27): set to 0 (no BARs for bridges)
        for i in 0x10..=0x27 {
            writemask[i] = 0;
        }
        self.devices.push(PciDevice {
            bus, device, function, config, writemask,
        });
    }

    /// Create default host bridge at 0:0:0 and ISA bridge at 0:1:0
    pub fn with_default_devices(mut self) -> Self {
        // Host bridge (device 0:0:0)
        let mut host = [0u8; 256];
        // Intel i440FX Host Bridge at 0:0:0
        // Vendor ID = 0x8086 (Intel), Device ID = 0x1237 (i440FX)
        host[0x00] = 0x86; host[0x01] = 0x80; // Vendor ID
        host[0x02] = 0x37; host[0x03] = 0x12; // Device ID
        host[0x04] = 0x06; host[0x05] = 0x00; // Command: mem+io
        host[0x06] = 0x00; host[0x07] = 0x00; // Status
        host[0x08] = 0x02;                     // Revision
        host[0x0A] = 0x00;                     // Subclass: Host bridge
        host[0x0B] = 0x06;                     // Class: Bridge
        host[0x0E] = 0x00;                     // Header type 0
        // PAM registers (0x59-0x5F): Programmable Attribute Map
        // Set all PAM regions to read-write (0x33 = read+write for both halves)
        for i in 0x59..=0x5F {
            host[i] = 0x33;
        }
        self.register_device(0, 0, 0, host);

        // Intel PIIX3 ISA Bridge at 0:1:0
        // Vendor ID = 0x8086, Device ID = 0x7000 (PIIX3)
        let mut isa = [0u8; 256];
        isa[0x00] = 0x86; isa[0x01] = 0x80;
        isa[0x02] = 0x00; isa[0x03] = 0x70;
        isa[0x04] = 0x07; isa[0x05] = 0x00; // Command
        isa[0x0A] = 0x01;                     // Subclass: ISA bridge
        isa[0x0B] = 0x06;                     // Class: Bridge
        isa[0x0E] = 0x00;                     // Header type: normal (no other functions)
        self.register_device(0, 1, 0, isa);

        self
    }

    /// Extract bus/device/function/register from config_address
    fn decode_address(&self) -> (bool, u8, u8, u8, u8) {
        let enable = self.config_address & 0x8000_0000 != 0;
        let bus = ((self.config_address >> 16) & 0xFF) as u8;
        let device = ((self.config_address >> 11) & 0x1F) as u8;
        let function = ((self.config_address >> 8) & 0x07) as u8;
        let register = (self.config_address & 0xFC) as u8; // bits 7:2, aligned to dword
        (enable, bus, device, function, register)
    }

    fn find_device(&self, bus: u8, device: u8, function: u8) -> Option<usize> {
        self.devices
            .iter()
            .position(|d| d.bus == bus && d.device == device && d.function == function)
    }
}

impl PortDevice for PciBus {
    fn port_in(&mut self, port: u16, size: u8) -> u32 {
        match port {
            0xCF8 => self.config_address,
            0xCFC..=0xCFF => {
                let (enable, bus, device, function, register) = self.decode_address();
                if !enable {
                    return 0xFFFF_FFFF;
                }

                let byte_offset = (port - 0xCFC) as u8;

                if let Some(idx) = self.find_device(bus, device, function) {
                    let dev = &self.devices[idx];
                    let base = register as usize;

                    match size {
                        1 => {
                            let addr = base + byte_offset as usize;
                            if addr < 256 {
                                dev.config[addr] as u32
                            } else {
                                0xFF
                            }
                        }
                        2 => {
                            let addr = base + byte_offset as usize;
                            if addr + 1 < 256 {
                                u16::from_le_bytes([dev.config[addr], dev.config[addr + 1]]) as u32
                            } else {
                                0xFFFF
                            }
                        }
                        _ => {
                            // 32-bit read from base register
                            if base + 3 < 256 {
                                u32::from_le_bytes([
                                    dev.config[base],
                                    dev.config[base + 1],
                                    dev.config[base + 2],
                                    dev.config[base + 3],
                                ])
                            } else {
                                0xFFFF_FFFF
                            }
                        }
                    }
                } else {
                    // Empty PCI slot: return 0xFF for most bytes,
                    // but header_type byte (0x0E) = 0x00 (non-multi-function)
                    // to prevent pci_next infinite loop on empty multi-function slots
                    let abs_offset = register as usize + byte_offset as usize;
                    match size {
                        1 => {
                            if abs_offset == 0x0E { 0x00 } else { 0xFF }
                        }
                        2 => {
                            let b0 = if abs_offset == 0x0E { 0x00u8 } else { 0xFF };
                            let b1 = if abs_offset + 1 == 0x0E { 0x00u8 } else { 0xFF };
                            u16::from_le_bytes([b0, b1]) as u32
                        }
                        _ => {
                            let mut bytes = [0xFFu8; 4];
                            for k in 0..4 {
                                if register as usize + k == 0x0E {
                                    bytes[k] = 0x00;
                                }
                            }
                            u32::from_le_bytes(bytes)
                        }
                    }
                }
            }
            _ => 0xFFFF_FFFF,
        }
    }

    fn port_out(&mut self, port: u16, size: u8, val: u32) {
        match port {
            0xCF8 => {
                self.config_address = val;
            }
            0xCFC..=0xCFF => {
                let (enable, bus, device, function, register) = self.decode_address();
                if !enable {
                    return;
                }

                let byte_offset = (port - 0xCFC) as u8;

                if let Some(idx) = self.find_device(bus, device, function) {
                    let dev = &mut self.devices[idx];
                    let base = register as usize;

                    // Apply writemask: only writable bits can change
                    match size {
                        1 => {
                            let addr = base + byte_offset as usize;
                            if addr < 256 {
                                let mask = dev.writemask[addr];
                                dev.config[addr] = (dev.config[addr] & !mask) | (val as u8 & mask);
                            }
                        }
                        2 => {
                            let addr = base + byte_offset as usize;
                            let bytes = (val as u16).to_le_bytes();
                            for i in 0..2 {
                                if addr + i < 256 {
                                    let mask = dev.writemask[addr + i];
                                    dev.config[addr + i] = (dev.config[addr + i] & !mask) | (bytes[i] & mask);
                                }
                            }
                        }
                        _ => {
                            let bytes = val.to_le_bytes();
                            for i in 0..4 {
                                if base + i < 256 {
                                    let mask = dev.writemask[base + i];
                                    dev.config[base + i] = (dev.config[base + i] & !mask) | (bytes[i] & mask);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn port_range(&self) -> (u16, u16) {
        (0xCF8, 0xCFF)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pci_config_read_vendor_id() {
        let mut pci = PciBus::new().with_default_devices();

        // Select device 0:0:0, register 0 (vendor/device ID)
        pci.port_out(0xCF8, 4, 0x8000_0000);

        // Read vendor+device ID (32-bit)
        let val = pci.port_in(0xCFC, 4);
        // Vendor=0x8086, Device=0x1237 (i440FX) -> little-endian dword
        assert_eq!(val, 0x1237_8086);
    }

    #[test]
    fn test_pci_empty_slot_returns_ffff() {
        let mut pci = PciBus::new().with_default_devices();

        // Select a non-existent device 0:31:0, register 0
        let addr: u32 = 0x8000_0000 | (31 << 11);
        pci.port_out(0xCF8, 4, addr);

        let val = pci.port_in(0xCFC, 4);
        assert_eq!(val, 0xFFFF_FFFF);
    }

    #[test]
    fn test_pci_config_write_read() {
        let mut pci = PciBus::new().with_default_devices();

        // Select ISA bridge 0:1:0, register 0x40 (a writable area)
        let addr: u32 = 0x8000_0000 | (1 << 11) | 0x40;
        pci.port_out(0xCF8, 4, addr);

        // Write a value
        pci.port_out(0xCFC, 4, 0xDEAD_BEEF);

        // Read it back
        let val = pci.port_in(0xCFC, 4);
        assert_eq!(val, 0xDEAD_BEEF);
    }
}


#[cfg(test)]
mod test_extra {
    use super::*;
    #[test]
    fn test_pci_readw_device_id_via_cfe() {
        let mut pci = PciBus::new().with_default_devices();
        pci.port_out(0xCF8, 4, 0x80000800);
        let vendor = pci.port_in(0xCFC, 2);
        assert_eq!(vendor, 0x8086, "vendor");
        let device = pci.port_in(0xCFE, 2);
        assert_eq!(device, 0x7000, "device ID via port 0xCFE");
    }
}

#[cfg(test)]
mod test_empty {
    use super::*;
    #[test]
    fn test_empty_slot_header_type() {
        let mut pci = PciBus::new().with_default_devices();
        // Select empty device 2, register 0x0C
        pci.port_out(0xCF8, 4, 0x80001000 | 0x0C);
        // Read byte at offset 0x0E (port 0xCFC + 2 = 0xCFE)
        let ht = pci.port_in(0xCFE, 1);
        assert_eq!(ht, 0x00, "empty slot header_type should be 0x00 (non-multi)");
        // Read dword at register 0x0C
        pci.port_out(0xCF8, 4, 0x80001000 | 0x0C);
        let dw = pci.port_in(0xCFC, 4);
        assert_eq!((dw >> 16) & 0xFF, 0x00, "header_type byte in dword should be 0x00");
    }
}
