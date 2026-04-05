/// QEMU fw_cfg device — provides firmware configuration to BIOS
///
/// Ports:
/// - 0x510: Selector (write 16-bit key to select an entry)
/// - 0x511: Data (read bytes sequentially from selected entry)

use crate::port_bus::PortDevice;
use std::collections::HashMap;

pub struct FwCfg {
    selector: u16,
    entries: HashMap<u16, Vec<u8>>,
    offset: usize,
}

impl FwCfg {
    pub fn new(ram_size: u64) -> Self {
        let mut fw = Self {
            selector: 0,
            entries: HashMap::new(),
            offset: 0,
        };
        fw.init(ram_size);
        fw
    }

    fn init(&mut self, ram_size: u64) {
        // Don't provide QEMU signature — force CMOS path for RAM
        // self.entries.insert(0x0000, b"QEMU".to_vec());

        // 0x0001: ID (interface version: bit 0 = traditional IO)
        self.entries.insert(0x0001, vec![0x01, 0x00, 0x00, 0x00]);

        // 0x0003: UUID
        self.entries.insert(0x0003, vec![0; 16]);

        // 0x0005: NUMA (0 nodes)
        self.entries.insert(0x0005, 8u64.to_le_bytes().to_vec()); // just 0

        // Build file directory for etc/e820 and etc/ram_size
        let ram_size_key: u16 = 0x8003;
        let e820_key: u16 = 0x8004;

        // etc/ram_size: 8 bytes u64 LE
        let ram_data = ram_size.to_le_bytes().to_vec();
        self.entries.insert(ram_size_key, ram_data.clone());

        // etc/e820: array of e820 entries
        // Each entry: u64 address, u64 size, u32 type (20 bytes each, no padding)
        let mut e820 = Vec::new();
        // Low memory (0 - 0x9FC00): usable
        e820.extend_from_slice(&0u64.to_le_bytes());       // address
        e820.extend_from_slice(&(0x9FC00u64).to_le_bytes()); // size
        e820.extend_from_slice(&1u32.to_le_bytes());        // type: RAM
        // High memory (1MB - ram_size): usable (if RAM > 1MB)
        if ram_size > 0x100000 {
            e820.extend_from_slice(&(0x100000u64).to_le_bytes());
            e820.extend_from_slice(&(ram_size - 0x100000).to_le_bytes());
            e820.extend_from_slice(&1u32.to_le_bytes());
        }
        self.entries.insert(e820_key, e820.clone());

        // File directory (key 0x0019)
        let mut dir = Vec::new();
        // Count (u32 BE)
        dir.extend_from_slice(&2u32.to_be_bytes());

        // File 1: etc/e820
        dir.extend_from_slice(&(e820.len() as u32).to_be_bytes());
        dir.extend_from_slice(&e820_key.to_be_bytes());
        dir.extend_from_slice(&0u16.to_be_bytes());
        let mut name1 = [0u8; 56];
        name1[..9].copy_from_slice(b"etc/e820\0");
        dir.extend_from_slice(&name1);

        // File 2: etc/ram_size
        dir.extend_from_slice(&(ram_data.len() as u32).to_be_bytes());
        dir.extend_from_slice(&ram_size_key.to_be_bytes());
        dir.extend_from_slice(&0u16.to_be_bytes());
        let mut name2 = [0u8; 56];
        name2[..14].copy_from_slice(b"etc/ram_size\0\0");
        dir.extend_from_slice(&name2);

        // etc/reserved-memory-end: u64 LE — marks the end of low memory
        let rsvd_end_key: u16 = 0x8005;
        self.entries.insert(rsvd_end_key, ram_size.to_le_bytes().to_vec());

        // Rebuild file directory with the extra entry
        // File 3: etc/reserved-memory-end
        let rsvd_data_len = 8u32; // u64
        dir.extend_from_slice(&rsvd_data_len.to_be_bytes());
        dir.extend_from_slice(&rsvd_end_key.to_be_bytes());
        dir.extend_from_slice(&0u16.to_be_bytes());
        let mut name3 = [0u8; 56];
        name3[..25].copy_from_slice(b"etc/reserved-memory-end\0\0");
        dir.extend_from_slice(&name3);

        // Update file count from 2 to 3
        let count = 3u32;
        dir[0] = (count >> 24) as u8;
        dir[1] = (count >> 16) as u8;
        dir[2] = (count >> 8) as u8;
        dir[3] = count as u8;

        self.entries.insert(0x0019, dir);
    }

    fn current_data(&self) -> &[u8] {
        self.entries.get(&self.selector).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

impl PortDevice for FwCfg {
    fn port_in(&mut self, port: u16, _size: u8) -> u32 {
        match port {
            0x510 => self.selector as u32,
            0x511 => {
                let data = self.current_data();
                if self.offset < data.len() {
                    let val = data[self.offset] as u32;
                    self.offset += 1;
                    val
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    fn port_out(&mut self, port: u16, size: u8, val: u32) {
        if port == 0x510 {
            // Selector can be written as 16-bit
            self.selector = if size >= 2 { val as u16 } else { val as u16 };
            self.offset = 0;
            let data_len = self.entries.get(&self.selector).map(|v| v.len()).unwrap_or(0);
            log::trace!("fw_cfg: SELECT key=0x{:04X} data_len={}", self.selector, data_len);
        }
    }

    fn port_range(&self) -> (u16, u16) {
        (0x510, 0x511)
    }
}
