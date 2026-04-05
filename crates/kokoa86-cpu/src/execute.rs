use kokoa86_mem::{MemoryAccess, MemoryBus};
use crate::decode::{AluOp, Instruction, Opcode};
use crate::flags::*;
use crate::modrm::ModrmOperand;
use crate::regs::CpuState;

/// Port I/O callback trait
pub trait PortIo {
    fn port_in(&mut self, port: u16, size: u8) -> u32;
    fn port_out(&mut self, port: u16, size: u8, val: u32);
}

/// Software interrupt callback trait
pub trait IntHandler {
    /// Returns true if the interrupt was handled by a stub
    fn handle_int(&mut self, cpu: &mut CpuState, mem: &mut MemoryBus, vector: u8) -> bool;
}

/// Execute result
#[derive(Debug)]
pub enum ExecResult {
    Continue,
    Halt,
    UnknownOpcode(u8),
}

/// Execute a single decoded instruction
pub fn execute(
    cpu: &mut CpuState,
    mem: &mut MemoryBus,
    ports: &mut dyn PortIo,
    int_handler: &mut dyn IntHandler,
    inst: &Instruction,
) -> ExecResult {
    let next_ip = (cpu.eip as u16).wrapping_add(inst.len as u16);

    match &inst.op {
        // === MOV register immediate ===
        Opcode::MovReg8Imm(reg, imm) => {
            cpu.set_reg8(*reg, *imm);
            cpu.eip = next_ip as u32;
        }
        Opcode::MovReg16Imm(reg, imm) => {
            cpu.set_reg16(*reg, *imm);
            cpu.eip = next_ip as u32;
        }

        // === MOV with ModR/M (8-bit) ===
        // 0x88: MOV r/m8, r8  — direction: reg -> r/m
        // 0x8A: MOV r8, r/m8  — direction: r/m -> reg
        // We decode both as MovModrm8; need to differentiate by original opcode.
        // For now, we handle direction based on the original opcode byte.
        // Since decode stores them the same way, we'll handle via the opcode variants:
        Opcode::MovModrm8(reg, rm, _bytes) => {
            // This is used for both 0x88 and 0x8A.
            // We need to check the original instruction byte to know direction.
            // Peek at original opcode byte:
            let orig = mem.read_u8(cpu.cs_ip());
            if orig == 0x88 {
                // MOV r/m8, r8
                let val = cpu.get_reg8(*reg);
                write_rm8(cpu, mem, rm, val);
            } else {
                // MOV r8, r/m8
                let val = read_rm8(cpu, mem, rm);
                cpu.set_reg8(*reg, val);
            }
            cpu.eip = next_ip as u32;
        }
        Opcode::MovModrm16(reg, rm, _bytes) => {
            let orig = mem.read_u8(cpu.cs_ip());
            if orig == 0x89 {
                // MOV r/m16, r16
                let val = cpu.get_reg16(*reg);
                write_rm16(cpu, mem, rm, val);
            } else {
                // MOV r16, r/m16
                let val = read_rm16(cpu, mem, rm);
                cpu.set_reg16(*reg, val);
            }
            cpu.eip = next_ip as u32;
        }

        // MOV AL/AX, [addr]
        Opcode::MovAlMem(addr) => {
            let lin = cpu.linear_addr(cpu.ds, *addr);
            cpu.set_reg8(0, mem.read_u8(lin));
            cpu.eip = next_ip as u32;
        }
        Opcode::MovAxMem(addr) => {
            let lin = cpu.linear_addr(cpu.ds, *addr);
            cpu.set_reg16(0, mem.read_u16(lin));
            cpu.eip = next_ip as u32;
        }
        Opcode::MovMemAl(addr) => {
            let lin = cpu.linear_addr(cpu.ds, *addr);
            mem.write_u8(lin, cpu.get_reg8(0));
            cpu.eip = next_ip as u32;
        }
        Opcode::MovMemAx(addr) => {
            let lin = cpu.linear_addr(cpu.ds, *addr);
            mem.write_u16(lin, cpu.get_reg16(0));
            cpu.eip = next_ip as u32;
        }

        // MOV Sreg, r/m16
        Opcode::MovSregRm(reg, rm, _) => {
            let val = read_rm16(cpu, mem, rm);
            cpu.set_sreg(*reg, val);
            cpu.eip = next_ip as u32;
        }
        // MOV r/m16, Sreg
        Opcode::MovRmSreg(reg, rm, _) => {
            let val = cpu.get_sreg(*reg);
            write_rm16(cpu, mem, rm, val);
            cpu.eip = next_ip as u32;
        }

        // MOV r/m16, imm16
        Opcode::MovRmImm16(rm, _, imm) => {
            write_rm16(cpu, mem, rm, *imm);
            cpu.eip = next_ip as u32;
        }

        // === ALU operations ===
        Opcode::AluRmReg8(op, reg, rm, _) => {
            let a = read_rm8(cpu, mem, rm);
            let b = cpu.get_reg8(*reg);
            let result = exec_alu8(cpu, *op, a as u32, b as u32);
            if *op != AluOp::Cmp {
                write_rm8(cpu, mem, rm, result as u8);
            }
            cpu.eip = next_ip as u32;
        }
        Opcode::AluRmReg16(op, reg, rm, _) => {
            let a = read_rm16(cpu, mem, rm);
            let b = cpu.get_reg16(*reg);
            let result = exec_alu16(cpu, *op, a as u32, b as u32);
            if *op != AluOp::Cmp {
                write_rm16(cpu, mem, rm, result as u16);
            }
            cpu.eip = next_ip as u32;
        }
        Opcode::AluRegRm8(op, reg, rm, _) => {
            let a = cpu.get_reg8(*reg);
            let b = read_rm8(cpu, mem, rm);
            let result = exec_alu8(cpu, *op, a as u32, b as u32);
            if *op != AluOp::Cmp {
                cpu.set_reg8(*reg, result as u8);
            }
            cpu.eip = next_ip as u32;
        }
        Opcode::AluRegRm16(op, reg, rm, _) => {
            let a = cpu.get_reg16(*reg);
            let b = read_rm16(cpu, mem, rm);
            let result = exec_alu16(cpu, *op, a as u32, b as u32);
            if *op != AluOp::Cmp {
                cpu.set_reg16(*reg, result as u16);
            }
            cpu.eip = next_ip as u32;
        }
        Opcode::AluAlImm8(op, imm) => {
            let a = cpu.get_reg8(0);
            let result = exec_alu8(cpu, *op, a as u32, *imm as u32);
            if *op != AluOp::Cmp {
                cpu.set_reg8(0, result as u8);
            }
            cpu.eip = next_ip as u32;
        }
        Opcode::AluAxImm16(op, imm) => {
            let a = cpu.get_reg16(0);
            let result = exec_alu16(cpu, *op, a as u32, *imm as u32);
            if *op != AluOp::Cmp {
                cpu.set_reg16(0, result as u16);
            }
            cpu.eip = next_ip as u32;
        }
        Opcode::AluRmImm8(op, rm, _, imm) => {
            let a = read_rm8(cpu, mem, rm);
            let result = exec_alu8(cpu, *op, a as u32, *imm as u32);
            if *op != AluOp::Cmp {
                write_rm8(cpu, mem, rm, result as u8);
            }
            cpu.eip = next_ip as u32;
        }
        Opcode::AluRmImm16(op, rm, _, imm) => {
            let a = read_rm16(cpu, mem, rm);
            let result = exec_alu16(cpu, *op, a as u32, *imm as u32);
            if *op != AluOp::Cmp {
                write_rm16(cpu, mem, rm, result as u16);
            }
            cpu.eip = next_ip as u32;
        }
        Opcode::AluRmImm16s8(op, rm, _, imm) => {
            let a = read_rm16(cpu, mem, rm);
            let result = exec_alu16(cpu, *op, a as u32, *imm as u32);
            if *op != AluOp::Cmp {
                write_rm16(cpu, mem, rm, result as u16);
            }
            cpu.eip = next_ip as u32;
        }

        // === TEST ===
        Opcode::TestRmReg8(reg, rm, _) => {
            let a = read_rm8(cpu, mem, rm);
            let b = cpu.get_reg8(*reg);
            update_flags_logic(cpu, (a & b) as u32, 8);
            cpu.eip = next_ip as u32;
        }
        Opcode::TestRmReg16(reg, rm, _) => {
            let a = read_rm16(cpu, mem, rm);
            let b = cpu.get_reg16(*reg);
            update_flags_logic(cpu, (a & b) as u32, 16);
            cpu.eip = next_ip as u32;
        }
        Opcode::TestAlImm8(imm) => {
            let a = cpu.get_reg8(0);
            update_flags_logic(cpu, (a & imm) as u32, 8);
            cpu.eip = next_ip as u32;
        }
        Opcode::TestAxImm16(imm) => {
            let a = cpu.get_reg16(0);
            update_flags_logic(cpu, (a & imm) as u32, 16);
            cpu.eip = next_ip as u32;
        }

        // === INC/DEC ===
        Opcode::IncReg16(reg) => {
            let val = cpu.get_reg16(*reg);
            let result = (val as u32).wrapping_add(1);
            // INC doesn't affect CF
            let cf = get_flag(cpu, FLAG_CF);
            update_flags_add(cpu, val as u32, 1, result as u64, 16);
            set_flag(cpu, FLAG_CF, cf);
            cpu.set_reg16(*reg, result as u16);
            cpu.eip = next_ip as u32;
        }
        Opcode::DecReg16(reg) => {
            let val = cpu.get_reg16(*reg);
            let result = (val as u32).wrapping_sub(1);
            let cf = get_flag(cpu, FLAG_CF);
            update_flags_sub(cpu, val as u32, 1, result as u64, 16);
            set_flag(cpu, FLAG_CF, cf);
            cpu.set_reg16(*reg, result as u16);
            cpu.eip = next_ip as u32;
        }

        // === Stack ===
        Opcode::PushReg16(reg) => {
            let val = cpu.get_reg16(*reg);
            push16(cpu, mem, val);
            cpu.eip = next_ip as u32;
        }
        Opcode::PopReg16(reg) => {
            let val = pop16(cpu, mem);
            cpu.set_reg16(*reg, val);
            cpu.eip = next_ip as u32;
        }
        Opcode::PushImm16(imm) => {
            push16(cpu, mem, *imm);
            cpu.eip = next_ip as u32;
        }
        Opcode::PushImm8(imm) => {
            push16(cpu, mem, *imm as i8 as i16 as u16);
            cpu.eip = next_ip as u32;
        }
        Opcode::Pushf => {
            push16(cpu, mem, cpu.eflags as u16);
            cpu.eip = next_ip as u32;
        }
        Opcode::Popf => {
            let val = pop16(cpu, mem);
            cpu.eflags = (val as u32) | 0x0002; // bit 1 always set
            cpu.eip = next_ip as u32;
        }

        // === Control flow ===
        Opcode::JmpShort(disp) => {
            cpu.eip = next_ip.wrapping_add(*disp as i16 as u16) as u32;
        }
        Opcode::JmpNear(disp) => {
            cpu.eip = next_ip.wrapping_add(*disp as u16) as u32;
        }
        Opcode::Jcc(cc, disp) => {
            if check_condition(cpu, *cc) {
                cpu.eip = next_ip.wrapping_add(*disp as i16 as u16) as u32;
            } else {
                cpu.eip = next_ip as u32;
            }
        }
        Opcode::CallNear(disp) => {
            push16(cpu, mem, next_ip);
            cpu.eip = next_ip.wrapping_add(*disp as u16) as u32;
        }
        Opcode::Ret => {
            let ip = pop16(cpu, mem);
            cpu.eip = ip as u32;
        }
        Opcode::RetImm16(imm) => {
            let ip = pop16(cpu, mem);
            cpu.eip = ip as u32;
            let sp = cpu.get_reg16(4).wrapping_add(*imm);
            cpu.set_reg16(4, sp);
        }

        // === LOOP ===
        Opcode::Loop(disp) => {
            let cx = cpu.get_reg16(1).wrapping_sub(1); // CX--
            cpu.set_reg16(1, cx);
            if cx != 0 {
                cpu.eip = next_ip.wrapping_add(*disp as i16 as u16) as u32;
            } else {
                cpu.eip = next_ip as u32;
            }
        }
        Opcode::Loope(disp) => {
            let cx = cpu.get_reg16(1).wrapping_sub(1);
            cpu.set_reg16(1, cx);
            if cx != 0 && get_flag(cpu, FLAG_ZF) {
                cpu.eip = next_ip.wrapping_add(*disp as i16 as u16) as u32;
            } else {
                cpu.eip = next_ip as u32;
            }
        }
        Opcode::Loopne(disp) => {
            let cx = cpu.get_reg16(1).wrapping_sub(1);
            cpu.set_reg16(1, cx);
            if cx != 0 && !get_flag(cpu, FLAG_ZF) {
                cpu.eip = next_ip.wrapping_add(*disp as i16 as u16) as u32;
            } else {
                cpu.eip = next_ip as u32;
            }
        }

        // === Interrupts ===
        Opcode::Int(vec) => {
            cpu.eip = next_ip as u32;
            if !int_handler.handle_int(cpu, mem, *vec) {
                // Real interrupt dispatch via IVT (Phase 2)
                log::warn!("Unhandled INT 0x{:02X}", vec);
            }
        }
        Opcode::Iret => {
            let ip = pop16(cpu, mem);
            let cs = pop16(cpu, mem);
            let flags = pop16(cpu, mem);
            cpu.eip = ip as u32;
            cpu.cs = cs;
            cpu.eflags = (flags as u32) | 0x0002;
        }

        // === I/O ===
        Opcode::InAlImm8(port) => {
            let val = ports.port_in(*port as u16, 1);
            cpu.set_reg8(0, val as u8);
            cpu.eip = next_ip as u32;
        }
        Opcode::InAxImm8(port) => {
            let val = ports.port_in(*port as u16, 2);
            cpu.set_reg16(0, val as u16);
            cpu.eip = next_ip as u32;
        }
        Opcode::InAlDx => {
            let port = cpu.get_reg16(2); // DX
            let val = ports.port_in(port, 1);
            cpu.set_reg8(0, val as u8);
            cpu.eip = next_ip as u32;
        }
        Opcode::InAxDx => {
            let port = cpu.get_reg16(2);
            let val = ports.port_in(port, 2);
            cpu.set_reg16(0, val as u16);
            cpu.eip = next_ip as u32;
        }
        Opcode::OutImm8Al(port) => {
            let val = cpu.get_reg8(0);
            ports.port_out(*port as u16, 1, val as u32);
            cpu.eip = next_ip as u32;
        }
        Opcode::OutImm8Ax(port) => {
            let val = cpu.get_reg16(0);
            ports.port_out(*port as u16, 2, val as u32);
            cpu.eip = next_ip as u32;
        }
        Opcode::OutDxAl => {
            let port = cpu.get_reg16(2);
            let val = cpu.get_reg8(0);
            ports.port_out(port, 1, val as u32);
            cpu.eip = next_ip as u32;
        }
        Opcode::OutDxAx => {
            let port = cpu.get_reg16(2);
            let val = cpu.get_reg16(0);
            ports.port_out(port, 2, val as u32);
            cpu.eip = next_ip as u32;
        }

        // === LEA ===
        Opcode::Lea16(reg, rm, _) => {
            match rm {
                ModrmOperand::Mem(addr) => {
                    // LEA stores the offset, not the linear address
                    cpu.set_reg16(*reg, *addr as u16);
                }
                ModrmOperand::Reg(_) => {
                    // LEA with register is undefined, but typically treated as NOP
                }
            }
            cpu.eip = next_ip as u32;
        }

        // === XCHG ===
        Opcode::XchgAxReg(reg) => {
            let ax = cpu.get_reg16(0);
            let other = cpu.get_reg16(*reg);
            cpu.set_reg16(0, other);
            cpu.set_reg16(*reg, ax);
            cpu.eip = next_ip as u32;
        }

        // === CBW/CWD ===
        Opcode::Cbw => {
            let al = cpu.get_reg8(0) as i8 as i16 as u16;
            cpu.set_reg16(0, al);
            cpu.eip = next_ip as u32;
        }
        Opcode::Cwd => {
            let ax = cpu.get_reg16(0) as i16;
            if ax < 0 {
                cpu.set_reg16(2, 0xFFFF); // DX
            } else {
                cpu.set_reg16(2, 0x0000);
            }
            cpu.eip = next_ip as u32;
        }

        // === String operations ===
        Opcode::Movsb => {
            exec_movsb(cpu, mem);
            cpu.eip = next_ip as u32;
        }
        Opcode::Movsw => {
            exec_movsw(cpu, mem);
            cpu.eip = next_ip as u32;
        }
        Opcode::Stosb => {
            let addr = cpu.linear_addr(cpu.es, cpu.get_reg16(7)); // ES:DI
            mem.write_u8(addr, cpu.get_reg8(0));
            if get_flag(cpu, FLAG_DF) {
                cpu.set_reg16(7, cpu.get_reg16(7).wrapping_sub(1));
            } else {
                cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(1));
            }
            cpu.eip = next_ip as u32;
        }
        Opcode::Stosw => {
            let addr = cpu.linear_addr(cpu.es, cpu.get_reg16(7));
            mem.write_u16(addr, cpu.get_reg16(0));
            if get_flag(cpu, FLAG_DF) {
                cpu.set_reg16(7, cpu.get_reg16(7).wrapping_sub(2));
            } else {
                cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(2));
            }
            cpu.eip = next_ip as u32;
        }
        Opcode::Lodsb => {
            let addr = cpu.linear_addr(cpu.ds, cpu.get_reg16(6)); // DS:SI
            cpu.set_reg8(0, mem.read_u8(addr));
            if get_flag(cpu, FLAG_DF) {
                cpu.set_reg16(6, cpu.get_reg16(6).wrapping_sub(1));
            } else {
                cpu.set_reg16(6, cpu.get_reg16(6).wrapping_add(1));
            }
            cpu.eip = next_ip as u32;
        }
        Opcode::Lodsw => {
            let addr = cpu.linear_addr(cpu.ds, cpu.get_reg16(6));
            cpu.set_reg16(0, mem.read_u16(addr));
            if get_flag(cpu, FLAG_DF) {
                cpu.set_reg16(6, cpu.get_reg16(6).wrapping_sub(2));
            } else {
                cpu.set_reg16(6, cpu.get_reg16(6).wrapping_add(2));
            }
            cpu.eip = next_ip as u32;
        }

        // === REP ===
        Opcode::Rep(inner) => {
            // REP: repeat while CX != 0
            while cpu.get_reg16(1) != 0 {
                let tmp_inst = Instruction { op: (**inner).clone(), len: 0 };
                execute(cpu, mem, ports, int_handler, &tmp_inst);
                // Don't advance IP from inner (len=0)
                let cx = cpu.get_reg16(1).wrapping_sub(1);
                cpu.set_reg16(1, cx);
            }
            cpu.eip = next_ip as u32;
        }
        Opcode::Repne(inner) => {
            while cpu.get_reg16(1) != 0 {
                let tmp_inst = Instruction { op: (**inner).clone(), len: 0 };
                execute(cpu, mem, ports, int_handler, &tmp_inst);
                let cx = cpu.get_reg16(1).wrapping_sub(1);
                cpu.set_reg16(1, cx);
                if get_flag(cpu, FLAG_ZF) {
                    break;
                }
            }
            cpu.eip = next_ip as u32;
        }

        // === Group FF ===
        Opcode::GroupFF(sub, rm, _) => {
            match sub {
                0 => {
                    // INC r/m16
                    let val = read_rm16(cpu, mem, rm);
                    let result = (val as u32).wrapping_add(1);
                    let cf = get_flag(cpu, FLAG_CF);
                    update_flags_add(cpu, val as u32, 1, result as u64, 16);
                    set_flag(cpu, FLAG_CF, cf);
                    write_rm16(cpu, mem, rm, result as u16);
                }
                1 => {
                    // DEC r/m16
                    let val = read_rm16(cpu, mem, rm);
                    let result = (val as u32).wrapping_sub(1);
                    let cf = get_flag(cpu, FLAG_CF);
                    update_flags_sub(cpu, val as u32, 1, result as u64, 16);
                    set_flag(cpu, FLAG_CF, cf);
                    write_rm16(cpu, mem, rm, result as u16);
                }
                2 => {
                    // CALL r/m16 (near indirect)
                    let target = read_rm16(cpu, mem, rm);
                    push16(cpu, mem, next_ip);
                    cpu.eip = target as u32;
                    return ExecResult::Continue;
                }
                4 => {
                    // JMP r/m16 (near indirect)
                    let target = read_rm16(cpu, mem, rm);
                    cpu.eip = target as u32;
                    return ExecResult::Continue;
                }
                6 => {
                    // PUSH r/m16
                    let val = read_rm16(cpu, mem, rm);
                    push16(cpu, mem, val);
                }
                _ => {
                    log::warn!("Unimplemented Group FF sub-opcode: {}", sub);
                }
            }
            cpu.eip = next_ip as u32;
        }

        // === Misc ===
        Opcode::Nop => {
            cpu.eip = next_ip as u32;
        }
        Opcode::Hlt => {
            cpu.halted = true;
            cpu.eip = next_ip as u32;
            return ExecResult::Halt;
        }
        Opcode::Cli => {
            set_flag(cpu, FLAG_IF, false);
            cpu.eip = next_ip as u32;
        }
        Opcode::Sti => {
            set_flag(cpu, FLAG_IF, true);
            cpu.eip = next_ip as u32;
        }
        Opcode::Cld => {
            set_flag(cpu, FLAG_DF, false);
            cpu.eip = next_ip as u32;
        }
        Opcode::Std => {
            set_flag(cpu, FLAG_DF, true);
            cpu.eip = next_ip as u32;
        }
        Opcode::Clc => {
            set_flag(cpu, FLAG_CF, false);
            cpu.eip = next_ip as u32;
        }
        Opcode::Stc => {
            set_flag(cpu, FLAG_CF, true);
            cpu.eip = next_ip as u32;
        }
        Opcode::Cmc => {
            let cf = get_flag(cpu, FLAG_CF);
            set_flag(cpu, FLAG_CF, !cf);
            cpu.eip = next_ip as u32;
        }

        // Unused decode variants that shouldn't reach here
        Opcode::MovRegRm8 | Opcode::MovRegRm16 |
        Opcode::MovRmReg8 | Opcode::MovRmReg16 |
        Opcode::MovRmImm8 => {
            cpu.eip = next_ip as u32;
        }

        Opcode::Unknown(byte) => {
            log::error!("Unknown opcode: 0x{:02X} at {:04X}:{:04X}", byte, cpu.cs, cpu.eip);
            return ExecResult::UnknownOpcode(*byte);
        }
    }

    ExecResult::Continue
}

