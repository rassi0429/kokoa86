/// ATA/IDE Hard Disk Controller (PIO mode)
///
/// Primary IDE: ports 0x1F0-0x1F7, 0x3F6
///
/// Registers:
/// 0x1F0: Data (16-bit read/write)
/// 0x1F1: Error (read) / Features (write)
/// 0x1F2: Sector Count
/// 0x1F3: LBA Low (Sector Number)
/// 0x1F4: LBA Mid (Cylinder Low)
/// 0x1F5: LBA High (Cylinder High)
/// 0x1F6: Drive/Head (bit 6 = LBA mode, bit 4 = drive select)
/// 0x1F7: Status (read) / Command (write)
/// 0x3F6: Alternate Status (read) / Device Control (write)

use crate::port_bus::PortDevice;

/// Status register bits
const STATUS_BSY: u8 = 0x80;
const STATUS_DRDY: u8 = 0x40;
const STATUS_DRQ: u8 = 0x08;
const STATUS_ERR: u8 = 0x01;

/// ATA commands
const CMD_READ_SECTORS: u8 = 0x20;
const CMD_WRITE_SECTORS: u8 = 0x30;
const CMD_IDENTIFY: u8 = 0xEC;

#[derive(Debug, Clone)]
pub struct AtaDisk {
    /// Disk image data
    data: Vec<u8>,
    /// Sector size (always 512)
    sector_size: usize,
    /// Current status
    status: u8,
    /// Error register
    error: u8,
    /// Sector count register
    sector_count: u8,
    /// LBA registers
    lba_low: u8,
    lba_mid: u8,
    lba_high: u8,
    /// Drive/Head register
    drive_head: u8,
    /// Device control register
    device_control: u8,
    /// Data buffer for current transfer
    buffer: Vec<u8>,
    /// Position within buffer
    buffer_pos: usize,
    /// Whether buffer is for reading (true) or writing (false)
    buffer_read: bool,
    /// Sectors remaining in current operation
    sectors_remaining: u8,
    /// Current LBA for multi-sector ops
    current_lba: u32,
    /// IRQ14 pending
    pub irq14_pending: bool,
}

