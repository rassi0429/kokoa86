use kokoa86_mem::{MemoryAccess, MemoryBus};
use crate::modrm::{ModrmOperand, decode_modrm_seg};
use crate::regs::{AddrSize, CpuState, OperandSize};

// Re-export for tests
pub use crate::modrm;

/// Decoded instruction
#[derive(Debug, Clone)]
pub struct Instruction {
    pub op: Opcode,
    pub len: u8,
    pub operand_size: OperandSize,
    pub addr_size: AddrSize,
    pub segment_override: Option<u8>,
}

#[derive(Debug, Clone)]
pub enum Opcode {
    // Data movement — "v" = operand-size dependent (16 or 32)
    MovReg8Imm(u8, u8),         // 0xB0-0xB7: MOV r8, imm8
    MovRegvImm(u8, u32),        // 0xB8-0xBF: MOV r16/32, imm16/32
    MovModrm8(u8, ModrmOperand, u8),  // 0x88/0x8A: r8 <-> r/m8 (direction via original opcode)
    MovModrmv(u8, ModrmOperand, u8),  // 0x89/0x8B: rv <-> r/mv
    MovRmImm8(ModrmOperand, u8, u8),  // 0xC6: MOV r/m8, imm8
    MovRmImmv(ModrmOperand, u8, u32), // 0xC7: MOV r/m16/32, imm16/32
    MovAlMem(u32),              // 0xA0: MOV AL, [addr]
    MovAxMem(u32),              // 0xA1: MOV AX/EAX, [addr]
    MovMemAl(u32),              // 0xA2
    MovMemAx(u32),              // 0xA3
    MovSregRm(u8, ModrmOperand, u8),  // 0x8E
    MovRmSreg(u8, ModrmOperand, u8),  // 0x8C

    // ALU: op, reg, rm, modrm_bytes
    AluRmReg8(AluOp, u8, ModrmOperand, u8),
    AluRmRegv(AluOp, u8, ModrmOperand, u8),
    AluRegRm8(AluOp, u8, ModrmOperand, u8),
    AluRegRmv(AluOp, u8, ModrmOperand, u8),
    AluAlImm8(AluOp, u8),
    AluAxImmv(AluOp, u32),
    AluRmImm8(AluOp, ModrmOperand, u8, u8),      // 0x80: r/m8, imm8
    AluRmImmv(AluOp, ModrmOperand, u8, u32),      // 0x81: r/mv, immv
    AluRmImmvs8(AluOp, ModrmOperand, u8, u32),    // 0x83: r/mv, sign-ext imm8

    // TEST
    TestRmReg8(u8, ModrmOperand, u8),
    TestRmRegv(u8, ModrmOperand, u8),
    TestAlImm8(u8),
    TestAxImmv(u32),

    // INC/DEC register
    IncRegv(u8),    // 0x40-0x47
    DecRegv(u8),    // 0x48-0x4F

    // Stack
    PushRegv(u8),   // 0x50-0x57
    PopRegv(u8),    // 0x58-0x5F
    PushImmv(u32),  // 0x68
    PushImm8(u8),   // 0x6A
    Pushf,          // 0x9C
    Popf,           // 0x9D

    // Control flow
    JmpShort(i8),       // 0xEB
    JmpNearRel(i32),    // 0xE9 (16 or 32 bit displacement)
    JmpFar(u16, u32),   // 0xEA: seg:offset
    Jcc(u8, i8),        // 0x70-0x7F: short
    JccNear(u8, i32),   // 0x0F 80-8F: near
    CallNearRel(i32),   // 0xE8
    CallFar(u16, u32),  // 0x9A
    Ret,                // 0xC3
    RetImm16(u16),      // 0xC2
    Retf,               // 0xCB
    RetfImm16(u16),     // 0xCA

    // Interrupts
    Int(u8),
    Iret,

    // I/O
    InAlImm8(u8),
    InAxImm8(u8),
    OutImm8Al(u8),
    OutImm8Ax(u8),
    InAlDx,
    InAxDx,
    OutDxAl,
    OutDxAx,

    // LEA
    Leav(u8, ModrmOperand, u8),

    // XCHG
    XchgAxReg(u8),           // 0x91-0x97
    XchgRmReg8(u8, ModrmOperand, u8),  // 0x86
    XchgRmRegv(u8, ModrmOperand, u8),  // 0x87

