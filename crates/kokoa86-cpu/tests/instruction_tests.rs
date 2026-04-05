//! Comprehensive CPU instruction unit tests
//!
//! Each test creates a minimal machine setup (CPU + memory), loads a short
//! instruction sequence, executes it, and verifies the resulting state.

use kokoa86_cpu::decode;
use kokoa86_cpu::execute::{self, ExecResult, IntHandler, PortIo};
use kokoa86_cpu::flags::*;
use kokoa86_cpu::regs::CpuState;
use kokoa86_mem::MemoryBus;

// Minimal stubs
struct NullPorts;
impl PortIo for NullPorts {
    fn port_in(&mut self, _port: u16, _size: u8) -> u32 { 0xFF }
    fn port_out(&mut self, _port: u16, _size: u8, _val: u32) {}
}

struct NullInt;
impl IntHandler for NullInt {
    fn handle_int(&mut self, _cpu: &mut CpuState, _mem: &mut MemoryBus, _vec: u8) -> bool { true }
}

fn run_code(code: &[u8]) -> (CpuState, MemoryBus) {
    let mut cpu = CpuState::default();
    let mut mem = MemoryBus::new(64 * 1024);
    cpu.eip = 0x7C00;
    cpu.esp = 0xFFFE;
    mem.load(0x7C00, code);

    let mut ports = NullPorts;
    let mut int_handler = NullInt;

    loop {
        let inst = decode::decode(&cpu, &mem);
        match execute::execute(&mut cpu, &mut mem, &mut ports, &mut int_handler, &inst) {
            ExecResult::Continue => {}
            ExecResult::Halt => break,
            ExecResult::UnknownOpcode(b) => panic!("Unknown opcode: 0x{:02X}", b),
        }
    }

    (cpu, mem)
}

// ============================================================
// MOV tests
// ============================================================

#[test]
fn test_mov_reg8_imm() {
    // MOV AL, 0x42; MOV CL, 0x13; HLT
    let (cpu, _) = run_code(&[0xB0, 0x42, 0xB1, 0x13, 0xF4]);
    assert_eq!(cpu.get_reg8(0), 0x42); // AL
    assert_eq!(cpu.get_reg8(1), 0x13); // CL
}

#[test]
fn test_mov_reg16_imm() {
    // MOV AX, 0x1234; MOV BX, 0x5678; HLT
    let (cpu, _) = run_code(&[0xB8, 0x34, 0x12, 0xBB, 0x78, 0x56, 0xF4]);
    assert_eq!(cpu.get_reg16(0), 0x1234);
    assert_eq!(cpu.get_reg16(3), 0x5678);
}

#[test]
fn test_mov_ah_imm() {
    // MOV AH, 0xAB; HLT
    let (cpu, _) = run_code(&[0xB4, 0xAB, 0xF4]);
    assert_eq!(cpu.get_reg8(4), 0xAB); // AH
    assert_eq!(cpu.get_reg8(0), 0x00); // AL should be zero
}

#[test]
fn test_mov_reg_to_reg_16() {
    // MOV AX, 0x1234; MOV BX, AX (89 C3); HLT
    let (cpu, _) = run_code(&[0xB8, 0x34, 0x12, 0x89, 0xC3, 0xF4]);
    assert_eq!(cpu.get_reg16(0), 0x1234);
    assert_eq!(cpu.get_reg16(3), 0x1234);
}

#[test]
fn test_mov_mem_to_al() {
    // Put 0x42 at DS:0x100, then MOV AL, [0x100]; HLT
    let mut cpu = CpuState::default();
    let mut mem = MemoryBus::new(64 * 1024);
    cpu.eip = 0x7C00;
    mem.load(0x100, &[0x42]);
    // A0 00 01 = MOV AL, [0x0100]
    // F4 = HLT
    mem.load(0x7C00, &[0xA0, 0x00, 0x01, 0xF4]);

    let mut ports = NullPorts;
    let mut ih = NullInt;
    loop {
        let inst = decode::decode(&cpu, &mem);
        match execute::execute(&mut cpu, &mut mem, &mut ports, &mut ih, &inst) {
            ExecResult::Continue => {}
            ExecResult::Halt => break,
            ExecResult::UnknownOpcode(b) => panic!("0x{:02X}", b),
        }
    }
    assert_eq!(cpu.get_reg8(0), 0x42);
}

// ============================================================
// ALU tests
// ============================================================

