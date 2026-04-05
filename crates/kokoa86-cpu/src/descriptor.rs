/// GDT/LDT Segment Descriptor parsing
///
/// A segment descriptor is 8 bytes in the GDT/LDT:
///
/// ```text
/// Byte 0-1: Limit [15:0]
/// Byte 2-3: Base [15:0]
/// Byte 4:   Base [23:16]
/// Byte 5:   Access byte: P(1) DPL(2) S(1) Type(4)
/// Byte 6:   Flags(4) Limit[19:16](4): G(1) D/B(1) L(1) AVL(1) Limit[19:16]
/// Byte 7:   Base [31:24]
/// ```

use kokoa86_mem::{MemoryAccess, MemoryBus};
use crate::regs::SegmentCache;

/// Load a segment descriptor from the GDT.
/// `selector` is the segment selector value.
/// Returns the parsed SegmentCache.
pub fn load_descriptor(mem: &MemoryBus, gdt_base: u32, selector: u16) -> SegmentCache {
    let index = (selector & 0xFFF8) as u32; // mask out RPL and TI
    let desc_addr = gdt_base + index;

    let limit_low = mem.read_u16(desc_addr) as u32;
    let base_low = mem.read_u16(desc_addr + 2) as u32;
    let base_mid = mem.read_u8(desc_addr + 4) as u32;
    let access = mem.read_u8(desc_addr + 5);
    let flags_limit_hi = mem.read_u8(desc_addr + 6);
    let base_hi = mem.read_u8(desc_addr + 7) as u32;

    let base = (base_hi << 24) | (base_mid << 16) | base_low;
    let limit_hi = (flags_limit_hi & 0x0F) as u32;
    let mut limit = (limit_hi << 16) | limit_low;

    let granularity = flags_limit_hi & 0x80 != 0;
    if granularity {
        limit = (limit << 12) | 0xFFF;
    }

    let big = flags_limit_hi & 0x40 != 0; // D/B bit
    let present = access & 0x80 != 0;
    let dpl = (access >> 5) & 0x03;
    let flags = (flags_limit_hi >> 4) & 0x0F;

    SegmentCache {
        selector,
        base,
        limit,
        access,
        flags,
        dpl,
        big,
        present,
    }
}

/// Load a descriptor and apply it to the appropriate segment cache in CPU state.
pub fn load_segment(
    cpu: &mut crate::regs::CpuState,
    mem: &MemoryBus,
    seg_idx: u8,
    selector: u16,
) {
    if cpu.mode == crate::regs::CpuMode::RealMode {
        // In real mode, just set the selector (no descriptor loading)
        cpu.set_sreg(seg_idx, selector);
        return;
    }

    let cache = if selector & 0xFFFC == 0 {
        // Null selector
        SegmentCache {
            selector,
            base: 0,
            limit: 0,
            access: 0,
            flags: 0,
            dpl: 0,
            big: false,
            present: false,
        }
    } else {
        load_descriptor(mem, cpu.gdtr.base, selector)
    };

    cpu.set_sreg(seg_idx, selector);
    match seg_idx {
        0 => cpu.es_cache = cache,
        1 => cpu.cs_cache = cache,
        2 => cpu.ss_cache = cache,
        3 => cpu.ds_cache = cache,
        4 => cpu.fs_cache = cache,
        5 => cpu.gs_cache = cache,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kokoa86_mem::MemoryBus;

    #[test]
    fn test_parse_code_segment() {
        let mut mem = MemoryBus::new(8192);
        // GDT at 0x1000, entry at index 1 (offset 8)
        // Code segment: base=0, limit=0xFFFFF, G=1, D=1, P=1, DPL=0, code+read
        let desc: [u8; 8] = [
            0xFF, 0xFF, // limit low = 0xFFFF
            0x00, 0x00, // base low = 0
            0x00,       // base mid = 0
            0x9A,       // access: P=1, DPL=0, S=1, code+read (1 00 1 1010)
            0xCF,       // G=1, D=1, L=0, AVL=0, limit hi=0xF
            0x00,       // base hi = 0
        ];
        mem.load(0x1008, &desc);

        let cache = load_descriptor(&mem, 0x1000, 0x08);
        assert_eq!(cache.base, 0);
        assert_eq!(cache.limit, 0xFFFFFFFF); // 0xFFFFF * 4K + 0xFFF
        assert!(cache.big);
        assert!(cache.present);
        assert_eq!(cache.dpl, 0);
    }

    #[test]
    fn test_parse_data_segment() {
        let mut mem = MemoryBus::new(8192);
        // Data segment: base=0x10000, limit=0x1000 (no granularity)
        let desc: [u8; 8] = [
            0x00, 0x10, // limit low = 0x1000
            0x00, 0x00, // base low = 0
            0x01,       // base mid = 1 (base = 0x10000)
            0x92,       // P=1, DPL=0, S=1, data+writable
            0x40,       // G=0, D=1, limit hi=0
            0x00,       // base hi = 0
        ];
        mem.load(0x1010, &desc);

        let cache = load_descriptor(&mem, 0x1000, 0x10);
        assert_eq!(cache.base, 0x10000);
        assert_eq!(cache.limit, 0x1000);
        assert!(cache.big);
        assert!(cache.present);
    }
}