// === Helper functions ===

fn read_rm8(cpu: &CpuState, mem: &MemoryBus, rm: &ModrmOperand) -> u8 {
    match rm {
        ModrmOperand::Reg(idx) => cpu.get_reg8(*idx),
        ModrmOperand::Mem(addr) => mem.read_u8(*addr),
    }
}

fn read_rm16(cpu: &CpuState, mem: &MemoryBus, rm: &ModrmOperand) -> u16 {
    match rm {
        ModrmOperand::Reg(idx) => cpu.get_reg16(*idx),
        ModrmOperand::Mem(addr) => mem.read_u16(*addr),
    }
}

fn write_rm8(cpu: &mut CpuState, mem: &mut MemoryBus, rm: &ModrmOperand, val: u8) {
    match rm {
        ModrmOperand::Reg(idx) => cpu.set_reg8(*idx, val),
        ModrmOperand::Mem(addr) => mem.write_u8(*addr, val),
    }
}

fn write_rm16(cpu: &mut CpuState, mem: &mut MemoryBus, rm: &ModrmOperand, val: u16) {
    match rm {
        ModrmOperand::Reg(idx) => cpu.set_reg16(*idx, val),
        ModrmOperand::Mem(addr) => mem.write_u16(*addr, val),
    }
}

fn push16(cpu: &mut CpuState, mem: &mut MemoryBus, val: u16) {
    let sp = cpu.get_reg16(4).wrapping_sub(2);
    cpu.set_reg16(4, sp);
    let addr = cpu.linear_addr(cpu.ss, sp);
    mem.write_u16(addr, val);
}