#[test]
fn test_add_reg16() {
    // MOV AX, 100; MOV BX, 200; ADD AX, BX; HLT
    let (cpu, _) = run_code(&[
        0xB8, 0x64, 0x00, // MOV AX, 100
        0xBB, 0xC8, 0x00, // MOV BX, 200
        0x01, 0xD8,       // ADD AX, BX
        0xF4,
    ]);
    assert_eq!(cpu.get_reg16(0), 300);
    assert!(!get_flag(&cpu, FLAG_ZF));
    assert!(!get_flag(&cpu, FLAG_CF));
}

#[test]
fn test_add_overflow_16() {
    // MOV AX, 0xFFFF; ADD AX, 1; HLT
    let (cpu, _) = run_code(&[
        0xB8, 0xFF, 0xFF, // MOV AX, 0xFFFF
        0x83, 0xC0, 0x01, // ADD AX, 1 (sign-extended imm8)
        0xF4,
    ]);
    assert_eq!(cpu.get_reg16(0), 0x0000);
    assert!(get_flag(&cpu, FLAG_ZF));
    assert!(get_flag(&cpu, FLAG_CF));
}

#[test]
fn test_sub_reg16() {
    // MOV AX, 300; SUB AX, 100; HLT
    let (cpu, _) = run_code(&[
        0xB8, 0x2C, 0x01, // MOV AX, 300
        0x83, 0xE8, 0x64, // SUB AX, 100 (sign-extended)
        0xF4,
    ]);
    assert_eq!(cpu.get_reg16(0), 200);
    assert!(!get_flag(&cpu, FLAG_ZF));
    assert!(!get_flag(&cpu, FLAG_CF));
}

#[test]
fn test_sub_borrow() {
    // MOV AX, 0; SUB AX, 1; HLT
    let (cpu, _) = run_code(&[
        0xB8, 0x00, 0x00, // MOV AX, 0
        0x83, 0xE8, 0x01, // SUB AX, 1
        0xF4,
    ]);
    assert_eq!(cpu.get_reg16(0), 0xFFFF);
    assert!(get_flag(&cpu, FLAG_CF)); // borrow
    assert!(get_flag(&cpu, FLAG_SF)); // negative
}

#[test]
fn test_and_reg8() {
    // MOV AL, 0xFF; AND AL, 0x0F; HLT
    let (cpu, _) = run_code(&[
        0xB0, 0xFF,       // MOV AL, 0xFF
        0x24, 0x0F,       // AND AL, 0x0F
        0xF4,
    ]);
    assert_eq!(cpu.get_reg8(0), 0x0F);
    assert!(!get_flag(&cpu, FLAG_CF));
    assert!(!get_flag(&cpu, FLAG_OF));
}

#[test]
fn test_or_reg8() {
    // MOV AL, 0xF0; OR AL, 0x0F; HLT
    let (cpu, _) = run_code(&[
        0xB0, 0xF0,
        0x0C, 0x0F,       // OR AL, 0x0F
        0xF4,
    ]);
    assert_eq!(cpu.get_reg8(0), 0xFF);
    assert!(get_flag(&cpu, FLAG_SF)); // bit 7 set
}

#[test]
fn test_xor_self_clears() {
    // MOV AX, 0x1234; XOR AX, AX; HLT
    let (cpu, _) = run_code(&[
        0xB8, 0x34, 0x12,
        0x31, 0xC0,       // XOR AX, AX
        0xF4,
    ]);
    assert_eq!(cpu.get_reg16(0), 0x0000);
    assert!(get_flag(&cpu, FLAG_ZF));
}

#[test]
fn test_cmp_equal() {
    // MOV AX, 42; CMP AX, 42; HLT
    let (cpu, _) = run_code(&[
        0xB8, 0x2A, 0x00,
        0x3D, 0x2A, 0x00, // CMP AX, 42
        0xF4,
    ]);
    assert!(get_flag(&cpu, FLAG_ZF));
    assert_eq!(cpu.get_reg16(0), 42); // CMP shouldn't modify AX
}

#[test]
fn test_cmp_less() {
    // MOV AX, 10; CMP AX, 20; HLT
    let (cpu, _) = run_code(&[
        0xB8, 0x0A, 0x00,
        0x3D, 0x14, 0x00,
        0xF4,
    ]);
    assert!(!get_flag(&cpu, FLAG_ZF));
    assert!(get_flag(&cpu, FLAG_CF)); // 10 < 20 => borrow
}

