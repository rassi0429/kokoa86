//! Flag computation tests

use kokoa86_cpu::flags::*;
use kokoa86_cpu::regs::CpuState;

#[test]
fn test_add_flags_zero() {
    let mut cpu = CpuState::default();
    update_flags_add(&mut cpu, 0, 0, 0, 8);
    assert!(get_flag(&cpu, FLAG_ZF));
    assert!(!get_flag(&cpu, FLAG_SF));
    assert!(!get_flag(&cpu, FLAG_CF));
    assert!(get_flag(&cpu, FLAG_PF)); // 0 has even parity
}

#[test]
fn test_add_flags_carry_8bit() {
    let mut cpu = CpuState::default();
    // 0xFF + 1 = 0x100 (carry out of 8 bits)
    update_flags_add(&mut cpu, 0xFF, 1, 0x100, 8);
    assert!(get_flag(&cpu, FLAG_CF));
    assert!(get_flag(&cpu, FLAG_ZF)); // low 8 bits = 0
}

#[test]
fn test_add_flags_overflow() {
    let mut cpu = CpuState::default();
    // 0x7F + 1 = 0x80 (positive + positive = negative => overflow)
    update_flags_add(&mut cpu, 0x7F, 1, 0x80, 8);
    assert!(get_flag(&cpu, FLAG_OF));
    assert!(get_flag(&cpu, FLAG_SF));
}

#[test]
fn test_sub_flags_zero() {
    let mut cpu = CpuState::default();
    update_flags_sub(&mut cpu, 5, 5, 0, 8);
    assert!(get_flag(&cpu, FLAG_ZF));
    assert!(!get_flag(&cpu, FLAG_CF));
}

#[test]
fn test_sub_flags_borrow() {
    let mut cpu = CpuState::default();
    // 0 - 1 = underflow (borrow)
    update_flags_sub(&mut cpu, 0, 1, 0xFFFFFFFFFFFFFFFF_u64.wrapping_add(1).wrapping_sub(1), 8);
    // Actually: 0 - 1 as u64 wrapping: let's just test via the function
    let mut cpu2 = CpuState::default();
    let result = (0u64).wrapping_sub(1);
    update_flags_sub(&mut cpu2, 0, 1, result, 8);
    assert!(get_flag(&cpu2, FLAG_CF));
    assert!(get_flag(&cpu2, FLAG_SF)); // 0xFF is negative in 8-bit
}

#[test]
fn test_logic_flags_clears_cf_of() {
    let mut cpu = CpuState::default();
    // Set CF and OF first
    set_flag(&mut cpu, FLAG_CF, true);
    set_flag(&mut cpu, FLAG_OF, true);
    // Logic op should clear them
    update_flags_logic(&mut cpu, 0xFF, 8);
    assert!(!get_flag(&cpu, FLAG_CF));
    assert!(!get_flag(&cpu, FLAG_OF));
    assert!(!get_flag(&cpu, FLAG_ZF));
    assert!(get_flag(&cpu, FLAG_SF));
}

#[test]
fn test_parity_flag() {
    let mut cpu = CpuState::default();
    // 0x01 has 1 bit set -> odd parity -> PF=0
    update_flags_logic(&mut cpu, 0x01, 8);
    assert!(!get_flag(&cpu, FLAG_PF));

    // 0x03 has 2 bits set -> even parity -> PF=1
    update_flags_logic(&mut cpu, 0x03, 8);
    assert!(get_flag(&cpu, FLAG_PF));
}

#[test]
fn test_condition_codes() {
    let mut cpu = CpuState::default();

    // ZF=1 => JE taken
    set_flag(&mut cpu, FLAG_ZF, true);
    assert!(check_condition(&cpu, 0x4)); // JE/JZ
    assert!(!check_condition(&cpu, 0x5)); // JNE/JNZ

    // CF=1 => JB taken
    set_flag(&mut cpu, FLAG_CF, true);
    assert!(check_condition(&cpu, 0x2)); // JB
    assert!(!check_condition(&cpu, 0x3)); // JNB/JAE

    // SF != OF => JL taken
    set_flag(&mut cpu, FLAG_SF, true);
    set_flag(&mut cpu, FLAG_OF, false);
    assert!(check_condition(&cpu, 0xC)); // JL
    assert!(!check_condition(&cpu, 0xD)); // JGE

    // SF == OF, ZF=0 => JG taken
    set_flag(&mut cpu, FLAG_SF, false);
    set_flag(&mut cpu, FLAG_OF, false);
    set_flag(&mut cpu, FLAG_ZF, false);
    assert!(check_condition(&cpu, 0xF)); // JG
}

#[test]
fn test_16bit_flags() {
    let mut cpu = CpuState::default();
    // 0x7FFF + 1 = 0x8000 (overflow in 16-bit)
    update_flags_add(&mut cpu, 0x7FFF, 1, 0x8000, 16);
    assert!(get_flag(&cpu, FLAG_OF));
    assert!(get_flag(&cpu, FLAG_SF));
    assert!(!get_flag(&cpu, FLAG_ZF));
    assert!(!get_flag(&cpu, FLAG_CF));
}