    // String operations
    Movsb, Movsv,
    Stosb, Stosv,
    Lodsb, Lodsv,
    Cmpsb, Cmpsv,
    Scasb, Scasv,

    // REP/REPNE
    Rep(Box<Opcode>),
    Repne(Box<Opcode>),

    // LOOP
    Loop(i8),
    Loope(i8),
    Loopne(i8),

    // CBW/CWD / CWDE/CDQ
    Cbw,   // 0x98: CBW (16-bit) or CWDE (32-bit)
    Cwd,   // 0x99: CWD (16-bit) or CDQ (32-bit)

    // Shift/Rotate: op, rm, modrm_bytes, count
    ShiftRm8(ShiftOp, ModrmOperand, u8, ShiftCount),
    ShiftRmv(ShiftOp, ModrmOperand, u8, ShiftCount),

    // Group F6/F7: sub-op, rm, modrm_bytes
    // F6: TEST/NOT/NEG/MUL/IMUL/DIV/IDIV r/m8
    GroupF6(u8, ModrmOperand, u8, Option<u8>),  // optional imm for TEST
    // F7: same for r/mv
    GroupF7(u8, ModrmOperand, u8, Option<u32>),

    // Group FF
    GroupFF(u8, ModrmOperand, u8),

    // Group FE: INC/DEC r/m8
    GroupFE(u8, ModrmOperand, u8),

    // 0x0F two-byte opcodes
    // LGDT/LIDT/SGDT/SIDT
    Group0F01(u8, ModrmOperand, u8),
    // MOV r32, CRn / MOV CRn, r32
    MovFromCr(u8, u8),  // cr, reg
    MovToCr(u8, u8),
    // MOVZX/MOVSX
    MovzxByte(u8, ModrmOperand, u8),  // 0F B6: r16/32 <- r/m8
    MovzxWord(u8, ModrmOperand, u8),  // 0F B7: r32 <- r/m16
    MovsxByte(u8, ModrmOperand, u8),  // 0F BE
    MovsxWord(u8, ModrmOperand, u8),  // 0F BF
    // SETcc
    Setcc(u8, ModrmOperand, u8),
    // IMUL r, r/m (two-operand)
    ImulRegRmv(u8, ModrmOperand, u8),  // 0F AF
    // IMUL r, r/m, imm (three-operand)
    ImulRegRmvImm8(u8, ModrmOperand, u8, i8),   // 0x6B
    ImulRegRmvImmv(u8, ModrmOperand, u8, u32),   // 0x69

    // SAHF/LAHF
    Sahf,  // 0x9E
    Lahf,  // 0x9F

    // Misc
    Nop,
    Hlt,
    Cli, Sti, Cld, Std, Cmc, Clc, Stc,

    // LEAVE
    Leave,  // 0xC9

    // ENTER
    Enter(u16, u8), // 0xC8: size, nesting

    // CPUID / cache / TSC / MSR
    Cpuid,           // 0x0F A2
    Wbinvd,          // 0x0F 09
    Invd,            // 0x0F 08
    Rdtsc,           // 0x0F 31
    Rdmsr,           // 0x0F 32
    Wrmsr,           // 0x0F 30

    // Port string I/O
    Insb,            // 0x6C
    Insv,            // 0x6D
    Outsb,           // 0x6E
    Outsv,           // 0x6F

    // Bit scan
    Bsf(u8, ModrmOperand, u8),    // 0x0F BC
    Bsr(u8, ModrmOperand, u8),    // 0x0F BD

    // Bit test
    BtRmReg(u8, ModrmOperand, u8),   // 0x0F A3
    BtsRmReg(u8, ModrmOperand, u8),  // 0x0F AB
    BtrRmReg(u8, ModrmOperand, u8),  // 0x0F B3
    BtcRmReg(u8, ModrmOperand, u8),  // 0x0F BB
    BtGroup(u8, ModrmOperand, u8, u8), // 0x0F BA /4-7 with imm8

    // BSWAP
    Bswap(u8),       // 0x0F C8+r

    // XADD
    XaddRm8(u8, ModrmOperand, u8),   // 0x0F C0
    XaddRmv(u8, ModrmOperand, u8),   // 0x0F C1

    // CMPXCHG
    CmpxchgRm8(u8, ModrmOperand, u8),  // 0x0F B0
    CmpxchgRmv(u8, ModrmOperand, u8),  // 0x0F B1

