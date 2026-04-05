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
        self.devices.push(PciDevice {
            bus,
            device,
            function,
            config,
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
        isa[0x0E] = 0x80;                     // Header type: multi-function
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
                    0xFFFF_FFFF
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

                    match size {
                        1 => {
                            let addr = base + byte_offset as usize;
                            if addr < 256 {
                                dev.config[addr] = val as u8;
                            }
                        }
                        2 => {
                            let addr = base + byte_offset as usize;
                            let bytes = (val as u16).to_le_bytes();
                            if addr + 1 < 256 {
                                dev.config[addr] = bytes[0];
                                dev.config[addr + 1] = bytes[1];
                            }
                        }
                        _ => {
                            // 32-bit write to base register
                            let bytes = val.to_le_bytes();
                            if base + 3 < 256 {
                                dev.config[base] = bytes[0];
                                dev.config[base + 1] = bytes[1];
                                dev.config[base + 2] = bytes[2];
                                dev.config[base + 3] = bytes[3];
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
