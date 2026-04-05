use kokoa86_mem::{MemoryAccess, MemoryBus};
use crate::regs::CpuState;

/// Decoded ModR/M result
#[derive(Debug, Clone, Copy)]
pub enum ModrmOperand {
    /// Register operand (index 0-7)
    Reg(u8),
    /// Memory operand with computed linear address
    Mem(u32),
}

/// Decode a ModR/M byte in 16-bit addressing mode (real mode).
/// Returns (reg field, r/m operand, bytes consumed including modrm byte).
pub fn decode_modrm16(
    cpu: &CpuState,
    mem: &MemoryBus,
    addr: u32, // address of the ModR/M byte
) -> (u8, ModrmOperand, u8) {
    let modrm = mem.read_u8(addr);
    let mod_field = (modrm >> 6) & 0x03;
    let reg = (modrm >> 3) & 0x07;
    let rm = modrm & 0x07;

    if mod_field == 0x03 {
        // Register direct
        return (reg, ModrmOperand::Reg(rm), 1);
    }

    let (base_addr, extra_bytes) = match mod_field {
        0x00 => {
            if rm == 0x06 {
                // Special case: direct address
                let disp = mem.read_u16(addr + 1);
                return (reg, ModrmOperand::Mem(cpu.linear_addr(cpu.ds, disp)), 3);
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

    // Apply default segment
    let seg = default_segment16(cpu, rm);
    let linear = cpu.linear_addr(seg, base_addr as u16);

    (reg, ModrmOperand::Mem(linear), 1 + extra_bytes)
}

/// Calculate the base address from the R/M field in 16-bit mode
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

/// Default segment for 16-bit R/M addressing
fn default_segment16(cpu: &CpuState, rm: u8) -> u16 {
    match rm {
        2 | 3 | 6 => cpu.ss, // BP-based addressing uses SS
        _ => cpu.ds,
    }
}