    // BCD/ASCII adjust
    Aad(u8),    // 0xD5 imm8
    Aam(u8),    // 0xD4 imm8
    Daa,        // 0x27
    Das,        // 0x2F
    Aaa,        // 0x37
    Aas,        // 0x3F

    // XLAT
    Xlat,       // 0xD7

    // PUSHA/POPA
    Pusha,      // 0x60
    Popa,       // 0x61

    // BOUND (decode + skip)
    Bound(u8, ModrmOperand, u8),  // 0x62
    // ARPL
    Arpl(u8, ModrmOperand, u8),   // 0x63

    // LES/LDS
    Les(u8, ModrmOperand, u8),    // 0xC4
    Lds(u8, ModrmOperand, u8),    // 0xC5

    // INT 3 / INTO
    Int3,       // 0xCC
    Into,       // 0xCE

    // 0x0F extended
    Lar(u8, ModrmOperand, u8),    // 0x0F 02
    Lsl(u8, ModrmOperand, u8),    // 0x0F 03
    Clts,                          // 0x0F 06

    Unknown(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AluOp {
    Add = 0, Or = 1, Adc = 2, Sbb = 3,
    And = 4, Sub = 5, Xor = 6, Cmp = 7,
}

impl AluOp {
    pub fn from_reg(reg: u8) -> Self {
        match reg {
            0 => AluOp::Add, 1 => AluOp::Or, 2 => AluOp::Adc, 3 => AluOp::Sbb,
            4 => AluOp::And, 5 => AluOp::Sub, 6 => AluOp::Xor, 7 => AluOp::Cmp,
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftOp {
    Rol = 0, Ror = 1, Rcl = 2, Rcr = 3,
    Shl = 4, Shr = 5, Sal = 6, Sar = 7,
}

impl ShiftOp {
    pub fn from_reg(reg: u8) -> Self {
        match reg {
            0 => ShiftOp::Rol, 1 => ShiftOp::Ror, 2 => ShiftOp::Rcl, 3 => ShiftOp::Rcr,
            4 => ShiftOp::Shl, 5 => ShiftOp::Shr, 6 => ShiftOp::Sal, 7 => ShiftOp::Sar,
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ShiftCount {
    One,
    CL,
    Imm(u8),
}

// ============================================================
// Decoder
// ============================================================

/// Decode at arbitrary linear address (for disassembly)
pub fn decode_at_addr(cpu: &CpuState, mem: &MemoryBus, addr: u32) -> Instruction {
    decode_impl(cpu, mem, addr)
}

/// Decode next instruction at CS:IP
pub fn decode(cpu: &CpuState, mem: &MemoryBus) -> Instruction {
    decode_impl(cpu, mem, cpu.cs_ip())
}

fn decode_impl(cpu: &CpuState, mem: &MemoryBus, base: u32) -> Instruction {
    let mut pos: u32 = 0;

    let fetch8 = |off: u32| -> u8 { mem.read_u8(base.wrapping_add(off)) };

    // Default sizes from CS descriptor
    let default_opsize = if cpu.cs_cache.big { OperandSize::Dword32 } else { OperandSize::Word16 };
    let default_addrsize = if cpu.cs_cache.big { AddrSize::Addr32 } else { AddrSize::Addr16 };
    let mut operand_size = default_opsize;
    let mut addr_size = default_addrsize;
    let mut seg_override: Option<u8> = None;

    // Parse prefix bytes
    loop {
        let b = fetch8(pos);
        match b {
            0x66 => {
                operand_size = match operand_size {
                    OperandSize::Word16 => OperandSize::Dword32,
                    OperandSize::Dword32 => OperandSize::Word16,
                };
                pos += 1;
            }
            0x67 => {
                addr_size = match addr_size {
                    AddrSize::Addr16 => AddrSize::Addr32,
                    AddrSize::Addr32 => AddrSize::Addr16,
                };
                pos += 1;
            }
            0x26 => { seg_override = Some(0); pos += 1; } // ES
            0x2E => { seg_override = Some(1); pos += 1; } // CS
            0x36 => { seg_override = Some(2); pos += 1; } // SS
            0x3E => { seg_override = Some(3); pos += 1; } // DS
            0x64 => { seg_override = Some(4); pos += 1; } // FS
            0x65 => { seg_override = Some(5); pos += 1; } // GS
            0xF0 => { pos += 1; } // LOCK prefix (ignore)
            0xF3 | 0xF2 => {
                // REP/REPNE — consume prefix, decode inner instruction
                pos += 1;
                let op = decode_opcode(cpu, mem, base, &mut pos, operand_size, addr_size, seg_override);
                let wrapped = if b == 0xF3 {
                    Opcode::Rep(Box::new(op))
                } else {
                    Opcode::Repne(Box::new(op))
                };
                return Instruction {
                    op: wrapped,
                    len: pos as u8,
                    operand_size,
                    addr_size,
                    segment_override: seg_override,
                };
            }
            _ => break,
        }
    }

    let op = decode_opcode(cpu, mem, base, &mut pos, operand_size, addr_size, seg_override);
    Instruction {
        op,
        len: pos as u8,
        operand_size,
        addr_size,
        segment_override: seg_override,
    }
}

fn decode_opcode(
    cpu: &CpuState,
    mem: &MemoryBus,
    base: u32,
    pos: &mut u32,
    operand_size: OperandSize,
    _addr_size: AddrSize,
    _seg_override: Option<u8>,
) -> Opcode {
    let fetch8 = |off: u32| -> u8 { mem.read_u8(base.wrapping_add(off)) };
    let fetch16 = |off: u32| -> u16 { mem.read_u16(base.wrapping_add(off)) };
    let fetch32 = |off: u32| -> u32 { mem.read_u32(base.wrapping_add(off)) };

    let is32 = operand_size == OperandSize::Dword32;

    // Helper to fetch immediate based on operand size
    let fetch_immv = |p: &mut u32| -> u32 {
        if is32 {
            let v = fetch32(*p);
            *p += 4;
            v
        } else {
            let v = fetch16(*p) as u32;
            *p += 2;
            v
        }
    };

    // Helper to sign-extend imm8 to operand size
    let sign_ext_imm8 = |v: u8| -> u32 {
        if is32 {
            v as i8 as i32 as u32
        } else {
            (v as i8 as i16 as u16) as u32
        }
    };

    let modrm = |p: &mut u32| -> (u8, ModrmOperand, u8) {
        decode_modrm_seg(cpu, mem, base + *p, _addr_size, _seg_override)
    };

    let opcode_byte = fetch8(*pos);
    *pos += 1;

    match opcode_byte {
        // ALU r/m8, r8
        0x00 | 0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 => {
            let alu = AluOp::from_reg((opcode_byte >> 3) & 7);
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::AluRmReg8(alu, reg, rm, bytes)
        }
        // ALU r/mv, rv
        0x01 | 0x09 | 0x11 | 0x19 | 0x21 | 0x29 | 0x31 | 0x39 => {
            let alu = AluOp::from_reg((opcode_byte >> 3) & 7);
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::AluRmRegv(alu, reg, rm, bytes)
        }
        // ALU r8, r/m8
        0x02 | 0x0A | 0x12 | 0x1A | 0x22 | 0x2A | 0x32 | 0x3A => {
            let alu = AluOp::from_reg((opcode_byte >> 3) & 7);
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::AluRegRm8(alu, reg, rm, bytes)
        }
        // ALU rv, r/mv
        0x03 | 0x0B | 0x13 | 0x1B | 0x23 | 0x2B | 0x33 | 0x3B => {
            let alu = AluOp::from_reg((opcode_byte >> 3) & 7);
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::AluRegRmv(alu, reg, rm, bytes)
        }
        // ALU AL, imm8
        0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C => {
            let alu = AluOp::from_reg((opcode_byte >> 3) & 7);
            let imm = fetch8(*pos); *pos += 1;
            Opcode::AluAlImm8(alu, imm)
        }
        // ALU AX/EAX, immv
        0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D => {
            let alu = AluOp::from_reg((opcode_byte >> 3) & 7);
            let imm = fetch_immv(pos);
            Opcode::AluAxImmv(alu, imm)
        }

        // DAA
        0x27 => Opcode::Daa,
        // DAS
        0x2F => Opcode::Das,
        // AAA
        0x37 => Opcode::Aaa,
        // AAS
        0x3F => Opcode::Aas,

        // Segment override prefixes (shouldn't reach here after prefix loop, but handle)
        0x06 => { // PUSH ES
            Opcode::PushRegv(0) // handled specially in execute
        }
        0x07 => Opcode::PopRegv(0), // POP ES
        0x0E => Opcode::PushRegv(1), // PUSH CS
        0x16 => Opcode::PushRegv(2), // PUSH SS
        0x17 => Opcode::PopRegv(2), // POP SS
        0x1E => Opcode::PushRegv(3), // PUSH DS
        0x1F => Opcode::PopRegv(3), // POP DS

        // Two-byte opcodes (0x0F)
        0x0F => decode_0f(cpu, mem, base, pos, operand_size, _addr_size, _seg_override),

        // INC rv
        0x40..=0x47 => Opcode::IncRegv(opcode_byte - 0x40),
        // DEC rv
        0x48..=0x4F => Opcode::DecRegv(opcode_byte - 0x48),

        // PUSH rv
        0x50..=0x57 => Opcode::PushRegv(opcode_byte - 0x50),
        // POP rv
        0x58..=0x5F => Opcode::PopRegv(opcode_byte - 0x58),

        // PUSHA/PUSHAD
        0x60 => Opcode::Pusha,
        // POPA/POPAD
        0x61 => Opcode::Popa,
        // BOUND
        0x62 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::Bound(reg, rm, bytes)
        }
        // ARPL
        0x63 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::Arpl(reg, rm, bytes)
        }

        // PUSH immv
        0x68 => {
            let imm = fetch_immv(pos);
            Opcode::PushImmv(imm)
        }
        // IMUL r, r/m, immv
        0x69 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            let imm = fetch_immv(pos);
            Opcode::ImulRegRmvImmv(reg, rm, bytes, imm)
        }
        // PUSH imm8
        0x6A => {
            let imm = fetch8(*pos); *pos += 1;
            Opcode::PushImm8(imm)
        }
        // IMUL r, r/m, imm8
        0x6B => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            let imm = fetch8(*pos) as i8; *pos += 1;
            Opcode::ImulRegRmvImm8(reg, rm, bytes, imm)
        }

        // INS/OUTS port string I/O
        0x6C => Opcode::Insb,
        0x6D => Opcode::Insv,
        0x6E => Opcode::Outsb,
        0x6F => Opcode::Outsv,

        // Jcc short
        0x70..=0x7F => {
            let cc = opcode_byte - 0x70;
            let disp = fetch8(*pos) as i8; *pos += 1;
            Opcode::Jcc(cc, disp)
        }

        // Group 0x80: ALU r/m8, imm8
        0x80 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            let imm = fetch8(*pos); *pos += 1;
            Opcode::AluRmImm8(AluOp::from_reg(reg), rm, bytes, imm)
        }
        // Group 0x81: ALU r/mv, immv
        0x81 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            let imm = fetch_immv(pos);
            Opcode::AluRmImmv(AluOp::from_reg(reg), rm, bytes, imm)
        }
        // Group 0x83: ALU r/mv, sign-extended imm8
        0x83 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            let imm = sign_ext_imm8(fetch8(*pos)); *pos += 1;
            Opcode::AluRmImmvs8(AluOp::from_reg(reg), rm, bytes, imm)
        }

        // TEST r/m8, r8
        0x84 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::TestRmReg8(reg, rm, bytes)
        }
        // TEST r/mv, rv
        0x85 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::TestRmRegv(reg, rm, bytes)
        }
        // XCHG r/m8, r8
        0x86 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::XchgRmReg8(reg, rm, bytes)
        }
        // XCHG r/mv, rv
        0x87 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::XchgRmRegv(reg, rm, bytes)
        }

        // MOV r/m8, r8
        0x88 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::MovModrm8(reg, rm, bytes)
        }
        // MOV r/mv, rv
        0x89 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::MovModrmv(reg, rm, bytes)
        }
        // MOV r8, r/m8
        0x8A => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::MovModrm8(reg, rm, bytes)
        }
        // MOV rv, r/mv
        0x8B => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::MovModrmv(reg, rm, bytes)
        }
        // MOV r/m16, Sreg
        0x8C => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::MovRmSreg(reg, rm, bytes)
        }
        // LEA
        0x8D => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::Leav(reg, rm, bytes)
        }
        // MOV Sreg, r/m16
        0x8E => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::MovSregRm(reg, rm, bytes)
        }

        // NOP / XCHG AX, rv
        0x90 => Opcode::Nop,
        0x91..=0x97 => Opcode::XchgAxReg(opcode_byte - 0x90),

        // CBW/CWDE
        0x98 => Opcode::Cbw,
        // CWD/CDQ
        0x99 => Opcode::Cwd,
        // CALL FAR
        0x9A => {
            let offset = fetch_immv(pos);
            let seg = fetch16(*pos); *pos += 2;
            Opcode::CallFar(seg, offset)
        }

        // PUSHF
        0x9C => Opcode::Pushf,
        // POPF
        0x9D => Opcode::Popf,
        // SAHF
        0x9E => Opcode::Sahf,
        // LAHF
        0x9F => Opcode::Lahf,

        // MOV AL/AX, [moffs] — moffs size is determined by ADDRESS size, not operand size
        0xA0 | 0xA1 | 0xA2 | 0xA3 => {
            let addr_32 = _addr_size == AddrSize::Addr32;
            let a = if addr_32 {
                let v = fetch32(*pos); *pos += 4; v
            } else {
                let v = fetch16(*pos) as u32; *pos += 2; v
            };
            match opcode_byte {
                0xA0 => Opcode::MovAlMem(a),
                0xA1 => Opcode::MovAxMem(a),
                0xA2 => Opcode::MovMemAl(a),
                0xA3 => Opcode::MovMemAx(a),
                _ => unreachable!(),
            }
        }

        // String ops
        0xA4 => Opcode::Movsb,
        0xA5 => Opcode::Movsv,
        0xA6 => Opcode::Cmpsb,
        0xA7 => Opcode::Cmpsv,
        0xA8 => { let imm = fetch8(*pos); *pos += 1; Opcode::TestAlImm8(imm) }
        0xA9 => { let imm = fetch_immv(pos); Opcode::TestAxImmv(imm) }
        0xAA => Opcode::Stosb,
        0xAB => Opcode::Stosv,
        0xAC => Opcode::Lodsb,
        0xAD => Opcode::Lodsv,
        0xAE => Opcode::Scasb,
        0xAF => Opcode::Scasv,

        // MOV r8, imm8
        0xB0..=0xB7 => {
            let reg = opcode_byte - 0xB0;
            let imm = fetch8(*pos); *pos += 1;
            Opcode::MovReg8Imm(reg, imm)
        }
        // MOV rv, immv
        0xB8..=0xBF => {
            let reg = opcode_byte - 0xB8;
            let imm = fetch_immv(pos);
            Opcode::MovRegvImm(reg, imm)
        }

        // Shift r/m8, imm8
        0xC0 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            let imm = fetch8(*pos); *pos += 1;
            Opcode::ShiftRm8(ShiftOp::from_reg(reg), rm, bytes, ShiftCount::Imm(imm))
        }
        // Shift r/mv, imm8
        0xC1 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            let imm = fetch8(*pos); *pos += 1;
            Opcode::ShiftRmv(ShiftOp::from_reg(reg), rm, bytes, ShiftCount::Imm(imm))
        }

        // RET imm16
        0xC2 => { let imm = fetch16(*pos); *pos += 2; Opcode::RetImm16(imm) }
        // RET
        0xC3 => Opcode::Ret,

        // LES
        0xC4 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::Les(reg, rm, bytes)
        }
        // LDS
        0xC5 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::Lds(reg, rm, bytes)
        }

        // MOV r/m8, imm8
        0xC6 => {
            let (_reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            let imm = fetch8(*pos); *pos += 1;
            Opcode::MovRmImm8(rm, bytes, imm)
        }
        // MOV r/mv, immv
        0xC7 => {
            let (_reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            let imm = fetch_immv(pos);
            Opcode::MovRmImmv(rm, bytes, imm)
        }

        // ENTER
        0xC8 => {
            let size = fetch16(*pos); *pos += 2;
            let nesting = fetch8(*pos); *pos += 1;
            Opcode::Enter(size, nesting)
        }
        // LEAVE
        0xC9 => Opcode::Leave,
        // RETF imm16
        0xCA => { let imm = fetch16(*pos); *pos += 2; Opcode::RetfImm16(imm) }
        // RETF
        0xCB => Opcode::Retf,

        // INT 3 (breakpoint)
        0xCC => Opcode::Int3,
        // INT imm8
        0xCD => { let v = fetch8(*pos); *pos += 1; Opcode::Int(v) }
        // INTO
        0xCE => Opcode::Into,
        // IRET
        0xCF => Opcode::Iret,

        // Shift r/m8, 1
        0xD0 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::ShiftRm8(ShiftOp::from_reg(reg), rm, bytes, ShiftCount::One)
        }
        // Shift r/mv, 1
        0xD1 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::ShiftRmv(ShiftOp::from_reg(reg), rm, bytes, ShiftCount::One)
        }
        // Shift r/m8, CL
        0xD2 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::ShiftRm8(ShiftOp::from_reg(reg), rm, bytes, ShiftCount::CL)
        }
        // Shift r/mv, CL
        0xD3 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::ShiftRmv(ShiftOp::from_reg(reg), rm, bytes, ShiftCount::CL)
        }

        // AAM imm8
        0xD4 => { let imm = fetch8(*pos); *pos += 1; Opcode::Aam(imm) }
        // AAD imm8
        0xD5 => { let imm = fetch8(*pos); *pos += 1; Opcode::Aad(imm) }

        // XLAT/XLATB
        0xD7 => Opcode::Xlat,

        // LOOPNE
        0xE0 => { let d = fetch8(*pos) as i8; *pos += 1; Opcode::Loopne(d) }
        // LOOPE
        0xE1 => { let d = fetch8(*pos) as i8; *pos += 1; Opcode::Loope(d) }
        // LOOP
        0xE2 => { let d = fetch8(*pos) as i8; *pos += 1; Opcode::Loop(d) }

        // IN AL, imm8
        0xE4 => { let p = fetch8(*pos); *pos += 1; Opcode::InAlImm8(p) }
        0xE5 => { let p = fetch8(*pos); *pos += 1; Opcode::InAxImm8(p) }
        0xE6 => { let p = fetch8(*pos); *pos += 1; Opcode::OutImm8Al(p) }
        0xE7 => { let p = fetch8(*pos); *pos += 1; Opcode::OutImm8Ax(p) }

        // CALL near
        0xE8 => {
            let disp = if is32 {
                let d = fetch32(*pos) as i32; *pos += 4; d
            } else {
                let d = fetch16(*pos) as i16 as i32; *pos += 2; d
            };
            Opcode::CallNearRel(disp)
        }
        // JMP near
        0xE9 => {
            let disp = if is32 {
                let d = fetch32(*pos) as i32; *pos += 4; d
            } else {
                let d = fetch16(*pos) as i16 as i32; *pos += 2; d
            };
            Opcode::JmpNearRel(disp)
        }
        // JMP far
        0xEA => {
            let offset = fetch_immv(pos);
            let seg = fetch16(*pos); *pos += 2;
            Opcode::JmpFar(seg, offset)
        }
        // JMP short
        0xEB => { let d = fetch8(*pos) as i8; *pos += 1; Opcode::JmpShort(d) }

        0xEC => Opcode::InAlDx,
        0xED => Opcode::InAxDx,
        0xEE => Opcode::OutDxAl,
        0xEF => Opcode::OutDxAx,

        // HLT
        0xF4 => Opcode::Hlt,
        // CMC
        0xF5 => Opcode::Cmc,

        // Group F6: byte operations
        0xF6 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            let test_imm = if reg == 0 || reg == 1 {
                let imm = fetch8(*pos); *pos += 1;
                Some(imm)
            } else { None };
            Opcode::GroupF6(reg, rm, bytes, test_imm)
        }
        // Group F7: word/dword operations
        0xF7 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            let test_imm = if reg == 0 || reg == 1 {
                let imm = fetch_immv(pos);
                Some(imm)
            } else { None };
            Opcode::GroupF7(reg, rm, bytes, test_imm)
        }

        0xF8 => Opcode::Clc,
        0xF9 => Opcode::Stc,
        0xFA => Opcode::Cli,
        0xFB => Opcode::Sti,
        0xFC => Opcode::Cld,
        0xFD => Opcode::Std,

        // Group FE: INC/DEC r/m8
        0xFE => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::GroupFE(reg, rm, bytes)
        }
        // Group FF
        0xFF => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::GroupFF(reg, rm, bytes)
        }

        _ => Opcode::Unknown(opcode_byte),
    }
}