// ============================================================
// INC / DEC tests
// ============================================================

#[test]
fn test_inc_reg16() {
    // MOV AX, 0xFFFF; INC AX; HLT
    let (cpu, _) = run_code(&[
        0xB8, 0xFF, 0xFF,
        0x40,             // INC AX
        0xF4,
    ]);
    assert_eq!(cpu.get_reg16(0), 0x0000);
    assert!(get_flag(&cpu, FLAG_ZF));
    // INC does NOT affect CF
}

#[test]
fn test_dec_to_zero() {
    // MOV CX, 1; DEC CX; HLT
    let (cpu, _) = run_code(&[
        0xB9, 0x01, 0x00,
        0x49,             // DEC CX
        0xF4,
    ]);
    assert_eq!(cpu.get_reg16(1), 0);
    assert!(get_flag(&cpu, FLAG_ZF));
}

// ============================================================
// Stack tests
// ============================================================

#[test]
fn test_push_pop_multiple() {
    // PUSH 0x1111; PUSH 0x2222; POP AX; POP BX; HLT
    let (cpu, _) = run_code(&[
        0x68, 0x11, 0x11, // PUSH 0x1111
        0x68, 0x22, 0x22, // PUSH 0x2222
        0x58,             // POP AX
        0x5B,             // POP BX
        0xF4,
    ]);
    assert_eq!(cpu.get_reg16(0), 0x2222); // LIFO order
    assert_eq!(cpu.get_reg16(3), 0x1111);
}

#[test]
fn test_pushf_popf() {
    // STC; PUSHF; CLC; POPF; HLT
    // After: CF should be restored to 1
    let (cpu, _) = run_code(&[
        0xF9,             // STC
        0x9C,             // PUSHF
        0xF8,             // CLC
        0x9D,             // POPF
        0xF4,
    ]);
    assert!(get_flag(&cpu, FLAG_CF));
}

// ============================================================
// Control flow tests
// ============================================================

#[test]
fn test_jmp_short_forward() {
    // JMP +2; MOV AX, 0xDEAD; MOV BX, 0x42; HLT
    let (cpu, _) = run_code(&[
        0xEB, 0x03,       // JMP +3 (skip MOV AX)
        0xB8, 0xAD, 0xDE, // MOV AX, 0xDEAD (skipped)
        0xBB, 0x42, 0x00, // MOV BX, 0x42
        0xF4,
    ]);
    assert_eq!(cpu.get_reg16(0), 0); // AX untouched
    assert_eq!(cpu.get_reg16(3), 0x42);
}

#[test]
fn test_loop_countdown() {
    // MOV CX, 5; MOV AX, 0; .loop: INC AX; LOOP .loop; HLT
    let (cpu, _) = run_code(&[
        0xB9, 0x05, 0x00, // MOV CX, 5
        0xB8, 0x00, 0x00, // MOV AX, 0
        // .loop (offset 6):
        0x40,             // INC AX
        0xE2, 0xFD,       // LOOP -3 (back to INC)
        0xF4,
    ]);
    assert_eq!(cpu.get_reg16(0), 5);
    assert_eq!(cpu.get_reg16(1), 0); // CX should be 0
}

#[test]
fn test_call_ret_nested() {
    // CALL sub1; HLT
    // sub1: MOV AX, 1; CALL sub2; RET
    // sub2: MOV BX, 2; RET
    let (cpu, _) = run_code(&[
        0xE8, 0x01, 0x00, // CALL sub1 (offset +1 from next IP -> 0x7C04)
        0xF4,             // HLT
        // sub1 at 0x7C04:
        0xB8, 0x01, 0x00, // MOV AX, 1
        0xE8, 0x01, 0x00, // CALL sub2 (offset +1 from next IP -> 0x7C0C)
        0xC3,             // RET
        // sub2 at 0x7C0C:
        0xBB, 0x02, 0x00, // MOV BX, 2
        0xC3,             // RET
    ]);
    assert_eq!(cpu.get_reg16(0), 1);
    assert_eq!(cpu.get_reg16(3), 2);
}

// ============================================================
// Conditional jumps (Jcc)
// ============================================================

