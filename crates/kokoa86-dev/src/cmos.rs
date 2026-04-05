/// CMOS/RTC (Real-Time Clock) — MC146818
///
/// Ports:
/// - 0x70: CMOS address register (write) + NMI mask (bit 7)
/// - 0x71: CMOS data register (read/write)
///
/// Standard CMOS registers:
/// 0x00: Seconds       0x07: Day of Month
/// 0x02: Minutes       0x08: Month
/// 0x04: Hours         0x09: Year
/// 0x06: Day of Week   0x32: Century
/// 0x0A: Status Register A
/// 0x0B: Status Register B
/// 0x0C: Status Register C
/// 0x0D: Status Register D
/// 0x0F: Shutdown Status
/// 0x10: Floppy Drive Type
/// 0x12: Hard Disk Type
/// 0x14: Equipment Byte
/// 0x15-0x16: Base Memory Size
/// 0x17-0x18: Extended Memory Size

use crate::port_bus::PortDevice;

#[derive(Debug, Clone)]
pub struct Cmos {
    /// Currently selected register index
    index: u8,
    /// CMOS RAM (128 bytes)
    data: [u8; 128],
    /// NMI mask
    nmi_disabled: bool,
}

impl Cmos {
    pub fn new(ram_kb: u16, ext_ram_kb: u16) -> Self {
        let mut cmos = Self {
            index: 0,
            data: [0; 128],
            nmi_disabled: false,
        };

        // Status Register A: normal update rate
        cmos.data[0x0A] = 0x26;
        // Status Register B: 24-hour mode, BCD
        cmos.data[0x0B] = 0x02;
        // Status Register C: no interrupts pending
        cmos.data[0x0C] = 0x00;
        // Status Register D: CMOS battery OK
        cmos.data[0x0D] = 0x80;

        // Equipment byte: VGA, no FPU, 1 floppy
        cmos.data[0x14] = 0x06;
        // Floppy type: none
        cmos.data[0x10] = 0x00;

        // Base memory (KB, usually 640)
        let base_kb = ram_kb.min(640);
        cmos.data[0x15] = base_kb as u8;
        cmos.data[0x16] = (base_kb >> 8) as u8;

        // Extended memory (above 1MB, in KB)
        cmos.data[0x17] = ext_ram_kb as u8;
        cmos.data[0x18] = (ext_ram_kb >> 8) as u8;
        // Also in registers 0x30-0x31 (same value, capped at 64MB)
        let ext_capped = ext_ram_kb.min(0xFFFF);
        cmos.data[0x30] = ext_capped as u8;
        cmos.data[0x31] = (ext_capped >> 8) as u8;

        // Set a default time
        cmos.data[0x00] = 0x00; // seconds
        cmos.data[0x02] = 0x00; // minutes
        cmos.data[0x04] = 0x12; // hours (12:00)
        cmos.data[0x06] = 0x01; // day of week (Sunday)
        cmos.data[0x07] = 0x01; // day of month
        cmos.data[0x08] = 0x01; // month (January)
        cmos.data[0x09] = 0x24; // year (2024 mod 100 = 24)
        cmos.data[0x32] = 0x20; // century (20)

        // Boot device: try HDD first
        cmos.data[0x3D] = 0x01;

        cmos
    }
}

impl PortDevice for Cmos {
    fn port_in(&mut self, port: u16, _size: u8) -> u32 {
        match port {
            0x70 => self.index as u32,
            0x71 => {
                let idx = self.index as usize;
                if idx < 128 {
                    self.data[idx] as u32
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    fn port_out(&mut self, port: u16, _size: u8, val: u32) {
        match port {
            0x70 => {
                self.nmi_disabled = val & 0x80 != 0;
                self.index = (val & 0x7F) as u8;
            }
            0x71 => {
                let idx = self.index as usize;
                if idx < 128 {
                    // Some registers are read-only
                    match idx {
                        0x0C | 0x0D => {} // read-only
                        _ => self.data[idx] = val as u8,
                    }
                }
            }
            _ => {}
        }
    }

    fn port_range(&self) -> (u16, u16) {
        (0x70, 0x71)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmos_read_time() {
        let cmos = Cmos::new(640, 0);
        let mut cmos = cmos;
        cmos.port_out(0x70, 1, 0x04); // select hours
        assert_eq!(cmos.port_in(0x71, 1), 0x12);
    }

    #[test]
    fn test_cmos_base_memory() {
        let mut cmos = Cmos::new(640, 1024);
        cmos.port_out(0x70, 1, 0x15);
        let lo = cmos.port_in(0x71, 1);
        cmos.port_out(0x70, 1, 0x16);
        let hi = cmos.port_in(0x71, 1);
        assert_eq!((hi << 8) | lo, 640);
    }

    #[test]
    fn test_cmos_ext_memory() {
        let mut cmos = Cmos::new(640, 15360);
        cmos.port_out(0x70, 1, 0x17);
        let lo = cmos.port_in(0x71, 1);
        cmos.port_out(0x70, 1, 0x18);
        let hi = cmos.port_in(0x71, 1);
        assert_eq!((hi << 8) | lo, 15360);
    }

    #[test]
    fn test_cmos_battery_ok() {
        let mut cmos = Cmos::new(640, 0);
        cmos.port_out(0x70, 1, 0x0D);
        assert_eq!(cmos.port_in(0x71, 1) & 0x80, 0x80);
    }
}