fn pop16(cpu: &mut CpuState, mem: &mut MemoryBus) -> u16 {
    let sp = cpu.get_reg16(4);
    let addr = cpu.linear_addr(cpu.ss, sp);
    let val = mem.read_u16(addr);
    cpu.set_reg16(4, sp.wrapping_add(2));
    val
}

fn exec_alu8(cpu: &mut CpuState, op: AluOp, a: u32, b: u32) -> u32 {
    let result = match op {
        AluOp::Add => {
            let r = (a as u64).wrapping_add(b as u64);
            update_flags_add(cpu, a, b, r, 8);
            r as u32
        }
        AluOp::Or => {
            let r = a | b;
            update_flags_logic(cpu, r, 8);
            r
        }
        AluOp::Adc => {
            let carry = if get_flag(cpu, FLAG_CF) { 1u64 } else { 0 };
            let r = (a as u64).wrapping_add(b as u64).wrapping_add(carry);
            update_flags_add(cpu, a, b.wrapping_add(carry as u32), r, 8);
            r as u32
        }
        AluOp::Sbb => {
            let borrow = if get_flag(cpu, FLAG_CF) { 1u64 } else { 0 };
            let r = (a as u64).wrapping_sub(b as u64).wrapping_sub(borrow);
            update_flags_sub(cpu, a, b.wrapping_add(borrow as u32), r, 8);
            r as u32
        }
        AluOp::And => {
            let r = a & b;
            update_flags_logic(cpu, r, 8);
            r
        }
        AluOp::Sub | AluOp::Cmp => {
            let r = (a as u64).wrapping_sub(b as u64);
            update_flags_sub(cpu, a, b, r, 8);
            r as u32
        }
        AluOp::Xor => {
            let r = a ^ b;
            update_flags_logic(cpu, r, 8);
            r
        }
    };
    result & 0xFF
}

