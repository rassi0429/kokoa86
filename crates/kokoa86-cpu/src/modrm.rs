use kokoa86_mem::{MemoryAccess, MemoryBus};
use crate::regs::{AddrSize, CpuState, CpuMode};

/// Decoded ModR/M result
#[derive(Debug, Clone, Copy)]
pub enum ModrmOperand {
    /// Register operand (index 0-7)
    Reg(u8),
    /// Memory operand with computed linear address
    Mem(u32),
}

/// Decode ModR/M with automatic address-size selection.
/// Returns (reg field, r/m operand, bytes consumed including modrm byte).
pub fn decode_modrm(
    cpu: &CpuState,
    mem: &MemoryBus,
    addr: u32,
    addr_size: AddrSize,
) -> (u8, ModrmOperand, u8) {
    match addr_size {
        AddrSize::Addr16 => decode_modrm16(cpu, mem, addr),
        AddrSize::Addr32 => decode_modrm32(cpu, mem, addr),
    }
}

/// Decode a ModR/M byte in 16-bit addressing mode.
pub fn decode_modrm16(
    cpu: &CpuState,
    mem: &MemoryBus,
    addr: u32,
) -> (u8, ModrmOperand, u8) {
    let modrm = mem.read_u8(addr);
    let mod_field = (modrm >> 6) & 0x03;
    let reg = (modrm >> 3) & 0x07;
    let rm = modrm & 0x07;

    if mod_field == 0x03 {
        return (reg, ModrmOperand::Reg(rm), 1);
    }

    let (base_addr, extra_bytes) = match mod_field {
        0x00 => {
            if rm == 0x06 {
                let disp = mem.read_u16(addr + 1);
                let linear = calc_linear16(cpu, cpu.ds, disp as u32);
                return (reg, ModrmOperand::Mem(linear), 3);
            }
            (calc_rm16_base(cpu, rm), 0u8)
        }
        0x01 => {
            let disp = mem.read_u8(addr + 1) as i8 as i16 as u16;
            (calc_rm16_base(cpu, rm).wrapping_add(disp as u32), 1)
        }
        0x02 => {
            let disp = mem.read_u16(addr + 1);
            (calc_rm16_base(cpu, rm).wrapping_add(disp as u32), 2)
        }
        _ => unreachable!(),
    };

    let seg = default_segment16(cpu, rm);
    let linear = calc_linear16(cpu, seg, base_addr);

    (reg, ModrmOperand::Mem(linear), 1 + extra_bytes)
}

/// Decode a ModR/M byte in 32-bit addressing mode (with SIB support).
pub fn decode_modrm32(
    cpu: &CpuState,
    mem: &MemoryBus,
    addr: u32,
) -> (u8, ModrmOperand, u8) {
    let modrm = mem.read_u8(addr);
    let mod_field = (modrm >> 6) & 0x03;
    let reg = (modrm >> 3) & 0x07;
    let rm = modrm & 0x07;

    if mod_field == 0x03 {
        return (reg, ModrmOperand::Reg(rm), 1);
    }

    let mut bytes_consumed: u8 = 1; // modrm byte itself
    let mut use_ss = false;

    let base_addr = if rm == 0x04 {
        // SIB byte follows
        let sib = mem.read_u8(addr + 1);
        bytes_consumed += 1;
        let scale = (sib >> 6) & 0x03;
        let index = (sib >> 3) & 0x07;
        let base = sib & 0x07;

        let base_val = if base == 5 && mod_field == 0x00 {
            // [disp32 + index*scale]
            let disp = mem.read_u32(addr + bytes_consumed as u32);
            bytes_consumed += 4;
            disp
        } else {
            if base == 5 || base == 4 {
                use_ss = base == 4; // ESP-based -> SS (actually EBP uses SS too)
            }
            if base == 5 { use_ss = true; }
            cpu.get_reg32(base)
        };

        let index_val = if index == 4 {
            0 // ESP can't be index
        } else {
            cpu.get_reg32(index) << scale
        };

        base_val.wrapping_add(index_val)
    } else if rm == 0x05 && mod_field == 0x00 {
        // [disp32]
        let disp = mem.read_u32(addr + 1);
        bytes_consumed += 4;
        disp
    } else {
        if rm == 5 { use_ss = true; } // EBP-based
        cpu.get_reg32(rm)
    };

    // Add displacement
    let final_addr = match mod_field {
        0x00 => base_addr,
        0x01 => {
            let disp = mem.read_u8(addr + bytes_consumed as u32) as i8 as i32 as u32;
            bytes_consumed += 1;
            base_addr.wrapping_add(disp)
        }
        0x02 => {
            let disp = mem.read_u32(addr + bytes_consumed as u32);
            bytes_consumed += 4;
            base_addr.wrapping_add(disp)
        }
        _ => unreachable!(),
    };

    // Apply segment base
    let seg = if use_ss { cpu.ss } else { cpu.ds };
    let linear = if cpu.mode == CpuMode::ProtectedMode {
        let cache = if use_ss { &cpu.ss_cache } else { &cpu.ds_cache };
        cache.base.wrapping_add(final_addr)
    } else {
        ((seg as u32) << 4).wrapping_add(final_addr)
    };

    (reg, ModrmOperand::Mem(linear), bytes_consumed)
}