#[test]
fn test_jz_taken() {
    // XOR AX, AX; JZ +2; MOV BX, 0xFF; MOV CX, 0x42; HLT
    let (cpu, _) = run_code(&[
        0x31, 0xC0,       // XOR AX, AX (sets ZF)
        0x74, 0x03,       // JZ +3
        0xBB, 0xFF, 0x00, // MOV BX, 0xFF (skipped)
        0xB9, 0x42, 0x00,
        0xF4,
    ]);
    assert_eq!(cpu.get_reg16(3), 0); // skipped
    assert_eq!(cpu.get_reg16(1), 0x42);
}

#[test]
fn test_jnz_not_taken() {
    // XOR AX, AX; JNZ +2; MOV BX, 0x42; HLT
    let (cpu, _) = run_code(&[
        0x31, 0xC0,       // XOR AX, AX (ZF=1)
        0x75, 0x03,       // JNZ +3 (not taken)
        0xBB, 0x42, 0x00, // MOV BX, 0x42 (executed)
        0xF4,
    ]);
    assert_eq!(cpu.get_reg16(3), 0x42);
}

#[test]
fn test_jb_unsigned_less() {
    // MOV AX, 5; CMP AX, 10; JB +3; MOV BX, 0; MOV CX, 1; HLT
    let (cpu, _) = run_code(&[
        0xB8, 0x05, 0x00,
        0x3D, 0x0A, 0x00, // CMP AX, 10
        0x72, 0x03,       // JB +3 (taken: 5 < 10)
        0xBB, 0x00, 0x00,
        0xB9, 0x01, 0x00,
        0xF4,
    ]);
    assert_eq!(cpu.get_reg16(3), 0); // skipped
    assert_eq!(cpu.get_reg16(1), 1);
}

// ============================================================
// String operations
// ============================================================

#[test]
fn test_stosb() {
    // MOV AL, 0x41; MOV DI, 0x200; STOSB; STOSB; STOSB; HLT
    let (cpu, mem) = run_code(&[
        0xB0, 0x41,       // MOV AL, 'A'
        0xBF, 0x00, 0x02, // MOV DI, 0x200
        0xAA,             // STOSB
        0xAA,             // STOSB
        0xAA,             // STOSB
        0xF4,
    ]);
    use kokoa86_mem::MemoryAccess;
    assert_eq!(mem.read_u8(0x200), 0x41);
    assert_eq!(mem.read_u8(0x201), 0x41);
    assert_eq!(mem.read_u8(0x202), 0x41);
    assert_eq!(cpu.get_reg16(7), 0x203); // DI advanced by 3
}

#[test]
fn test_rep_stosb() {
    // MOV CX, 4; MOV AL, 0x55; MOV DI, 0x300; REP STOSB; HLT
    let (cpu, mem) = run_code(&[
        0xB9, 0x04, 0x00, // MOV CX, 4
        0xB0, 0x55,       // MOV AL, 0x55
        0xBF, 0x00, 0x03, // MOV DI, 0x300
        0xF3, 0xAA,       // REP STOSB
        0xF4,
    ]);
    use kokoa86_mem::MemoryAccess;
    for i in 0..4 {
        assert_eq!(mem.read_u8(0x300 + i), 0x55);
    }
    assert_eq!(cpu.get_reg16(1), 0); // CX = 0
}

#[test]
fn test_movsb() {
    use kokoa86_mem::MemoryAccess;
    // Setup: put "ABC" at 0x100
    let mut cpu = CpuState::default();
    let mut mem = MemoryBus::new(64 * 1024);
    cpu.eip = 0x7C00;
    cpu.esp = 0xFFFE;
    mem.load(0x100, b"ABC");

    // MOV SI, 0x100; MOV DI, 0x200; MOV CX, 3; REP MOVSB; HLT
    mem.load(0x7C00, &[
        0xBE, 0x00, 0x01, // MOV SI, 0x100
        0xBF, 0x00, 0x02, // MOV DI, 0x200
        0xB9, 0x03, 0x00, // MOV CX, 3
        0xF3, 0xA4,       // REP MOVSB
        0xF4,
    ]);

    let mut ports = NullPorts;
    let mut ih = NullInt;
    loop {
        let inst = decode::decode(&cpu, &mem);
        match execute::execute(&mut cpu, &mut mem, &mut ports, &mut ih, &inst) {
            ExecResult::Continue => {}
            ExecResult::Halt => break,
            _ => panic!(),
        }
    }

    assert_eq!(mem.read_u8(0x200), b'A');
    assert_eq!(mem.read_u8(0x201), b'B');
    assert_eq!(mem.read_u8(0x202), b'C');
}

// ============================================================
// TEST instruction
// ============================================================

