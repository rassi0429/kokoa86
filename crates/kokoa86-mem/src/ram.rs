use crate::bus::MemoryAccess;

pub struct Ram {
    data: Vec<u8>,
}

impl Ram {
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0; size],
        }
    }

    pub fn load(&mut self, offset: usize, bytes: &[u8]) {
        self.data[offset..offset + bytes.len()].copy_from_slice(bytes);
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }
}

impl MemoryAccess for Ram {
    fn read_u8(&self, addr: u32) -> u8 {
        self.data[addr as usize]
    }

    fn read_u16(&self, addr: u32) -> u16 {
        let i = addr as usize;
        u16::from_le_bytes([self.data[i], self.data[i + 1]])
    }

    fn read_u32(&self, addr: u32) -> u32 {
        let i = addr as usize;
        u32::from_le_bytes([
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ])
    }

    fn write_u8(&mut self, addr: u32, val: u8) {
        self.data[addr as usize] = val;
    }

    fn write_u16(&mut self, addr: u32, val: u16) {
        let i = addr as usize;
        let bytes = val.to_le_bytes();
        self.data[i] = bytes[0];
        self.data[i + 1] = bytes[1];
    }

    fn write_u32(&mut self, addr: u32, val: u32) {
        let i = addr as usize;
        let bytes = val.to_le_bytes();
        self.data[i] = bytes[0];
        self.data[i + 1] = bytes[1];
        self.data[i + 2] = bytes[2];
        self.data[i + 3] = bytes[3];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_write_u8() {
        let mut ram = Ram::new(16);
        ram.write_u8(0, 0x42);
        assert_eq!(ram.read_u8(0), 0x42);
    }

    #[test]
    fn test_little_endian_u16() {
        let mut ram = Ram::new(16);
        ram.write_u16(0, 0x1234);
        assert_eq!(ram.read_u8(0), 0x34); // little-endian: low byte first
        assert_eq!(ram.read_u8(1), 0x12);
        assert_eq!(ram.read_u16(0), 0x1234);
    }

    #[test]
    fn test_little_endian_u32() {
        let mut ram = Ram::new(16);
        ram.write_u32(0, 0xDEADBEEF);
        assert_eq!(ram.read_u8(0), 0xEF);
        assert_eq!(ram.read_u8(1), 0xBE);
        assert_eq!(ram.read_u8(2), 0xAD);
        assert_eq!(ram.read_u8(3), 0xDE);
        assert_eq!(ram.read_u32(0), 0xDEADBEEF);
    }

    #[test]
    fn test_load() {
        let mut ram = Ram::new(16);
        ram.load(4, &[0x01, 0x02, 0x03]);
        assert_eq!(ram.read_u8(4), 0x01);
        assert_eq!(ram.read_u8(5), 0x02);
        assert_eq!(ram.read_u8(6), 0x03);
    }
}
