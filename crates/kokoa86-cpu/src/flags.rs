use crate::regs::CpuState;

pub const FLAG_CF: u32 = 1 << 0;  // Carry
pub const FLAG_PF: u32 = 1 << 2;  // Parity
pub const FLAG_AF: u32 = 1 << 4;  // Auxiliary carry
pub const FLAG_ZF: u32 = 1 << 6;  // Zero
pub const FLAG_SF: u32 = 1 << 7;  // Sign
pub const FLAG_TF: u32 = 1 << 8;  // Trap
pub const FLAG_IF: u32 = 1 << 9;  // Interrupt enable
pub const FLAG_DF: u32 = 1 << 10; // Direction
pub const FLAG_OF: u32 = 1 << 11; // Overflow

pub fn get_flag(cpu: &CpuState, flag: u32) -> bool {
    (cpu.eflags & flag) != 0
}

pub fn set_flag(cpu: &mut CpuState, flag: u32, val: bool) {
    if val {
        cpu.eflags |= flag;
    } else {
        cpu.eflags &= !flag;
    }
}

fn parity(val: u8) -> bool {
    val.count_ones() % 2 == 0
}

/// Update flags after an ADD operation
pub fn update_flags_add(cpu: &mut CpuState, a: u32, b: u32, result: u64, width: u8) {
    let mask = match width {
        8 => 0xFF_u64,
        16 => 0xFFFF,
        32 => 0xFFFFFFFF,
        _ => unreachable!(),
    };
    let sign_bit = 1u64 << (width - 1);
    let res = result & mask;

    set_flag(cpu, FLAG_CF, result > mask);
    set_flag(cpu, FLAG_ZF, res == 0);
    set_flag(cpu, FLAG_SF, (res & sign_bit) != 0);
    set_flag(cpu, FLAG_OF, ((a as u64 ^ res) & (b as u64 ^ res) & sign_bit) != 0);
    set_flag(cpu, FLAG_PF, parity(res as u8));
    set_flag(cpu, FLAG_AF, ((a ^ b ^ res as u32) & 0x10) != 0);
}

/// Update flags after a SUB/CMP operation (a - b = result)
pub fn update_flags_sub(cpu: &mut CpuState, a: u32, b: u32, result: u64, width: u8) {
    let mask = match width {
        8 => 0xFF_u64,
        16 => 0xFFFF,
        32 => 0xFFFFFFFF,
        _ => unreachable!(),
    };
    let sign_bit = 1u64 << (width - 1);
    let res = result & mask;

    set_flag(cpu, FLAG_CF, (a as u64) < (b as u64)); // borrow
    set_flag(cpu, FLAG_ZF, res == 0);
    set_flag(cpu, FLAG_SF, (res & sign_bit) != 0);
    set_flag(cpu, FLAG_OF, ((a as u64 ^ b as u64) & (a as u64 ^ res) & sign_bit) != 0);
    set_flag(cpu, FLAG_PF, parity(res as u8));
    set_flag(cpu, FLAG_AF, ((a ^ b ^ res as u32) & 0x10) != 0);
}

/// Update flags after a logic operation (AND, OR, XOR) — CF=0, OF=0
pub fn update_flags_logic(cpu: &mut CpuState, result: u32, width: u8) {
    let sign_bit = 1u32 << (width - 1);
    let mask = match width {
        8 => 0xFF_u32,
        16 => 0xFFFF,
        32 => 0xFFFFFFFF,
        _ => unreachable!(),
    };
    let res = result & mask;

    set_flag(cpu, FLAG_CF, false);
    set_flag(cpu, FLAG_OF, false);
    set_flag(cpu, FLAG_ZF, res == 0);
    set_flag(cpu, FLAG_SF, (res & sign_bit) != 0);
    set_flag(cpu, FLAG_PF, parity(res as u8));
}

/// Check a condition code (for Jcc instructions)
pub fn check_condition(cpu: &CpuState, cc: u8) -> bool {
    match cc {
        0x0 => get_flag(cpu, FLAG_OF),                                    // O
        0x1 => !get_flag(cpu, FLAG_OF),                                   // NO
        0x2 => get_flag(cpu, FLAG_CF),                                    // B/C/NAE
        0x3 => !get_flag(cpu, FLAG_CF),                                   // NB/NC/AE
        0x4 => get_flag(cpu, FLAG_ZF),                                    // E/Z
        0x5 => !get_flag(cpu, FLAG_ZF),                                   // NE/NZ
        0x6 => get_flag(cpu, FLAG_CF) || get_flag(cpu, FLAG_ZF),          // BE/NA
        0x7 => !get_flag(cpu, FLAG_CF) && !get_flag(cpu, FLAG_ZF),        // NBE/A
        0x8 => get_flag(cpu, FLAG_SF),                                    // S
        0x9 => !get_flag(cpu, FLAG_SF),                                   // NS
        0xA => get_flag(cpu, FLAG_PF),                                    // P/PE
        0xB => !get_flag(cpu, FLAG_PF),                                   // NP/PO
        0xC => get_flag(cpu, FLAG_SF) != get_flag(cpu, FLAG_OF),          // L/NGE
        0xD => get_flag(cpu, FLAG_SF) == get_flag(cpu, FLAG_OF),          // NL/GE
        0xE => get_flag(cpu, FLAG_ZF) || (get_flag(cpu, FLAG_SF) != get_flag(cpu, FLAG_OF)), // LE/NG
        0xF => !get_flag(cpu, FLAG_ZF) && (get_flag(cpu, FLAG_SF) == get_flag(cpu, FLAG_OF)), // NLE/G
        _ => unreachable!(),
    }
}