fn exec_alu16(cpu: &mut CpuState, op: AluOp, a: u32, b: u32) -> u32 {
    let result = match op {
        AluOp::Add => {
            let r = (a as u64).wrapping_add(b as u64);
            update_flags_add(cpu, a, b, r, 16);
            r as u32
        }
        AluOp::Or => {
            let r = a | b;
            update_flags_logic(cpu, r, 16);
            r
        }
        AluOp::Adc => {
            let carry = if get_flag(cpu, FLAG_CF) { 1u64 } else { 0 };
            let r = (a as u64).wrapping_add(b as u64).wrapping_add(carry);
            update_flags_add(cpu, a, b.wrapping_add(carry as u32), r, 16);
            r as u32
        }
        AluOp::Sbb => {
            let borrow = if get_flag(cpu, FLAG_CF) { 1u64 } else { 0 };
            let r = (a as u64).wrapping_sub(b as u64).wrapping_sub(borrow);
            update_flags_sub(cpu, a, b.wrapping_add(borrow as u32), r, 16);
            r as u32
        }
        AluOp::And => {
            let r = a & b;
            update_flags_logic(cpu, r, 16);
            r
        }
        AluOp::Sub | AluOp::Cmp => {
            let r = (a as u64).wrapping_sub(b as u64);
            update_flags_sub(cpu, a, b, r, 16);
            r as u32
        }
        AluOp::Xor => {
            let r = a ^ b;
            update_flags_logic(cpu, r, 16);
            r
        }
    };
    result & 0xFFFF
}