impl AtaDisk {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            sector_size: 512,
            status: STATUS_DRDY,
            error: 0,
            sector_count: 0,
            lba_low: 0,
            lba_mid: 0,
            lba_high: 0,
            drive_head: 0xA0,
            device_control: 0,
            buffer: Vec::new(),
            buffer_pos: 0,
            buffer_read: true,
            sectors_remaining: 0,
            current_lba: 0,
            irq14_pending: false,
        }
    }

    /// Load a disk image
    pub fn load_image(&mut self, data: Vec<u8>) {
        // Pad to sector boundary
        let sectors = (data.len() + 511) / 512;
        let mut padded = data;
        padded.resize(sectors * 512, 0);
        self.data = padded;
    }

    pub fn has_disk(&self) -> bool {
        !self.data.is_empty()
    }

    fn total_sectors(&self) -> u32 {
        (self.data.len() / self.sector_size) as u32
    }

    fn get_lba(&self) -> u32 {
        let lba = (self.lba_low as u32)
            | ((self.lba_mid as u32) << 8)
            | ((self.lba_high as u32) << 16)
            | (((self.drive_head & 0x0F) as u32) << 24);
        lba
    }

    fn execute_command(&mut self, cmd: u8) {
        match cmd {
            CMD_READ_SECTORS => {
                let lba = self.get_lba();
                let count = if self.sector_count == 0 { 256 } else { self.sector_count as u32 };

                if lba + count > self.total_sectors() {
                    self.status = STATUS_DRDY | STATUS_ERR;
                    self.error = 0x04; // abort
                    return;
                }

                // Load first sector into buffer
                self.current_lba = lba;
                self.sectors_remaining = (count - 1) as u8;
                self.load_sector(lba);
                self.status = STATUS_DRDY | STATUS_DRQ;
                self.irq14_pending = true;
            }
            CMD_WRITE_SECTORS => {
                let count = if self.sector_count == 0 { 256 } else { self.sector_count as u32 };
                self.current_lba = self.get_lba();
                self.sectors_remaining = (count - 1) as u8;
                self.buffer = vec![0u8; 512];
                self.buffer_pos = 0;
                self.buffer_read = false;
                self.status = STATUS_DRDY | STATUS_DRQ;
            }
            CMD_IDENTIFY => {
                if !self.has_disk() {
                    self.status = STATUS_ERR;
                    self.error = 0x04;
                    return;
                }

                let mut id = vec![0u8; 512];
                let total = self.total_sectors();

                // Word 0: General config
                id[0] = 0x40; id[1] = 0x00; // Fixed disk

                // Word 1: Number of cylinders
                let cyls = (total / (16 * 63)).min(16383) as u16;
                id[2] = cyls as u8; id[3] = (cyls >> 8) as u8;

                // Word 3: Number of heads
                id[6] = 16; id[7] = 0;

                // Word 6: Sectors per track
                id[12] = 63; id[13] = 0;

                // Words 27-46: Model string
                let model = b"kokoa86 Virtual Disk            ";
                for (i, &b) in model.iter().enumerate().take(40) {
                    // ATA swaps bytes in words
                    let word_offset = 27 * 2 + i;
                    if word_offset < 512 {
                        id[word_offset ^ 1] = b; // byte swap within word
                    }
                }

                // Word 49: Capabilities (LBA supported)
                id[98] = 0x00; id[99] = 0x02; // LBA

                // Word 60-61: Total sectors (LBA28)
                id[120] = (total & 0xFF) as u8;
                id[121] = ((total >> 8) & 0xFF) as u8;
                id[122] = ((total >> 16) & 0xFF) as u8;
                id[123] = ((total >> 24) & 0xFF) as u8;

                self.buffer = id;
                self.buffer_pos = 0;
                self.buffer_read = true;
                self.status = STATUS_DRDY | STATUS_DRQ;
                self.irq14_pending = true;
            }
            _ => {
                log::warn!("ATA: Unknown command 0x{:02X}", cmd);
                self.status = STATUS_DRDY | STATUS_ERR;
                self.error = 0x04; // abort
            }
        }
    }

    fn load_sector(&mut self, lba: u32) {
        let offset = lba as usize * self.sector_size;
        if offset + self.sector_size <= self.data.len() {
            self.buffer = self.data[offset..offset + self.sector_size].to_vec();
        } else {
            self.buffer = vec![0; self.sector_size];
        }
        self.buffer_pos = 0;
        self.buffer_read = true;
    }

    fn write_sector(&mut self, lba: u32) {
        let offset = lba as usize * self.sector_size;
        if offset + self.sector_size <= self.data.len() {
            self.data[offset..offset + self.sector_size].copy_from_slice(&self.buffer);
        }
    }
}

impl PortDevice for AtaDisk {
    fn port_in(&mut self, port: u16, size: u8) -> u32 {
        match port {
            0x1F0 => {
                // Data register (16-bit)
                if !self.buffer_read || self.buffer_pos >= self.buffer.len() {
                    return 0;
                }
                if size >= 2 && self.buffer_pos + 1 < self.buffer.len() {
                    let lo = self.buffer[self.buffer_pos] as u32;
                    let hi = self.buffer[self.buffer_pos + 1] as u32;
                    self.buffer_pos += 2;

                    // Check if sector transfer complete
                    if self.buffer_pos >= self.buffer.len() {
                        if self.sectors_remaining > 0 {
                            self.current_lba += 1;
                            self.sectors_remaining -= 1;
                            self.load_sector(self.current_lba);
                            self.irq14_pending = true;
                        } else {
                            self.status = STATUS_DRDY;
                        }
                    }

                    lo | (hi << 8)
                } else {
                    let b = self.buffer[self.buffer_pos] as u32;
                    self.buffer_pos += 1;
                    b
                }
            }
            0x1F1 => self.error as u32,
            0x1F2 => self.sector_count as u32,
            0x1F3 => self.lba_low as u32,
            0x1F4 => self.lba_mid as u32,
            0x1F5 => self.lba_high as u32,
            0x1F6 => self.drive_head as u32,
            0x1F7 | 0x3F6 => self.status as u32,
            _ => 0xFF,
        }
    }