// === Helpers ===

fn calc_rm16_base(cpu: &CpuState, rm: u8) -> u32 {
    match rm {
        0 => (cpu.ebx as u16).wrapping_add(cpu.esi as u16) as u32,
        1 => (cpu.ebx as u16).wrapping_add(cpu.edi as u16) as u32,
        2 => (cpu.ebp as u16).wrapping_add(cpu.esi as u16) as u32,
        3 => (cpu.ebp as u16).wrapping_add(cpu.edi as u16) as u32,
        4 => cpu.esi as u16 as u32,
        5 => cpu.edi as u16 as u32,
        6 => cpu.ebp as u16 as u32,
        7 => cpu.ebx as u16 as u32,
        _ => unreachable!(),
    }
}

fn default_segment16(cpu: &CpuState, rm: u8) -> u16 {
    match rm {
        2 | 3 | 6 => cpu.ss,
        _ => cpu.ds,
    }
}

fn calc_linear16(cpu: &CpuState, seg: u16, offset: u32) -> u32 {
    if cpu.mode == CpuMode::ProtectedMode {
        // In protected mode, use segment cache base
        // For simplicity, find which segment register matches
        let cache = if seg == cpu.ds { &cpu.ds_cache }
            else if seg == cpu.ss { &cpu.ss_cache }
            else if seg == cpu.es { &cpu.es_cache }
            else if seg == cpu.cs { &cpu.cs_cache }
            else { &cpu.ds_cache };
        cache.base.wrapping_add(offset & 0xFFFF)
    } else {
        ((seg as u32) << 4).wrapping_add(offset & 0xFFFF)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kokoa86_mem::MemoryBus;

    #[test]
    fn test_modrm32_register_direct() {
        let cpu = CpuState::default();
        let mut mem = MemoryBus::new(256);
        mem.load(0, &[0xC0]); // mod=11, reg=0, rm=0 (EAX)
        let (reg, rm, bytes) = decode_modrm32(&cpu, &mem, 0);
        assert_eq!(reg, 0);
        assert!(matches!(rm, ModrmOperand::Reg(0)));
        assert_eq!(bytes, 1);
    }

    #[test]
    fn test_modrm32_disp32() {
        let cpu = CpuState::default();
        let mut mem = MemoryBus::new(256);
        // mod=00, reg=0, rm=5 => [disp32]
        mem.load(0, &[0x05, 0x00, 0x10, 0x00, 0x00]); // [0x1000]
        let (reg, rm, bytes) = decode_modrm32(&cpu, &mem, 0);
        assert_eq!(reg, 0);
        assert!(matches!(rm, ModrmOperand::Mem(0x1000)));
        assert_eq!(bytes, 5);
    }

    #[test]
    fn test_modrm32_sib_base_index() {
        let mut cpu = CpuState::default();
        cpu.ebx = 0x100;
        cpu.ecx = 0x20;
        let mut mem = MemoryBus::new(256);
        // mod=00, reg=0, rm=4 (SIB follows)
        // SIB: scale=0, index=1(ECX), base=3(EBX) => [EBX + ECX]
        mem.load(0, &[0x04, 0x0B]);
        let (_, rm, bytes) = decode_modrm32(&cpu, &mem, 0);
        assert_eq!(bytes, 2);
        assert!(matches!(rm, ModrmOperand::Mem(0x120)));
    }

    #[test]
    fn test_modrm32_sib_scale() {
        let mut cpu = CpuState::default();
        cpu.eax = 0x100;
        cpu.ecx = 0x10;
        let mut mem = MemoryBus::new(256);
        // mod=00, reg=0, rm=4 (SIB)
        // SIB: scale=2 (x4), index=1(ECX), base=0(EAX) => [EAX + ECX*4]
        mem.load(0, &[0x04, 0x88]); // scale=10, index=001, base=000
        let (_, rm, _) = decode_modrm32(&cpu, &mem, 0);
        assert!(matches!(rm, ModrmOperand::Mem(0x140))); // 0x100 + 0x10*4
    }

    #[test]
    fn test_modrm32_reg_disp8() {
        let mut cpu = CpuState::default();
        cpu.ebx = 0x200;
        let mut mem = MemoryBus::new(256);
        // mod=01, reg=0, rm=3 (EBX + disp8)
        mem.load(0, &[0x43, 0x10]); // [EBX + 0x10]
        let (_, rm, bytes) = decode_modrm32(&cpu, &mem, 0);
        assert_eq!(bytes, 2);
        assert!(matches!(rm, ModrmOperand::Mem(0x210)));
    }
}
