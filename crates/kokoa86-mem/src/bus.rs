pub trait MemoryAccess {
    fn read_u8(&self, addr: u32) -> u8;
    fn read_u16(&self, addr: u32) -> u16;
    fn read_u32(&self, addr: u32) -> u32;
    fn write_u8(&mut self, addr: u32, val: u8);
    fn write_u16(&mut self, addr: u32, val: u16);
    fn write_u32(&mut self, addr: u32, val: u32);
}

pub struct MemoryBus {
    /// Flat memory for Phase 1 (1MB real-mode addressable)
    ram: crate::Ram,
}

impl MemoryBus {
    pub fn new(ram_size: usize) -> Self {
        Self {
            ram: crate::Ram::new(ram_size),
        }
    }

    pub fn load(&mut self, offset: usize, bytes: &[u8]) {
        self.ram.load(offset, bytes);
    }

    /// Read a slice of bytes (for instruction fetch, etc.)
    pub fn read_bytes(&self, addr: u32, buf: &mut [u8]) {
        for (i, byte) in buf.iter_mut().enumerate() {
            *byte = self.ram.read_u8(addr.wrapping_add(i as u32));
        }
    }
}

impl MemoryAccess for MemoryBus {
    fn read_u8(&self, addr: u32) -> u8 {
        self.ram.read_u8(addr)
    }

    fn read_u16(&self, addr: u32) -> u16 {
        self.ram.read_u16(addr)
    }

    fn read_u32(&self, addr: u32) -> u32 {
        self.ram.read_u32(addr)
    }

    fn write_u8(&mut self, addr: u32, val: u8) {
        self.ram.write_u8(addr, val);
    }

    fn write_u16(&mut self, addr: u32, val: u16) {
        self.ram.write_u16(addr, val);
    }

    fn write_u32(&mut self, addr: u32, val: u32) {
        self.ram.write_u32(addr, val);
    }
}
