use kokoa86_mem::{MemoryAccess, MemoryBus};
use crate::modrm::{ModrmOperand, decode_modrm16};
use crate::regs::CpuState;

/// Decoded instruction
#[derive(Debug, Clone)]
pub struct Instruction {
    pub op: Opcode,
    pub len: u8, // instruction length in bytes
}

#[derive(Debug, Clone)]
pub enum Opcode {
    // Data movement
    MovRegRm8,               // 0x8A: MOV r8, r/m8
    MovRegRm16,              // 0x8B: MOV r16, r/m16
    MovRmReg8,               // 0x88: MOV r/m8, r8
    MovRmReg16,              // 0x89: MOV r/m16, r16
    MovReg8Imm(u8, u8),      // 0xB0-0xB7: MOV r8, imm8
    MovReg16Imm(u8, u16),    // 0xB8-0xBF: MOV r16, imm16
    MovRmImm8,               // 0xC6: MOV r/m8, imm8
    MovRmImm16(ModrmOperand, u8, u16), // 0xC7: MOV r/m16, imm16
    MovAlMem(u16),           // 0xA0: MOV AL, [addr]
    MovAxMem(u16),           // 0xA1: MOV AX, [addr]
    MovMemAl(u16),           // 0xA2: MOV [addr], AL
    MovMemAx(u16),           // 0xA3: MOV [addr], AX
    MovSregRm(u8, ModrmOperand, u8),  // 0x8E: MOV Sreg, r/m16
    MovRmSreg(u8, ModrmOperand, u8),  // 0x8C: MOV r/m16, Sreg

    // ALU: (reg_field, rm_operand, modrm_bytes)
    AluRmReg8(AluOp, u8, ModrmOperand, u8),
    AluRmReg16(AluOp, u8, ModrmOperand, u8),
    AluRegRm8(AluOp, u8, ModrmOperand, u8),
    AluRegRm16(AluOp, u8, ModrmOperand, u8),
    AluAlImm8(AluOp, u8),
    AluAxImm16(AluOp, u16),
    // Group 0x80-0x83
    AluRmImm8(AluOp, ModrmOperand, u8, u8),     // r/m8, imm8
    AluRmImm16(AluOp, ModrmOperand, u8, u16),   // r/m16, imm16
    AluRmImm16s8(AluOp, ModrmOperand, u8, u16), // r/m16, sign-extended imm8

    // ModR/M data for non-group MOV
    MovModrm8(u8, ModrmOperand, u8),   // reg, operand, modrm_bytes
    MovModrm16(u8, ModrmOperand, u8),

    // INC/DEC register
    IncReg16(u8),   // 0x40-0x47
    DecReg16(u8),   // 0x48-0x4F

    // Stack
    PushReg16(u8),  // 0x50-0x57
    PopReg16(u8),   // 0x58-0x5F
    PushImm16(u16), // 0x68
    PushImm8(u8),   // 0x6A

    // Control flow
    JmpShort(i8),       // 0xEB
    JmpNear(i16),       // 0xE9
    Jcc(u8, i8),        // 0x70-0x7F: condition code, displacement
    CallNear(i16),      // 0xE8
    Ret,                // 0xC3
    RetImm16(u16),      // 0xC2

    // Interrupts
    Int(u8),            // 0xCD
    Iret,               // 0xCF

    // I/O
    InAlImm8(u8),      // 0xE4
    InAxImm8(u8),      // 0xE5
    OutImm8Al(u8),     // 0xE6
    OutImm8Ax(u8),     // 0xE7
    InAlDx,            // 0xEC
    InAxDx,            // 0xED
    OutDxAl,           // 0xEE
    OutDxAx,           // 0xEF

    // LEA
    Lea16(u8, ModrmOperand, u8), // 0x8D

    // XCHG
    XchgAxReg(u8),     // 0x90-0x97 (0x90 = NOP)

    // Misc
    Nop,
    Hlt,
    Cli,
    Sti,
    Cld,
    Std,
    Cmc,
    Clc,
    Stc,

    // String operations
    Movsb,  // 0xA4
    Movsw,  // 0xA5
    Stosb,  // 0xAA
    Stosw,  // 0xAB
    Lodsb,  // 0xAC
    Lodsw,  // 0xAD

    // REP prefix + string op
    Rep(Box<Opcode>),
    Repne(Box<Opcode>),