/// Decode 0x0F two-byte opcodes
fn decode_0f(
    cpu: &CpuState,
    mem: &MemoryBus,
    base: u32,
    pos: &mut u32,
    operand_size: OperandSize,
    addr_size: AddrSize,
    seg_override: Option<u8>,
) -> Opcode {
    let fetch8 = |off: u32| -> u8 { mem.read_u8(base.wrapping_add(off)) };
    let fetch16 = |off: u32| -> u16 { mem.read_u16(base.wrapping_add(off)) };
    let fetch32 = |off: u32| -> u32 { mem.read_u32(base.wrapping_add(off)) };
    let is32 = operand_size == OperandSize::Dword32;

    let modrm = |p: &mut u32| -> (u8, ModrmOperand, u8) {
        decode_modrm_seg(cpu, mem, base + *p, addr_size, seg_override)
    };

    let second = fetch8(*pos);
    *pos += 1;

    match second {
        // Group 7: SGDT/SIDT/LGDT/LIDT/SMSW/LMSW/INVLPG
        0x01 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::Group0F01(reg, rm, bytes)
        }

        // LAR r, r/m
        0x02 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::Lar(reg, rm, bytes)
        }
        // LSL r, r/m
        0x03 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::Lsl(reg, rm, bytes)
        }

        // CLTS
        0x06 => Opcode::Clts,

        // MOV r32, CRn
        0x20 => {
            let modrm_byte = fetch8(*pos); *pos += 1;
            let cr = (modrm_byte >> 3) & 7;
            let reg = modrm_byte & 7;
            Opcode::MovFromCr(cr, reg)
        }
        // MOV CRn, r32
        0x22 => {
            let modrm_byte = fetch8(*pos); *pos += 1;
            let cr = (modrm_byte >> 3) & 7;
            let reg = modrm_byte & 7;
            Opcode::MovToCr(cr, reg)
        }

        // Jcc near (16-bit or 32-bit displacement)
        0x80..=0x8F => {
            let cc = second - 0x80;
            let disp = if is32 {
                let d = fetch32(*pos) as i32; *pos += 4; d
            } else {
                let d = fetch16(*pos) as i16 as i32; *pos += 2; d
            };
            Opcode::JccNear(cc, disp)
        }

        // SETcc r/m8
        0x90..=0x9F => {
            let cc = second - 0x90;
            let (_, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::Setcc(cc, rm, bytes)
        }

        // IMUL r, r/m
        0xAF => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::ImulRegRmv(reg, rm, bytes)
        }

        // INVD
        0x08 => Opcode::Invd,
        // WBINVD
        0x09 => Opcode::Wbinvd,

        // WRMSR
        0x30 => Opcode::Wrmsr,
        // RDTSC
        0x31 => Opcode::Rdtsc,
        // RDMSR
        0x32 => Opcode::Rdmsr,

        // CPUID
        0xA2 => Opcode::Cpuid,

        // BT r/m, reg
        0xA3 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::BtRmReg(reg, rm, bytes)
        }

        // BTS r/m, reg
        0xAB => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::BtsRmReg(reg, rm, bytes)
        }

        // CMPXCHG r/m8, r8
        0xB0 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::CmpxchgRm8(reg, rm, bytes)
        }
        // CMPXCHG r/mv, rv
        0xB1 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::CmpxchgRmv(reg, rm, bytes)
        }

        // BTR r/m, reg
        0xB3 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::BtrRmReg(reg, rm, bytes)
        }

        // MOVZX r, r/m8
        0xB6 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::MovzxByte(reg, rm, bytes)
        }
        // MOVZX r32, r/m16
        0xB7 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::MovzxWord(reg, rm, bytes)
        }
        // MOVSX r, r/m8
        0xBE => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::MovsxByte(reg, rm, bytes)
        }
        // MOVSX r32, r/m16
        0xBF => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::MovsxWord(reg, rm, bytes)
        }

        // BT group: 0F BA /4-7 with imm8
        0xBA => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            let imm = fetch8(*pos); *pos += 1;
            Opcode::BtGroup(reg, rm, bytes, imm)
        }

        // BTC r/m, reg
        0xBB => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::BtcRmReg(reg, rm, bytes)
        }

        // BSF
        0xBC => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::Bsf(reg, rm, bytes)
        }
        // BSR
        0xBD => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::Bsr(reg, rm, bytes)
        }

        // XADD r/m8, r8
        0xC0 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::XaddRm8(reg, rm, bytes)
        }
        // XADD r/mv, rv
        0xC1 => {
            let (reg, rm, bytes) = modrm(pos);
            *pos += bytes as u32;
            Opcode::XaddRmv(reg, rm, bytes)
        }

        // BSWAP r32
        0xC8..=0xCF => Opcode::Bswap(second - 0xC8),

        _ => Opcode::Unknown(second),
    }
}