#[test]
fn test_test_al_imm8() {
    // MOV AL, 0x0F; TEST AL, 0x10; HLT (no bits in common -> ZF=1)
    let (cpu, _) = run_code(&[
        0xB0, 0x0F,
        0xA8, 0x10,       // TEST AL, 0x10
        0xF4,
    ]);
    assert!(get_flag(&cpu, FLAG_ZF));
}

#[test]
fn test_test_al_match() {
    // MOV AL, 0xFF; TEST AL, 0x80; HLT (bit 7 set -> ZF=0, SF=1)
    let (cpu, _) = run_code(&[
        0xB0, 0xFF,
        0xA8, 0x80,
        0xF4,
    ]);
    assert!(!get_flag(&cpu, FLAG_ZF));
    assert!(get_flag(&cpu, FLAG_SF));
}

// ============================================================
// CBW / CWD
// ============================================================

#[test]
fn test_cbw_positive() {
    // MOV AL, 0x42; CBW; HLT
    let (cpu, _) = run_code(&[0xB0, 0x42, 0x98, 0xF4]);
    assert_eq!(cpu.get_reg16(0), 0x0042);
}

#[test]
fn test_cbw_negative() {
    // MOV AL, 0x80; CBW; HLT (sign-extend 0x80 -> 0xFF80)
    let (cpu, _) = run_code(&[0xB0, 0x80, 0x98, 0xF4]);
    assert_eq!(cpu.get_reg16(0), 0xFF80);
}

#[test]
fn test_cwd_positive() {
    // MOV AX, 0x1234; CWD; HLT
    let (cpu, _) = run_code(&[0xB8, 0x34, 0x12, 0x99, 0xF4]);
    assert_eq!(cpu.get_reg16(2), 0x0000); // DX
}

#[test]
fn test_cwd_negative() {
    // MOV AX, 0x8000; CWD; HLT
    let (cpu, _) = run_code(&[0xB8, 0x00, 0x80, 0x99, 0xF4]);
    assert_eq!(cpu.get_reg16(2), 0xFFFF); // DX
}

// ============================================================
// Misc
// ============================================================

#[test]
fn test_xchg_ax_bx() {
    // MOV AX, 0x1111; MOV BX, 0x2222; XCHG AX, BX; HLT
    let (cpu, _) = run_code(&[
        0xB8, 0x11, 0x11,
        0xBB, 0x22, 0x22,
        0x93,             // XCHG AX, BX
        0xF4,
    ]);
    assert_eq!(cpu.get_reg16(0), 0x2222);
    assert_eq!(cpu.get_reg16(3), 0x1111);
}

#[test]
fn test_flag_manipulation() {
    // STC; CMC; HLT (CF: 0 -> 1 -> 0)
    let (cpu, _) = run_code(&[
        0xF9,             // STC (CF=1)
        0xF5,             // CMC (CF=0)
        0xF4,
    ]);
    assert!(!get_flag(&cpu, FLAG_CF));
}

#[test]
fn test_cli_sti() {
    // STI; CLI; HLT
    let (cpu, _) = run_code(&[0xFB, 0xFA, 0xF4]);
    assert!(!get_flag(&cpu, FLAG_IF));
}

#[test]
fn test_cld_std() {
    // STD; CLD; HLT
    let (cpu, _) = run_code(&[0xFD, 0xFC, 0xF4]);
    assert!(!get_flag(&cpu, FLAG_DF));
}

#[test]
fn test_nop() {
    // NOP; NOP; MOV AX, 1; HLT
    let (cpu, _) = run_code(&[0x90, 0x90, 0xB8, 0x01, 0x00, 0xF4]);
    assert_eq!(cpu.get_reg16(0), 1);
}

// ============================================================
// LEA
// ============================================================

#[test]
fn test_lea_bx_si() {
    // MOV BX, 0x100; MOV SI, 0x50; LEA AX, [BX+SI]; HLT
    // LEA r16, [BX+SI]: modrm = 00 000 000 = 0x00 (mod=00, reg=AX, r/m=000 = [BX+SI])
    let (cpu, _) = run_code(&[
        0xBB, 0x00, 0x01, // MOV BX, 0x100
        0xBE, 0x50, 0x00, // MOV SI, 0x50
        0x8D, 0x00,       // LEA AX, [BX+SI]
        0xF4,
    ]);
    assert_eq!(cpu.get_reg16(0), 0x150);
}