    // LOOP
    Loop(i8),       // 0xE2
    Loope(i8),      // 0xE1
    Loopne(i8),     // 0xE0

    // CBW/CWD
    Cbw,  // 0x98
    Cwd,  // 0x99

    // TEST
    TestRmReg8(u8, ModrmOperand, u8),
    TestRmReg16(u8, ModrmOperand, u8),
    TestAlImm8(u8),
    TestAxImm16(u16),

    // Group FF
    GroupFF(u8, ModrmOperand, u8), // sub-opcode, operand, modrm_bytes

    // PUSHF/POPF
    Pushf,
    Popf,

    /// Unimplemented opcode (for graceful error)
    Unknown(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AluOp {
    Add = 0,
    Or = 1,
    Adc = 2,
    Sbb = 3,
    And = 4,
    Sub = 5,
    Xor = 6,
    Cmp = 7,
}

impl AluOp {
    pub fn from_reg(reg: u8) -> Self {
        match reg {
            0 => AluOp::Add,
            1 => AluOp::Or,
            2 => AluOp::Adc,
            3 => AluOp::Sbb,
            4 => AluOp::And,
            5 => AluOp::Sub,
            6 => AluOp::Xor,
            7 => AluOp::Cmp,
            _ => unreachable!(),
        }
    }
}

/// Decode the next instruction at CS:IP
pub fn decode(cpu: &CpuState, mem: &MemoryBus) -> Instruction {
    let base = cpu.cs_ip();
    let mut pos: u32 = 0;

    let fetch8 = |off: u32| -> u8 { mem.read_u8(base.wrapping_add(off)) };
    // Handle REP/REPNE prefix
    let opcode_byte = fetch8(pos);

    if opcode_byte == 0xF3 || opcode_byte == 0xF2 {
        pos += 1;
        let inner = decode_at(cpu, mem, base, &mut pos);
        let wrapped = if opcode_byte == 0xF3 {
            Opcode::Rep(Box::new(inner.op))
        } else {
            Opcode::Repne(Box::new(inner.op))
        };
        return Instruction { op: wrapped, len: inner.len + 1 };
    }

    decode_at(cpu, mem, base, &mut pos)
}

fn decode_at(cpu: &CpuState, mem: &MemoryBus, base: u32, pos: &mut u32) -> Instruction {
    let fetch8 = |off: u32| -> u8 { mem.read_u8(base.wrapping_add(off)) };
    let fetch16 = |off: u32| -> u16 { mem.read_u16(base.wrapping_add(off)) };

    let opcode_byte = fetch8(*pos);
    *pos += 1;

    let op = match opcode_byte {
        // ALU r/m8, r8
        0x00 | 0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 => {
            let alu = AluOp::from_reg((opcode_byte >> 3) & 7);
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            Opcode::AluRmReg8(alu, reg, rm, bytes)
        }
        // ALU r/m16, r16
        0x01 | 0x09 | 0x11 | 0x19 | 0x21 | 0x29 | 0x31 | 0x39 => {
            let alu = AluOp::from_reg((opcode_byte >> 3) & 7);
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            Opcode::AluRmReg16(alu, reg, rm, bytes)
        }
        // ALU r8, r/m8
        0x02 | 0x0A | 0x12 | 0x1A | 0x22 | 0x2A | 0x32 | 0x3A => {
            let alu = AluOp::from_reg((opcode_byte >> 3) & 7);
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            Opcode::AluRegRm8(alu, reg, rm, bytes)
        }
        // ALU r16, r/m16
        0x03 | 0x0B | 0x13 | 0x1B | 0x23 | 0x2B | 0x33 | 0x3B => {
            let alu = AluOp::from_reg((opcode_byte >> 3) & 7);
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            Opcode::AluRegRm16(alu, reg, rm, bytes)
        }
        // ALU AL, imm8
        0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C => {
            let alu = AluOp::from_reg((opcode_byte >> 3) & 7);
            let imm = fetch8(*pos);
            *pos += 1;
            Opcode::AluAlImm8(alu, imm)
        }
        // ALU AX, imm16
        0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D => {
            let alu = AluOp::from_reg((opcode_byte >> 3) & 7);
            let imm = fetch16(*pos);
            *pos += 2;
            Opcode::AluAxImm16(alu, imm)
        }

        // INC r16
        0x40..=0x47 => Opcode::IncReg16(opcode_byte - 0x40),
        // DEC r16
        0x48..=0x4F => Opcode::DecReg16(opcode_byte - 0x48),

        // PUSH r16
        0x50..=0x57 => Opcode::PushReg16(opcode_byte - 0x50),
        // POP r16
        0x58..=0x5F => Opcode::PopReg16(opcode_byte - 0x58),

        // PUSH imm16
        0x68 => {
            let imm = fetch16(*pos);
            *pos += 2;
            Opcode::PushImm16(imm)
        }
        // PUSH imm8 (sign-extended)
        0x6A => {
            let imm = fetch8(*pos);
            *pos += 1;
            Opcode::PushImm8(imm)
        }

        // Jcc short
        0x70..=0x7F => {
            let cc = opcode_byte - 0x70;
            let disp = fetch8(*pos) as i8;
            *pos += 1;
            Opcode::Jcc(cc, disp)
        }

        // Group 0x80: ALU r/m8, imm8
        0x80 => {
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            let imm = fetch8(*pos);
            *pos += 1;
            Opcode::AluRmImm8(AluOp::from_reg(reg), rm, bytes, imm)
        }
        // Group 0x81: ALU r/m16, imm16
        0x81 => {
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            let imm = fetch16(*pos);
            *pos += 2;
            Opcode::AluRmImm16(AluOp::from_reg(reg), rm, bytes, imm)
        }
        // Group 0x83: ALU r/m16, sign-extended imm8
        0x83 => {
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            let imm = fetch8(*pos) as i8 as i16 as u16;
            *pos += 1;
            Opcode::AluRmImm16s8(AluOp::from_reg(reg), rm, bytes, imm)
        }

        // TEST r/m8, r8
        0x84 => {
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            Opcode::TestRmReg8(reg, rm, bytes)
        }
        // TEST r/m16, r16
        0x85 => {
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            Opcode::TestRmReg16(reg, rm, bytes)
        }

        // MOV r/m8, r8
        0x88 => {
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            Opcode::MovModrm8(reg, rm, bytes)
        }
        // MOV r/m16, r16
        0x89 => {
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            Opcode::MovModrm16(reg, rm, bytes)
        }
        // MOV r8, r/m8
        0x8A => {
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            Opcode::MovModrm8(reg, rm, bytes)
        }
        // MOV r16, r/m16
        0x8B => {
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            Opcode::MovModrm16(reg, rm, bytes)
        }
        // MOV r/m16, Sreg
        0x8C => {
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            Opcode::MovRmSreg(reg, rm, bytes)
        }
        // LEA r16, m
        0x8D => {
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            Opcode::Lea16(reg, rm, bytes)
        }
        // MOV Sreg, r/m16
        0x8E => {
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            Opcode::MovSregRm(reg, rm, bytes)
        }

        // NOP / XCHG AX, r16
        0x90 => Opcode::Nop,
        0x91..=0x97 => Opcode::XchgAxReg(opcode_byte - 0x90),

        // CBW
        0x98 => Opcode::Cbw,
        // CWD
        0x99 => Opcode::Cwd,

        // PUSHF
        0x9C => Opcode::Pushf,
        // POPF
        0x9D => Opcode::Popf,

        // MOV AL, [addr]
        0xA0 => {
            let addr = fetch16(*pos);
            *pos += 2;
            Opcode::MovAlMem(addr)
        }
        // MOV AX, [addr]
        0xA1 => {
            let addr = fetch16(*pos);
            *pos += 2;
            Opcode::MovAxMem(addr)
        }
        // MOV [addr], AL
        0xA2 => {
            let addr = fetch16(*pos);
            *pos += 2;
            Opcode::MovMemAl(addr)
        }
        // MOV [addr], AX
        0xA3 => {
            let addr = fetch16(*pos);
            *pos += 2;
            Opcode::MovMemAx(addr)
        }

        // MOVSB/MOVSW
        0xA4 => Opcode::Movsb,
        0xA5 => Opcode::Movsw,

        // TEST AL, imm8
        0xA8 => {
            let imm = fetch8(*pos);
            *pos += 1;
            Opcode::TestAlImm8(imm)
        }
        // TEST AX, imm16
        0xA9 => {
            let imm = fetch16(*pos);
            *pos += 2;
            Opcode::TestAxImm16(imm)
        }

        // STOSB/STOSW
        0xAA => Opcode::Stosb,
        0xAB => Opcode::Stosw,
        // LODSB/LODSW
        0xAC => Opcode::Lodsb,
        0xAD => Opcode::Lodsw,

        // MOV r8, imm8
        0xB0..=0xB7 => {
            let reg = opcode_byte - 0xB0;
            let imm = fetch8(*pos);
            *pos += 1;
            Opcode::MovReg8Imm(reg, imm)
        }
        // MOV r16, imm16
        0xB8..=0xBF => {
            let reg = opcode_byte - 0xB8;
            let imm = fetch16(*pos);
            *pos += 2;
            Opcode::MovReg16Imm(reg, imm)
        }

        // RET imm16
        0xC2 => {
            let imm = fetch16(*pos);
            *pos += 2;
            Opcode::RetImm16(imm)
        }
        // RET
        0xC3 => Opcode::Ret,

        // MOV r/m8, imm8
        0xC6 => {
            let (_reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            let imm = fetch8(*pos);
            *pos += 1;
            Opcode::AluRmImm8(AluOp::Add, rm, bytes, imm) // Reuse — will handle in execute as MOV
        }
        // MOV r/m16, imm16
        0xC7 => {
            let (_reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            let imm = fetch16(*pos);
            *pos += 2;
            Opcode::MovRmImm16(rm, bytes, imm)
        }

        // INT imm8
        0xCD => {
            let vec = fetch8(*pos);
            *pos += 1;
            Opcode::Int(vec)
        }
        // IRET
        0xCF => Opcode::Iret,

        // LOOPNE
        0xE0 => {
            let disp = fetch8(*pos) as i8;
            *pos += 1;
            Opcode::Loopne(disp)
        }
        // LOOPE
        0xE1 => {
            let disp = fetch8(*pos) as i8;
            *pos += 1;
            Opcode::Loope(disp)
        }
        // LOOP
        0xE2 => {
            let disp = fetch8(*pos) as i8;
            *pos += 1;
            Opcode::Loop(disp)
        }

        // IN AL, imm8
        0xE4 => {
            let port = fetch8(*pos);
            *pos += 1;
            Opcode::InAlImm8(port)
        }
        // IN AX, imm8
        0xE5 => {
            let port = fetch8(*pos);
            *pos += 1;
            Opcode::InAxImm8(port)
        }
        // OUT imm8, AL
        0xE6 => {
            let port = fetch8(*pos);
            *pos += 1;
            Opcode::OutImm8Al(port)
        }
        // OUT imm8, AX
        0xE7 => {
            let port = fetch8(*pos);
            *pos += 1;
            Opcode::OutImm8Ax(port)
        }

        // CALL near
        0xE8 => {
            let disp = fetch16(*pos) as i16;
            *pos += 2;
            Opcode::CallNear(disp)
        }
        // JMP near
        0xE9 => {
            let disp = fetch16(*pos) as i16;
            *pos += 2;
            Opcode::JmpNear(disp)
        }
        // JMP short
        0xEB => {
            let disp = fetch8(*pos) as i8;
            *pos += 1;
            Opcode::JmpShort(disp)
        }

        // IN AL, DX
        0xEC => Opcode::InAlDx,
        // IN AX, DX
        0xED => Opcode::InAxDx,
        // OUT DX, AL
        0xEE => Opcode::OutDxAl,
        // OUT DX, AX
        0xEF => Opcode::OutDxAx,

        // HLT
        0xF4 => Opcode::Hlt,

        // CMC
        0xF5 => Opcode::Cmc,

        // CLC
        0xF8 => Opcode::Clc,
        // STC
        0xF9 => Opcode::Stc,
        // CLI
        0xFA => Opcode::Cli,
        // STI
        0xFB => Opcode::Sti,
        // CLD
        0xFC => Opcode::Cld,
        // STD
        0xFD => Opcode::Std,

        // Group 0xFF
        0xFF => {
            let (reg, rm, bytes) = decode_modrm16(cpu, mem, base + *pos);
            *pos += bytes as u32;
            Opcode::GroupFF(reg, rm, bytes)
        }

        _ => Opcode::Unknown(opcode_byte),
    };

    Instruction { op, len: *pos as u8 }
}

/// Special MovRmImm16 that wraps rm + bytes + imm
#[derive(Debug, Clone)]
pub struct MovRmImm16Data {
    pub rm: ModrmOperand,
    pub modrm_bytes: u8,
    pub imm: u16,
}