fn exec_movsb(cpu: &mut CpuState, mem: &mut MemoryBus) {
    let src = cpu.linear_addr(cpu.ds, cpu.get_reg16(6)); // DS:SI
    let dst = cpu.linear_addr(cpu.es, cpu.get_reg16(7)); // ES:DI
    let val = mem.read_u8(src);
    mem.write_u8(dst, val);
    if get_flag(cpu, FLAG_DF) {
        cpu.set_reg16(6, cpu.get_reg16(6).wrapping_sub(1));
        cpu.set_reg16(7, cpu.get_reg16(7).wrapping_sub(1));
    } else {
        cpu.set_reg16(6, cpu.get_reg16(6).wrapping_add(1));
        cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(1));
    }
}

fn exec_movsw(cpu: &mut CpuState, mem: &mut MemoryBus) {
    let src = cpu.linear_addr(cpu.ds, cpu.get_reg16(6));
    let dst = cpu.linear_addr(cpu.es, cpu.get_reg16(7));
    let val = mem.read_u16(src);
    mem.write_u16(dst, val);
    if get_flag(cpu, FLAG_DF) {
        cpu.set_reg16(6, cpu.get_reg16(6).wrapping_sub(2));
        cpu.set_reg16(7, cpu.get_reg16(7).wrapping_sub(2));
    } else {
        cpu.set_reg16(6, cpu.get_reg16(6).wrapping_add(2));
        cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(2));
    }
}
