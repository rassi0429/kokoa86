pub trait MemoryAccess {
    fn read_u8(&self, addr: u32) -> u8;
    fn read_u16(&self, addr: u32) -> u16;
    fn read_u32(&self, addr: u32) -> u32;
    fn write_u8(&mut self, addr: u32, val: u8);
    fn write_u16(&mut self, addr: u32, val: u16);
    fn write_u32(&mut self, addr: u32, val: u32);
}

pub struct MemoryBus {
    ram: crate::Ram,
    /// Optional ROM region (e.g., BIOS)
    /// Mapped at a specific address range, read-only from guest perspective
    rom: Option<RomRegion>,
}

struct RomRegion {
    base: u32,
    data: Vec<u8>,
}

impl MemoryBus {
    pub fn new(ram_size: usize) -> Self {
        Self {
            ram: crate::Ram::new(ram_size),
            rom: None,
        }
    }

    pub fn load(&mut self, offset: usize, bytes: &[u8]) {
        self.ram.load(offset, bytes);
    }

    /// Map a ROM at a specific base address.
    /// Reads in this range return ROM data; writes go to underlying RAM (shadowing).
    pub fn map_rom(&mut self, base: u32, data: Vec<u8>) {
        // Also copy to RAM so writes to ROM area land in RAM
        if (base as usize) + data.len() <= self.ram.size() {
            self.ram.load(base as usize, &data);
        }
        self.rom = Some(RomRegion { base, data });
    }

    /// Read a slice of bytes (for instruction fetch, etc.)
    pub fn read_bytes(&self, addr: u32, buf: &mut [u8]) {
        for (i, byte) in buf.iter_mut().enumerate() {
            *byte = self.read_u8(addr.wrapping_add(i as u32));
        }
    }

    /// Check if an address falls in ROM
    fn rom_read(&self, addr: u32) -> Option<u8> {
        if let Some(ref rom) = self.rom {
            let offset = addr.wrapping_sub(rom.base);
            if offset < rom.data.len() as u32 {
                return Some(rom.data[offset as usize]);
            }
        }
        None
    }
}

impl MemoryAccess for MemoryBus {
    fn read_u8(&self, addr: u32) -> u8 {
        if let Some(val) = self.rom_read(addr) {
            return val;
        }
        if (addr as usize) < self.ram.size() {
            self.ram.read_u8(addr)
        } else {
            0xFF
        }
    }

    fn read_u16(&self, addr: u32) -> u16 {
        // Check ROM for both bytes
        if self.rom.is_some() {
            if let (Some(lo), Some(hi)) = (self.rom_read(addr), self.rom_read(addr + 1)) {
                return u16::from_le_bytes([lo, hi]);
            }
        }
        if (addr as usize + 1) < self.ram.size() {
            self.ram.read_u16(addr)
        } else {
            let lo = self.read_u8(addr);
            let hi = self.read_u8(addr.wrapping_add(1));
            u16::from_le_bytes([lo, hi])
        }
    }

    fn read_u32(&self, addr: u32) -> u32 {
        if self.rom.is_some() {
            if let (Some(b0), Some(b1), Some(b2), Some(b3)) = (
                self.rom_read(addr), self.rom_read(addr + 1),
                self.rom_read(addr + 2), self.rom_read(addr + 3),
            ) {
                return u32::from_le_bytes([b0, b1, b2, b3]);
            }
        }
        if (addr as usize + 3) < self.ram.size() {
            self.ram.read_u32(addr)
        } else {
            let b0 = self.read_u8(addr);
            let b1 = self.read_u8(addr.wrapping_add(1));
            let b2 = self.read_u8(addr.wrapping_add(2));
            let b3 = self.read_u8(addr.wrapping_add(3));
            u32::from_le_bytes([b0, b1, b2, b3])
        }
    }

    fn write_u8(&mut self, addr: u32, val: u8) {
        if (addr as usize) < self.ram.size() {
            self.ram.write_u8(addr, val);
        }
    }

    fn write_u16(&mut self, addr: u32, val: u16) {
        if (addr as usize + 1) < self.ram.size() {
            self.ram.write_u16(addr, val);
        }
    }

    fn write_u32(&mut self, addr: u32, val: u32) {
        if (addr as usize + 3) < self.ram.size() {
            self.ram.write_u32(addr, val);
        }
    }
}