    fn port_out(&mut self, port: u16, size: u8, val: u32) {
        match port {
            0x1F0 => {
                // Data write
                if self.buffer_read || self.buffer_pos >= self.buffer.len() {
                    return;
                }
                if size >= 2 && self.buffer_pos + 1 < self.buffer.len() {
                    self.buffer[self.buffer_pos] = val as u8;
                    self.buffer[self.buffer_pos + 1] = (val >> 8) as u8;
                    self.buffer_pos += 2;
                } else {
                    self.buffer[self.buffer_pos] = val as u8;
                    self.buffer_pos += 1;
                }

                if self.buffer_pos >= self.buffer.len() {
                    // Sector complete
                    self.write_sector(self.current_lba);
                    if self.sectors_remaining > 0 {
                        self.current_lba += 1;
                        self.sectors_remaining -= 1;
                        self.buffer = vec![0u8; 512];
                        self.buffer_pos = 0;
                    } else {
                        self.status = STATUS_DRDY;
                    }
                    self.irq14_pending = true;
                }
            }
            0x1F1 => {} // Features (ignore)
            0x1F2 => self.sector_count = val as u8,
            0x1F3 => self.lba_low = val as u8,
            0x1F4 => self.lba_mid = val as u8,
            0x1F5 => self.lba_high = val as u8,
            0x1F6 => self.drive_head = val as u8,
            0x1F7 => self.execute_command(val as u8),
            0x3F6 => self.device_control = val as u8,
            _ => {}
        }
    }

    fn port_range(&self) -> (u16, u16) {
        (0x1F0, 0x1F7)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_disk(sectors: usize) -> AtaDisk {
        let mut disk = AtaDisk::new();
        let mut data = vec![0u8; sectors * 512];
        // Write some recognizable data
        for i in 0..sectors {
            data[i * 512] = i as u8;
        }
        disk.load_image(data);
        disk
    }

    #[test]
    fn test_ata_identify() {
        let mut disk = make_disk(100);
        disk.port_out(0x1F6, 1, 0xE0); // drive 0, LBA
        disk.port_out(0x1F7, 1, CMD_IDENTIFY as u32);

        assert_eq!(disk.status & STATUS_DRQ, STATUS_DRQ);

        // Read 256 words (512 bytes)
        let mut data = Vec::new();
        for _ in 0..256 {
            let word = disk.port_in(0x1F0, 2);
            data.push(word as u8);
            data.push((word >> 8) as u8);
        }

        // Check total sectors (words 60-61)
        let total = data[120] as u32
            | (data[121] as u32) << 8
            | (data[122] as u32) << 16
            | (data[123] as u32) << 24;
        assert_eq!(total, 100);
    }

    #[test]
    fn test_ata_read_sector() {
        let mut disk = make_disk(10);

        // Read sector 0
        disk.port_out(0x1F2, 1, 1);    // 1 sector
        disk.port_out(0x1F3, 1, 0);    // LBA low
        disk.port_out(0x1F4, 1, 0);
        disk.port_out(0x1F5, 1, 0);
        disk.port_out(0x1F6, 1, 0xE0); // LBA mode, drive 0
        disk.port_out(0x1F7, 1, CMD_READ_SECTORS as u32);

        assert_eq!(disk.status & STATUS_DRQ, STATUS_DRQ);

        let first_word = disk.port_in(0x1F0, 2);
        assert_eq!(first_word & 0xFF, 0); // sector 0 starts with 0
    }

    #[test]
    fn test_ata_no_disk() {
        let mut disk = AtaDisk::new();
        disk.port_out(0x1F7, 1, CMD_IDENTIFY as u32);
        assert_eq!(disk.status & STATUS_ERR, STATUS_ERR);
    }
}
