use kokoa86_mem::{MemoryAccess, MemoryBus};
use crate::decode::*;
use crate::flags::*;
use crate::modrm::ModrmOperand;
use crate::regs::{CpuState, OperandSize};

/// Port I/O callback trait
pub trait PortIo {
    fn port_in(&mut self, port: u16, size: u8) -> u32;
    fn port_out(&mut self, port: u16, size: u8, val: u32);
}

/// Software interrupt callback trait
pub trait IntHandler {
    fn handle_int(&mut self, cpu: &mut CpuState, mem: &mut MemoryBus, vector: u8) -> bool;
}

#[derive(Debug)]
pub enum ExecResult {
    Continue,
    Halt,
    UnknownOpcode(u8),
    DivideError,
}

pub fn execute(
    cpu: &mut CpuState,
    mem: &mut MemoryBus,
    ports: &mut dyn PortIo,
    int_handler: &mut dyn IntHandler,
    inst: &Instruction,
) -> ExecResult {
    let is32 = inst.operand_size == OperandSize::Dword32;
    let width: u8 = if is32 { 32 } else { 16 };

    // Use CS.D bit to determine EIP width (NOT operand size prefix)
    let eip_32bit = cpu.cs_cache.big || cpu.mode == crate::regs::CpuMode::ProtectedMode;

    // Compute next IP (after this instruction)
    let next_ip = if eip_32bit {
        cpu.eip.wrapping_add(inst.len as u32)
    } else {
        (cpu.eip as u16).wrapping_add(inst.len as u16) as u32
    };

    match &inst.op {
        // === MOV register immediate ===
        Opcode::MovReg8Imm(reg, imm) => {
            cpu.set_reg8(*reg, *imm);
            cpu.eip = next_ip;
        }
        Opcode::MovRegvImm(reg, imm) => {
            if is32 { cpu.set_reg32(*reg, *imm); } else { cpu.set_reg16(*reg, *imm as u16); }
            cpu.eip = next_ip;
        }

        // === MOV ModR/M 8-bit ===
        Opcode::MovModrm8(reg, rm, _) => {
            let orig = mem.read_u8(cpu.cs_ip());
            if orig == 0x88 {
                let val = cpu.get_reg8(*reg);
                write_rm8(cpu, mem, rm, val);
            } else {
                let val = read_rm8(cpu, mem, rm);
                cpu.set_reg8(*reg, val);
            }
            cpu.eip = next_ip;
        }
        Opcode::MovModrmv(reg, rm, _) => {
            let orig = mem.read_u8(cpu.cs_ip());
            if orig == 0x89 || (orig >= 0x26 && orig <= 0x65) {
                // Direction: reg -> r/m. Also handle prefixed case.
                // Check if after prefix bytes the opcode is 0x89
                let actual = find_opcode_byte(cpu, mem, inst);
                if actual == 0x89 {
                    if is32 {
                        let val = cpu.get_reg32(*reg);
                        write_rm32(cpu, mem, rm, val);
                    } else {
                        let val = cpu.get_reg16(*reg);
                        write_rm16(cpu, mem, rm, val);
                    }
                } else {
                    if is32 {
                        let val = read_rm32(cpu, mem, rm);
                        cpu.set_reg32(*reg, val);
                    } else {
                        let val = read_rm16(cpu, mem, rm);
                        cpu.set_reg16(*reg, val);
                    }
                }
            } else {
                if is32 {
                    let val = read_rm32(cpu, mem, rm);
                    cpu.set_reg32(*reg, val);
                } else {
                    let val = read_rm16(cpu, mem, rm);
                    cpu.set_reg16(*reg, val);
                }
            }
            cpu.eip = next_ip;
        }

        Opcode::MovRmImm8(rm, _, imm) => {
            write_rm8(cpu, mem, rm, *imm);
            cpu.eip = next_ip;
        }
        Opcode::MovRmImmv(rm, _, imm) => {
            if is32 { write_rm32(cpu, mem, rm, *imm); } else { write_rm16(cpu, mem, rm, *imm as u16); }
            cpu.eip = next_ip;
        }

        Opcode::MovAlMem(addr) => {
            let lin = cpu.linear_addr(cpu.ds, *addr as u16);
            cpu.set_reg8(0, mem.read_u8(lin));
            cpu.eip = next_ip;
        }
        Opcode::MovAxMem(addr) => {
            let lin = cpu.linear_addr(cpu.ds, *addr as u16);
            if is32 { cpu.set_reg32(0, mem.read_u32(lin)); } else { cpu.set_reg16(0, mem.read_u16(lin)); }
            cpu.eip = next_ip;
        }
        Opcode::MovMemAl(addr) => {
            let lin = cpu.linear_addr(cpu.ds, *addr as u16);
            mem.write_u8(lin, cpu.get_reg8(0));
            cpu.eip = next_ip;
        }
        Opcode::MovMemAx(addr) => {
            let lin = cpu.linear_addr(cpu.ds, *addr as u16);
            if is32 { mem.write_u32(lin, cpu.get_reg32(0)); } else { mem.write_u16(lin, cpu.get_reg16(0)); }
            cpu.eip = next_ip;
        }

        Opcode::MovSregRm(reg, rm, _) => {
            let val = read_rmv(cpu, mem, rm, false) as u16;
            crate::descriptor::load_segment(cpu, mem, *reg, val);
            cpu.eip = next_ip;
        }
        Opcode::MovRmSreg(reg, rm, _) => {
            let val = cpu.get_sreg(*reg);
            write_rm16(cpu, mem, rm, val);
            cpu.eip = next_ip;
        }

        // === ALU ===
        Opcode::AluRmReg8(op, reg, rm, _) => {
            let a = read_rm8(cpu, mem, rm) as u32;
            let b = cpu.get_reg8(*reg) as u32;
            let result = exec_alu(cpu, *op, a, b, 8);
            if *op != AluOp::Cmp { write_rm8(cpu, mem, rm, result as u8); }
            cpu.eip = next_ip;
        }
        Opcode::AluRmRegv(op, reg, rm, _) => {
            let a = read_rmv(cpu, mem, rm, is32);
            let b = if is32 { cpu.get_reg32(*reg) } else { cpu.get_reg16(*reg) as u32 };
            let result = exec_alu(cpu, *op, a, b, width);
            if *op != AluOp::Cmp { write_rmv(cpu, mem, rm, result, is32); }
            cpu.eip = next_ip;
        }
        Opcode::AluRegRm8(op, reg, rm, _) => {
            let a = cpu.get_reg8(*reg) as u32;
            let b = read_rm8(cpu, mem, rm) as u32;
            let result = exec_alu(cpu, *op, a, b, 8);
            if *op != AluOp::Cmp { cpu.set_reg8(*reg, result as u8); }
            cpu.eip = next_ip;
        }
        Opcode::AluRegRmv(op, reg, rm, _) => {
            let a = if is32 { cpu.get_reg32(*reg) } else { cpu.get_reg16(*reg) as u32 };
            let b = read_rmv(cpu, mem, rm, is32);
            let result = exec_alu(cpu, *op, a, b, width);
            if *op != AluOp::Cmp {
                if is32 { cpu.set_reg32(*reg, result); } else { cpu.set_reg16(*reg, result as u16); }
            }
            cpu.eip = next_ip;
        }
        Opcode::AluAlImm8(op, imm) => {
            let a = cpu.get_reg8(0) as u32;
            let result = exec_alu(cpu, *op, a, *imm as u32, 8);
            if *op != AluOp::Cmp { cpu.set_reg8(0, result as u8); }
            cpu.eip = next_ip;
        }
        Opcode::AluAxImmv(op, imm) => {
            let a = if is32 { cpu.get_reg32(0) } else { cpu.get_reg16(0) as u32 };
            let result = exec_alu(cpu, *op, a, *imm, width);
            if *op != AluOp::Cmp {
                if is32 { cpu.set_reg32(0, result); } else { cpu.set_reg16(0, result as u16); }
            }
            cpu.eip = next_ip;
        }
        Opcode::AluRmImm8(op, rm, _, imm) => {
            let a = read_rm8(cpu, mem, rm) as u32;
            let result = exec_alu(cpu, *op, a, *imm as u32, 8);
            if *op != AluOp::Cmp { write_rm8(cpu, mem, rm, result as u8); }
            cpu.eip = next_ip;
        }
        Opcode::AluRmImmv(op, rm, _, imm) => {
            let a = read_rmv(cpu, mem, rm, is32);
            let result = exec_alu(cpu, *op, a, *imm, width);
            if *op != AluOp::Cmp { write_rmv(cpu, mem, rm, result, is32); }
            cpu.eip = next_ip;
        }
        Opcode::AluRmImmvs8(op, rm, _, imm) => {
            let a = read_rmv(cpu, mem, rm, is32);
            let result = exec_alu(cpu, *op, a, *imm, width);
            if *op != AluOp::Cmp { write_rmv(cpu, mem, rm, result, is32); }
            cpu.eip = next_ip;
        }

        // === TEST ===
        Opcode::TestRmReg8(reg, rm, _) => {
            let a = read_rm8(cpu, mem, rm);
            let b = cpu.get_reg8(*reg);
            update_flags_logic(cpu, (a & b) as u32, 8);
            cpu.eip = next_ip;
        }
        Opcode::TestRmRegv(reg, rm, _) => {
            let a = read_rmv(cpu, mem, rm, is32);
            let b = if is32 { cpu.get_reg32(*reg) } else { cpu.get_reg16(*reg) as u32 };
            update_flags_logic(cpu, a & b, width);
            cpu.eip = next_ip;
        }
        Opcode::TestAlImm8(imm) => {
            update_flags_logic(cpu, (cpu.get_reg8(0) & imm) as u32, 8);
            cpu.eip = next_ip;
        }
        Opcode::TestAxImmv(imm) => {
            let a = if is32 { cpu.get_reg32(0) } else { cpu.get_reg16(0) as u32 };
            update_flags_logic(cpu, a & imm, width);
            cpu.eip = next_ip;
        }

        // === INC/DEC ===
        Opcode::IncRegv(reg) => {
            let val = if is32 { cpu.get_reg32(*reg) } else { cpu.get_reg16(*reg) as u32 };
            let result = val.wrapping_add(1);
            let cf = get_flag(cpu, FLAG_CF);
            update_flags_add(cpu, val, 1, result as u64, width);
            set_flag(cpu, FLAG_CF, cf);
            if is32 { cpu.set_reg32(*reg, result); } else { cpu.set_reg16(*reg, result as u16); }
            cpu.eip = next_ip;
        }
        Opcode::DecRegv(reg) => {
            let val = if is32 { cpu.get_reg32(*reg) } else { cpu.get_reg16(*reg) as u32 };
            let result = val.wrapping_sub(1);
            let cf = get_flag(cpu, FLAG_CF);
            update_flags_sub(cpu, val, 1, result as u64, width);
            set_flag(cpu, FLAG_CF, cf);
            if is32 { cpu.set_reg32(*reg, result); } else { cpu.set_reg16(*reg, result as u16); }
            cpu.eip = next_ip;
        }

        // === Stack ===
        Opcode::PushRegv(reg) => {
            let val = if is32 { cpu.get_reg32(*reg) } else { cpu.get_reg16(*reg) as u32 };
            pushv(cpu, mem, val, is32);
            cpu.eip = next_ip;
        }
        Opcode::PopRegv(reg) => {
            let val = popv(cpu, mem, is32);
            if is32 { cpu.set_reg32(*reg, val); } else { cpu.set_reg16(*reg, val as u16); }
            cpu.eip = next_ip;
        }
        Opcode::PushImmv(imm) => {
            pushv(cpu, mem, *imm, is32);
            cpu.eip = next_ip;
        }
        Opcode::PushImm8(imm) => {
            let val = if is32 { *imm as i8 as i32 as u32 } else { *imm as i8 as i16 as u16 as u32 };
            pushv(cpu, mem, val, is32);
            cpu.eip = next_ip;
        }
        Opcode::Pushf => {
            if is32 { push32(cpu, mem, cpu.eflags); } else { push16(cpu, mem, cpu.eflags as u16); }
            cpu.eip = next_ip;
        }
        Opcode::Popf => {
            let val = if is32 { pop32(cpu, mem) } else { pop16(cpu, mem) as u32 };
            cpu.eflags = (val & 0x00FCFFFF) | 0x0002;
            cpu.eip = next_ip;
        }

        // === Control flow ===
        Opcode::JmpShort(disp) => {
            cpu.eip = next_ip.wrapping_add(*disp as i32 as u32);
            if !eip_32bit { cpu.eip &= 0xFFFF; }
        }
        Opcode::JmpNearRel(disp) => {
            cpu.eip = next_ip.wrapping_add(*disp as u32);
            if !eip_32bit { cpu.eip &= 0xFFFF; }
        }
        Opcode::JmpFar(seg, offset) => {
            crate::descriptor::load_segment(cpu, mem, 1, *seg); // CS = index 1
            cpu.eip = *offset;
        }
        Opcode::Jcc(cc, disp) => {
            if check_condition(cpu, *cc) {
                cpu.eip = next_ip.wrapping_add(*disp as i32 as u32);
                if !eip_32bit { cpu.eip &= 0xFFFF; }
            } else {
                cpu.eip = next_ip;
            }
        }
        Opcode::JccNear(cc, disp) => {
            if check_condition(cpu, *cc) {
                cpu.eip = next_ip.wrapping_add(*disp as u32);
                if !eip_32bit { cpu.eip &= 0xFFFF; }
            } else {
                cpu.eip = next_ip;
            }
        }
        Opcode::CallNearRel(disp) => {
            pushv(cpu, mem, next_ip, is32);
            cpu.eip = next_ip.wrapping_add(*disp as u32);
            if !eip_32bit { cpu.eip &= 0xFFFF; }
        }
        Opcode::CallFar(seg, offset) => {
            pushv(cpu, mem, cpu.cs as u32, is32);
            pushv(cpu, mem, next_ip, is32);
            crate::descriptor::load_segment(cpu, mem, 1, *seg);
            cpu.eip = *offset;
        }
        Opcode::Ret => {
            let ip = popv(cpu, mem, is32);
            cpu.eip = ip;
        }
        Opcode::RetImm16(imm) => {
            let ip = popv(cpu, mem, is32);
            cpu.eip = ip;
            let sp = cpu.esp.wrapping_add(*imm as u32);
            cpu.esp = if is32 { sp } else { (cpu.esp & 0xFFFF0000) | (sp & 0xFFFF) };
        }
        Opcode::Retf => {
            let ip = popv(cpu, mem, is32);
            let cs = popv(cpu, mem, is32) as u16;
            crate::descriptor::load_segment(cpu, mem, 1, cs);
            cpu.eip = ip;
        }
        Opcode::RetfImm16(imm) => {
            let ip = popv(cpu, mem, is32);
            let cs = popv(cpu, mem, is32) as u16;
            crate::descriptor::load_segment(cpu, mem, 1, cs);
            cpu.eip = ip;
            let sp = cpu.esp.wrapping_add(*imm as u32);
            cpu.esp = if is32 { sp } else { (cpu.esp & 0xFFFF0000) | (sp & 0xFFFF) };
        }

        // === LOOP ===
        Opcode::Loop(disp) => {
            let cx = cpu.get_reg16(1).wrapping_sub(1);
            cpu.set_reg16(1, cx);
            if cx != 0 {
                cpu.eip = next_ip.wrapping_add(*disp as i32 as u32);
                if !eip_32bit { cpu.eip &= 0xFFFF; }
            } else { cpu.eip = next_ip; }
        }
        Opcode::Loope(disp) => {
            let cx = cpu.get_reg16(1).wrapping_sub(1);
            cpu.set_reg16(1, cx);
            if cx != 0 && get_flag(cpu, FLAG_ZF) {
                cpu.eip = next_ip.wrapping_add(*disp as i32 as u32);
                if !eip_32bit { cpu.eip &= 0xFFFF; }
            } else { cpu.eip = next_ip; }
        }
        Opcode::Loopne(disp) => {
            let cx = cpu.get_reg16(1).wrapping_sub(1);
            cpu.set_reg16(1, cx);
            if cx != 0 && !get_flag(cpu, FLAG_ZF) {
                cpu.eip = next_ip.wrapping_add(*disp as i32 as u32);
                if !eip_32bit { cpu.eip &= 0xFFFF; }
            } else { cpu.eip = next_ip; }
        }

        // === Interrupts ===
        Opcode::Int(vec) => {
            cpu.eip = next_ip;
            if !int_handler.handle_int(cpu, mem, *vec) {
                log::warn!("Unhandled INT 0x{:02X}", vec);
            }
        }
        Opcode::Iret => {
            if is32 {
                let ip = pop32(cpu, mem);
                let cs = pop32(cpu, mem);
                let flags = pop32(cpu, mem);
                cpu.eip = ip;
                cpu.cs = cs as u16;
                cpu.eflags = (flags & 0x00FCFFFF) | 0x0002;
            } else {
                let ip = pop16(cpu, mem);
                let cs = pop16(cpu, mem);
                let flags = pop16(cpu, mem);
                cpu.eip = ip as u32;
                cpu.cs = cs;
                cpu.eflags = (flags as u32) | 0x0002;
            }
        }

        // === I/O ===
        Opcode::InAlImm8(port) => {
            cpu.set_reg8(0, ports.port_in(*port as u16, 1) as u8);
            cpu.eip = next_ip;
        }
        Opcode::InAxImm8(port) => {
            let v = ports.port_in(*port as u16, if is32 { 4 } else { 2 });
            if is32 { cpu.set_reg32(0, v); } else { cpu.set_reg16(0, v as u16); }
            cpu.eip = next_ip;
        }
        Opcode::InAlDx => {
            cpu.set_reg8(0, ports.port_in(cpu.get_reg16(2), 1) as u8);
            cpu.eip = next_ip;
        }
        Opcode::InAxDx => {
            let v = ports.port_in(cpu.get_reg16(2), if is32 { 4 } else { 2 });
            if is32 { cpu.set_reg32(0, v); } else { cpu.set_reg16(0, v as u16); }
            cpu.eip = next_ip;
        }
        Opcode::OutImm8Al(port) => {
            ports.port_out(*port as u16, 1, cpu.get_reg8(0) as u32);
            cpu.eip = next_ip;
        }
        Opcode::OutImm8Ax(port) => {
            let v = if is32 { cpu.get_reg32(0) } else { cpu.get_reg16(0) as u32 };
            ports.port_out(*port as u16, if is32 { 4 } else { 2 }, v);
            cpu.eip = next_ip;
        }
        Opcode::OutDxAl => {
            ports.port_out(cpu.get_reg16(2), 1, cpu.get_reg8(0) as u32);
            cpu.eip = next_ip;
        }
        Opcode::OutDxAx => {
            let v = if is32 { cpu.get_reg32(0) } else { cpu.get_reg16(0) as u32 };
            ports.port_out(cpu.get_reg16(2), if is32 { 4 } else { 2 }, v);
            cpu.eip = next_ip;
        }

        // === LEA ===
        Opcode::Leav(reg, rm, _) => {
            if let ModrmOperand::Mem(addr) = rm {
                if is32 { cpu.set_reg32(*reg, *addr); } else { cpu.set_reg16(*reg, *addr as u16); }
            }
            cpu.eip = next_ip;
        }

        // === XCHG ===
        Opcode::XchgAxReg(reg) => {
            if is32 {
                let a = cpu.get_reg32(0); let b = cpu.get_reg32(*reg);
                cpu.set_reg32(0, b); cpu.set_reg32(*reg, a);
            } else {
                let a = cpu.get_reg16(0); let b = cpu.get_reg16(*reg);
                cpu.set_reg16(0, b); cpu.set_reg16(*reg, a);
            }
            cpu.eip = next_ip;
        }
        Opcode::XchgRmReg8(reg, rm, _) => {
            let a = read_rm8(cpu, mem, rm);
            let b = cpu.get_reg8(*reg);
            write_rm8(cpu, mem, rm, b);
            cpu.set_reg8(*reg, a);
            cpu.eip = next_ip;
        }
        Opcode::XchgRmRegv(reg, rm, _) => {
            let a = read_rmv(cpu, mem, rm, is32);
            let b = if is32 { cpu.get_reg32(*reg) } else { cpu.get_reg16(*reg) as u32 };
            write_rmv(cpu, mem, rm, b, is32);
            if is32 { cpu.set_reg32(*reg, a); } else { cpu.set_reg16(*reg, a as u16); }
            cpu.eip = next_ip;
        }

        // === CBW/CWD ===
        Opcode::Cbw => {
            if is32 {
                // CWDE: sign-extend AX to EAX
                cpu.eax = cpu.get_reg16(0) as i16 as i32 as u32;
            } else {
                // CBW: sign-extend AL to AX
                let al = cpu.get_reg8(0) as i8 as i16 as u16;
                cpu.set_reg16(0, al);
            }
            cpu.eip = next_ip;
        }
        Opcode::Cwd => {
            if is32 {
                // CDQ: sign-extend EAX to EDX:EAX
                if (cpu.eax as i32) < 0 { cpu.edx = 0xFFFFFFFF; } else { cpu.edx = 0; }
            } else {
                let ax = cpu.get_reg16(0) as i16;
                if ax < 0 { cpu.set_reg16(2, 0xFFFF); } else { cpu.set_reg16(2, 0); }
            }
            cpu.eip = next_ip;
        }

        // === SAHF/LAHF ===
        Opcode::Sahf => {
            let ah = cpu.get_reg8(4);
            cpu.eflags = (cpu.eflags & 0xFFFFFF00) | (ah as u32) | 0x02;
            cpu.eip = next_ip;
        }
        Opcode::Lahf => {
            cpu.set_reg8(4, cpu.eflags as u8);
            cpu.eip = next_ip;
        }

        // === String operations ===
        Opcode::Movsb => { exec_movsb(cpu, mem); cpu.eip = next_ip; }
        Opcode::Movsv => {
            if is32 { exec_movsd(cpu, mem); } else { exec_movsw(cpu, mem); }
            cpu.eip = next_ip;
        }
        Opcode::Stosb => { exec_stosb(cpu, mem); cpu.eip = next_ip; }
        Opcode::Stosv => {
            if is32 { exec_stosd(cpu, mem); } else { exec_stosw(cpu, mem); }
            cpu.eip = next_ip;
        }
        Opcode::Lodsb => { exec_lodsb(cpu, mem); cpu.eip = next_ip; }
        Opcode::Lodsv => {
            if is32 { exec_lodsd(cpu, mem); } else { exec_lodsw(cpu, mem); }
            cpu.eip = next_ip;
        }
        Opcode::Cmpsb => { exec_cmpsb(cpu, mem); cpu.eip = next_ip; }
        Opcode::Cmpsv => { exec_cmpsw(cpu, mem); cpu.eip = next_ip; }
        Opcode::Scasb => { exec_scasb(cpu, mem); cpu.eip = next_ip; }
        Opcode::Scasv => { exec_scasw(cpu, mem); cpu.eip = next_ip; }

        // === REP ===
        Opcode::Rep(inner) => {
            while cpu.get_reg16(1) != 0 {
                let tmp = Instruction { op: (**inner).clone(), len: 0, operand_size: inst.operand_size, addr_size: inst.addr_size, segment_override: inst.segment_override };
                execute(cpu, mem, ports, int_handler, &tmp);
                let cx = cpu.get_reg16(1).wrapping_sub(1);
                cpu.set_reg16(1, cx);
                // For CMPS/SCAS with REPE, stop if ZF=0
                match **inner {
                    Opcode::Cmpsb | Opcode::Cmpsv | Opcode::Scasb | Opcode::Scasv => {
                        if !get_flag(cpu, FLAG_ZF) { break; }
                    }
                    _ => {}
                }
            }
            cpu.eip = next_ip;
        }
        Opcode::Repne(inner) => {
            while cpu.get_reg16(1) != 0 {
                let tmp = Instruction { op: (**inner).clone(), len: 0, operand_size: inst.operand_size, addr_size: inst.addr_size, segment_override: inst.segment_override };
                execute(cpu, mem, ports, int_handler, &tmp);
                let cx = cpu.get_reg16(1).wrapping_sub(1);
                cpu.set_reg16(1, cx);
                match **inner {
                    Opcode::Cmpsb | Opcode::Cmpsv | Opcode::Scasb | Opcode::Scasv => {
                        if get_flag(cpu, FLAG_ZF) { break; }
                    }
                    _ => {}
                }
            }
            cpu.eip = next_ip;
        }

        // === Shift/Rotate ===
        Opcode::ShiftRm8(op, rm, _, count) => {
            let val = read_rm8(cpu, mem, rm) as u32;
            let cnt = resolve_shift_count(cpu, count) & 0x1F;
            if cnt > 0 {
                let result = exec_shift(cpu, *op, val, cnt, 8);
                write_rm8(cpu, mem, rm, result as u8);
            }
            cpu.eip = next_ip;
        }
        Opcode::ShiftRmv(op, rm, _, count) => {
            let val = read_rmv(cpu, mem, rm, is32);
            let cnt = resolve_shift_count(cpu, count) & 0x1F;
            if cnt > 0 {
                let result = exec_shift(cpu, *op, val, cnt, width);
                write_rmv(cpu, mem, rm, result, is32);
            }
            cpu.eip = next_ip;
        }

        // === Group F6 (byte) ===
        Opcode::GroupF6(sub, rm, _, test_imm) => {
            match sub {
                0 | 1 => { // TEST r/m8, imm8
                    let val = read_rm8(cpu, mem, rm);
                    update_flags_logic(cpu, (val & test_imm.unwrap()) as u32, 8);
                }
                2 => { // NOT r/m8
                    let val = read_rm8(cpu, mem, rm);
                    write_rm8(cpu, mem, rm, !val);
                }
                3 => { // NEG r/m8
                    let val = read_rm8(cpu, mem, rm) as u32;
                    let result = 0u32.wrapping_sub(val);
                    update_flags_sub(cpu, 0, val, result as u64, 8);
                    write_rm8(cpu, mem, rm, result as u8);
                }
                4 => { // MUL r/m8
                    let a = cpu.get_reg8(0) as u16;
                    let b = read_rm8(cpu, mem, rm) as u16;
                    let result = a * b;
                    cpu.set_reg16(0, result); // AX = result
                    let high = (result >> 8) != 0;
                    set_flag(cpu, FLAG_CF, high);
                    set_flag(cpu, FLAG_OF, high);
                }
                5 => { // IMUL r/m8
                    let a = cpu.get_reg8(0) as i8 as i16;
                    let b = read_rm8(cpu, mem, rm) as i8 as i16;
                    let result = a * b;
                    cpu.set_reg16(0, result as u16);
                    let high = result != (result as i8 as i16);
                    set_flag(cpu, FLAG_CF, high);
                    set_flag(cpu, FLAG_OF, high);
                }
                6 => { // DIV r/m8
                    let dividend = cpu.get_reg16(0) as u16;
                    let divisor = read_rm8(cpu, mem, rm) as u16;
                    if divisor == 0 { cpu.eip = next_ip; return ExecResult::DivideError; }
                    let quotient = dividend / divisor;
                    if quotient > 0xFF { cpu.eip = next_ip; return ExecResult::DivideError; }
                    cpu.set_reg8(0, quotient as u8);      // AL
                    cpu.set_reg8(4, (dividend % divisor) as u8); // AH
                }
                7 => { // IDIV r/m8
                    let dividend = cpu.get_reg16(0) as i16;
                    let divisor = read_rm8(cpu, mem, rm) as i8 as i16;
                    if divisor == 0 { cpu.eip = next_ip; return ExecResult::DivideError; }
                    let quotient = dividend / divisor;
                    if quotient > 127 || quotient < -128 { cpu.eip = next_ip; return ExecResult::DivideError; }
                    cpu.set_reg8(0, quotient as u8);
                    cpu.set_reg8(4, (dividend % divisor) as u8);
                }
                _ => {}
            }
            cpu.eip = next_ip;
        }

        // === Group F7 (word/dword) ===
        Opcode::GroupF7(sub, rm, _, test_imm) => {
            match sub {
                0 | 1 => { // TEST r/mv, immv
                    let val = read_rmv(cpu, mem, rm, is32);
                    update_flags_logic(cpu, val & test_imm.unwrap(), width);
                }
                2 => { // NOT
                    let val = read_rmv(cpu, mem, rm, is32);
                    write_rmv(cpu, mem, rm, !val, is32);
                }
                3 => { // NEG
                    let val = read_rmv(cpu, mem, rm, is32);
                    let result = 0u64.wrapping_sub(val as u64);
                    update_flags_sub(cpu, 0, val, result, width);
                    write_rmv(cpu, mem, rm, result as u32, is32);
                }
                4 => { // MUL
                    if is32 {
                        let a = cpu.get_reg32(0) as u64;
                        let b = read_rmv(cpu, mem, rm, true) as u64;
                        let result = a * b;
                        cpu.eax = result as u32;
                        cpu.edx = (result >> 32) as u32;
                        let high = cpu.edx != 0;
                        set_flag(cpu, FLAG_CF, high);
                        set_flag(cpu, FLAG_OF, high);
                    } else {
                        let a = cpu.get_reg16(0) as u32;
                        let b = read_rmv(cpu, mem, rm, false) as u32;
                        let result = a * b;
                        cpu.set_reg16(0, result as u16);
                        cpu.set_reg16(2, (result >> 16) as u16);
                        let high = (result >> 16) != 0;
                        set_flag(cpu, FLAG_CF, high);
                        set_flag(cpu, FLAG_OF, high);
                    }
                }
                5 => { // IMUL
                    if is32 {
                        let a = cpu.get_reg32(0) as i32 as i64;
                        let b = read_rmv(cpu, mem, rm, true) as i32 as i64;
                        let result = a * b;
                        cpu.eax = result as u32;
                        cpu.edx = (result >> 32) as u32;
                        let high = result != (cpu.eax as i32 as i64);
                        set_flag(cpu, FLAG_CF, high);
                        set_flag(cpu, FLAG_OF, high);
                    } else {
                        let a = cpu.get_reg16(0) as i16 as i32;
                        let b = read_rmv(cpu, mem, rm, false) as i16 as i32;
                        let result = a * b;
                        cpu.set_reg16(0, result as u16);
                        cpu.set_reg16(2, (result >> 16) as u16);
                        let high = result != (result as i16 as i32);
                        set_flag(cpu, FLAG_CF, high);
                        set_flag(cpu, FLAG_OF, high);
                    }
                }
                6 => { // DIV
                    if is32 {
                        let dividend = ((cpu.edx as u64) << 32) | (cpu.eax as u64);
                        let divisor = read_rmv(cpu, mem, rm, true) as u64;
                        if divisor == 0 { cpu.eip = next_ip; return ExecResult::DivideError; }
                        let quotient = dividend / divisor;
                        if quotient > 0xFFFFFFFF { cpu.eip = next_ip; return ExecResult::DivideError; }
                        cpu.eax = quotient as u32;
                        cpu.edx = (dividend % divisor) as u32;
                    } else {
                        let dividend = ((cpu.get_reg16(2) as u32) << 16) | (cpu.get_reg16(0) as u32);
                        let divisor = read_rmv(cpu, mem, rm, false);
                        if divisor == 0 { cpu.eip = next_ip; return ExecResult::DivideError; }
                        let quotient = dividend / divisor;
                        if quotient > 0xFFFF { cpu.eip = next_ip; return ExecResult::DivideError; }
                        cpu.set_reg16(0, quotient as u16);
                        cpu.set_reg16(2, (dividend % divisor) as u16);
                    }
                }
                7 => { // IDIV
                    if is32 {
                        let dividend = (((cpu.edx as u64) << 32) | (cpu.eax as u64)) as i64;
                        let divisor = read_rmv(cpu, mem, rm, true) as i32 as i64;
                        if divisor == 0 { cpu.eip = next_ip; return ExecResult::DivideError; }
                        let quotient = dividend / divisor;
                        if quotient > i32::MAX as i64 || quotient < i32::MIN as i64 {
                            cpu.eip = next_ip; return ExecResult::DivideError;
                        }
                        cpu.eax = quotient as u32;
                        cpu.edx = (dividend % divisor) as u32;
                    } else {
                        let dividend = ((cpu.get_reg16(2) as u32) << 16 | cpu.get_reg16(0) as u32) as i32;
                        let divisor = read_rmv(cpu, mem, rm, false) as i16 as i32;
                        if divisor == 0 { cpu.eip = next_ip; return ExecResult::DivideError; }
                        let quotient = dividend / divisor;
                        if quotient > i16::MAX as i32 || quotient < i16::MIN as i32 {
                            cpu.eip = next_ip; return ExecResult::DivideError;
                        }
                        cpu.set_reg16(0, quotient as u16);
                        cpu.set_reg16(2, (dividend % divisor) as u16);
                    }
                }
                _ => {}
            }
            cpu.eip = next_ip;
        }

        // === Group FE (INC/DEC r/m8) ===
        Opcode::GroupFE(sub, rm, _) => {
            match sub {
                0 => {
                    let val = read_rm8(cpu, mem, rm) as u32;
                    let result = val.wrapping_add(1);
                    let cf = get_flag(cpu, FLAG_CF);
                    update_flags_add(cpu, val, 1, result as u64, 8);
                    set_flag(cpu, FLAG_CF, cf);
                    write_rm8(cpu, mem, rm, result as u8);
                }
                1 => {
                    let val = read_rm8(cpu, mem, rm) as u32;
                    let result = val.wrapping_sub(1);
                    let cf = get_flag(cpu, FLAG_CF);
                    update_flags_sub(cpu, val, 1, result as u64, 8);
                    set_flag(cpu, FLAG_CF, cf);
                    write_rm8(cpu, mem, rm, result as u8);
                }
                _ => log::warn!("Unimplemented FE sub-op: {}", sub),
            }
            cpu.eip = next_ip;
        }

        // === Group FF ===
        Opcode::GroupFF(sub, rm, _) => {
            match sub {
                0 => { // INC r/mv
                    let val = read_rmv(cpu, mem, rm, is32);
                    let result = val.wrapping_add(1);
                    let cf = get_flag(cpu, FLAG_CF);
                    update_flags_add(cpu, val, 1, result as u64, width);
                    set_flag(cpu, FLAG_CF, cf);
                    write_rmv(cpu, mem, rm, result, is32);
                }
                1 => { // DEC r/mv
                    let val = read_rmv(cpu, mem, rm, is32);
                    let result = val.wrapping_sub(1);
                    let cf = get_flag(cpu, FLAG_CF);
                    update_flags_sub(cpu, val, 1, result as u64, width);
                    set_flag(cpu, FLAG_CF, cf);
                    write_rmv(cpu, mem, rm, result, is32);
                }
                2 => { // CALL r/mv (near indirect)
                    let target = read_rmv(cpu, mem, rm, is32);
                    pushv(cpu, mem, next_ip, is32);
                    cpu.eip = target;
                    return ExecResult::Continue;
                }
                4 => { // JMP r/mv (near indirect)
                    let target = read_rmv(cpu, mem, rm, is32);
                    cpu.eip = target;
                    return ExecResult::Continue;
                }
                6 => { // PUSH r/mv
                    let val = read_rmv(cpu, mem, rm, is32);
                    pushv(cpu, mem, val, is32);
                }
                _ => log::warn!("Unimplemented FF sub-op: {}", sub),
            }
            cpu.eip = next_ip;
        }

        // === 0x0F two-byte opcodes ===
        Opcode::Group0F01(sub, rm, _) => {
            match sub {
                2 => { // LGDT
                    if let ModrmOperand::Mem(addr) = rm {
                        let limit = mem.read_u16(*addr);
                        let base = if is32 {
                            mem.read_u32(*addr + 2)
                        } else {
                            mem.read_u32(*addr + 2) & 0x00FFFFFF
                        };
                        cpu.gdtr = crate::regs::DescriptorTableReg { base, limit };
                    }
                }
                3 => { // LIDT
                    if let ModrmOperand::Mem(addr) = rm {
                        let limit = mem.read_u16(*addr);
                        let base = if is32 {
                            mem.read_u32(*addr + 2)
                        } else {
                            mem.read_u32(*addr + 2) & 0x00FFFFFF
                        };
                        cpu.idtr = crate::regs::DescriptorTableReg { base, limit };
                    }
                }
                0 => { // SGDT
                    if let ModrmOperand::Mem(addr) = rm {
                        mem.write_u16(*addr, cpu.gdtr.limit);
                        mem.write_u32(*addr + 2, cpu.gdtr.base);
                    }
                }
                1 => { // SIDT
                    if let ModrmOperand::Mem(addr) = rm {
                        mem.write_u16(*addr, cpu.idtr.limit);
                        mem.write_u32(*addr + 2, cpu.idtr.base);
                    }
                }
                4 => { // SMSW: store machine status word (CR0 low 16 bits)
                    let val = (cpu.cr0 & 0xFFFF) as u16;
                    match rm {
                        ModrmOperand::Reg(idx) => {
                            if is32 { cpu.set_reg32(*idx, val as u32); } else { cpu.set_reg16(*idx, val); }
                        }
                        ModrmOperand::Mem(addr) => {
                            mem.write_u16(*addr, val);
                        }
                    }
                }
                6 => { // LMSW: load machine status word
                    let val = match rm {
                        ModrmOperand::Reg(idx) => cpu.get_reg16(*idx),
                        ModrmOperand::Mem(addr) => mem.read_u16(*addr),
                    };
                    let old_pe = cpu.cr0 & 1;
                    // LMSW cannot clear PE once set
                    cpu.cr0 = (cpu.cr0 & 0xFFFF0000) | (val as u32);
                    if old_pe == 1 {
                        cpu.cr0 |= 1; // PE cannot be cleared by LMSW
                    }
                    let new_pe = cpu.cr0 & 1;
                    if old_pe == 0 && new_pe == 1 {
                        cpu.mode = crate::regs::CpuMode::ProtectedMode;
                        cpu.cs_cache.base = (cpu.cs as u32) << 4;
                        cpu.cs_cache.selector = cpu.cs;
                        cpu.ds_cache.base = (cpu.ds as u32) << 4;
                        cpu.es_cache.base = (cpu.es as u32) << 4;
                        cpu.ss_cache.base = (cpu.ss as u32) << 4;
                    }
                }
                7 => { // INVLPG: invalidate TLB entry (no-op for us)
                }
                _ => log::warn!("Unimplemented 0F01 sub-op: {}", sub),
            }
            cpu.eip = next_ip;
        }

        Opcode::MovFromCr(cr, reg) => {
            let val = match cr {
                0 => cpu.cr0, 2 => cpu.cr2, 3 => cpu.cr3, 4 => cpu.cr4,
                _ => 0,
            };
            cpu.set_reg32(*reg, val);
            cpu.eip = next_ip;
        }
        Opcode::MovToCr(cr, reg) => {
            let val = cpu.get_reg32(*reg);
            match cr {
                0 => {
                    let old_pe = cpu.cr0 & 1;
                    cpu.cr0 = val;
                    let new_pe = val & 1;
                    if old_pe == 0 && new_pe == 1 {
                        cpu.mode = crate::regs::CpuMode::ProtectedMode;
                        // Preserve real-mode segment bases in caches so
                        // code continues executing from the same physical
                        // address until a far JMP reloads CS from the GDT.
                        cpu.cs_cache.base = (cpu.cs as u32) << 4;
                        cpu.cs_cache.selector = cpu.cs;
                        cpu.ds_cache.base = (cpu.ds as u32) << 4;
                        cpu.es_cache.base = (cpu.es as u32) << 4;
                        cpu.ss_cache.base = (cpu.ss as u32) << 4;
                    } else if old_pe == 1 && new_pe == 0 {
                        cpu.mode = crate::regs::CpuMode::RealMode;
                    }
                }
                2 => cpu.cr2 = val,
                3 => cpu.cr3 = val,
                4 => cpu.cr4 = val,
                _ => {}
            }
            cpu.eip = next_ip;
        }

        Opcode::MovzxByte(reg, rm, _) => {
            let val = read_rm8(cpu, mem, rm) as u32;
            if is32 { cpu.set_reg32(*reg, val); } else { cpu.set_reg16(*reg, val as u16); }
            cpu.eip = next_ip;
        }
        Opcode::MovzxWord(reg, rm, _) => {
            let val = read_rm16(cpu, mem, rm) as u32;
            cpu.set_reg32(*reg, val);
            cpu.eip = next_ip;
        }
        Opcode::MovsxByte(reg, rm, _) => {
            let val = read_rm8(cpu, mem, rm) as i8;
            if is32 { cpu.set_reg32(*reg, val as i32 as u32); } else { cpu.set_reg16(*reg, val as i16 as u16); }
            cpu.eip = next_ip;
        }
        Opcode::MovsxWord(reg, rm, _) => {
            let val = read_rm16(cpu, mem, rm) as i16;
            cpu.set_reg32(*reg, val as i32 as u32);
            cpu.eip = next_ip;
        }

        Opcode::Setcc(cc, rm, _) => {
            let val = if check_condition(cpu, *cc) { 1u8 } else { 0 };
            write_rm8(cpu, mem, rm, val);
            cpu.eip = next_ip;
        }

        Opcode::ImulRegRmv(reg, rm, _) => {
            if is32 {
                let a = cpu.get_reg32(*reg) as i32 as i64;
                let b = read_rmv(cpu, mem, rm, true) as i32 as i64;
                let result = a * b;
                cpu.set_reg32(*reg, result as u32);
                let overflow = result != (result as i32 as i64);
                set_flag(cpu, FLAG_CF, overflow);
                set_flag(cpu, FLAG_OF, overflow);
            } else {
                let a = cpu.get_reg16(*reg) as i16 as i32;
                let b = read_rmv(cpu, mem, rm, false) as i16 as i32;
                let result = a * b;
                cpu.set_reg16(*reg, result as u16);
                let overflow = result != (result as i16 as i32);
                set_flag(cpu, FLAG_CF, overflow);
                set_flag(cpu, FLAG_OF, overflow);
            }
            cpu.eip = next_ip;
        }
        Opcode::ImulRegRmvImm8(reg, rm, _, imm) => {
            if is32 {
                let b = read_rmv(cpu, mem, rm, true) as i32 as i64;
                let result = b * (*imm as i64);
                cpu.set_reg32(*reg, result as u32);
                set_flag(cpu, FLAG_CF, result != (result as i32 as i64));
                set_flag(cpu, FLAG_OF, result != (result as i32 as i64));
            } else {
                let b = read_rmv(cpu, mem, rm, false) as i16 as i32;
                let result = b * (*imm as i32);
                cpu.set_reg16(*reg, result as u16);
                set_flag(cpu, FLAG_CF, result != (result as i16 as i32));
                set_flag(cpu, FLAG_OF, result != (result as i16 as i32));
            }
            cpu.eip = next_ip;
        }
        Opcode::ImulRegRmvImmv(reg, rm, _, imm) => {
            if is32 {
                let b = read_rmv(cpu, mem, rm, true) as i32 as i64;
                let result = b * (*imm as i32 as i64);
                cpu.set_reg32(*reg, result as u32);
                set_flag(cpu, FLAG_CF, result != (result as i32 as i64));
                set_flag(cpu, FLAG_OF, result != (result as i32 as i64));
            } else {
                let b = read_rmv(cpu, mem, rm, false) as i16 as i32;
                let result = b * (*imm as i16 as i32);
                cpu.set_reg16(*reg, result as u16);
                set_flag(cpu, FLAG_CF, result != (result as i16 as i32));
                set_flag(cpu, FLAG_OF, result != (result as i16 as i32));
            }
            cpu.eip = next_ip;
        }

        // === LEAVE ===
        Opcode::Leave => {
            if is32 {
                cpu.esp = cpu.ebp;
                cpu.ebp = pop32(cpu, mem);
            } else {
                cpu.set_reg16(4, cpu.get_reg16(5)); // SP = BP
                let val = pop16(cpu, mem);
                cpu.set_reg16(5, val); // BP = pop
            }
            cpu.eip = next_ip;
        }

        // === ENTER ===
        Opcode::Enter(size, nesting) => {
            // Simplified: only handle nesting level 0
            pushv(cpu, mem, cpu.ebp, is32);
            let frame = cpu.esp;
            if *nesting == 0 {
                // Simple case
            }
            if is32 {
                cpu.ebp = frame;
                cpu.esp = frame.wrapping_sub(*size as u32);
            } else {
                cpu.set_reg16(5, frame as u16);
                cpu.set_reg16(4, (frame as u16).wrapping_sub(*size));
            }
            cpu.eip = next_ip;
        }

        // === Misc ===
        Opcode::Nop => { cpu.eip = next_ip; }
        Opcode::Hlt => {
            cpu.halted = true;
            cpu.eip = next_ip;
            return ExecResult::Halt;
        }
        Opcode::Cli => { set_flag(cpu, FLAG_IF, false); cpu.eip = next_ip; }
        Opcode::Sti => { set_flag(cpu, FLAG_IF, true); cpu.eip = next_ip; }
        Opcode::Cld => { set_flag(cpu, FLAG_DF, false); cpu.eip = next_ip; }
        Opcode::Std => { set_flag(cpu, FLAG_DF, true); cpu.eip = next_ip; }
        Opcode::Clc => { set_flag(cpu, FLAG_CF, false); cpu.eip = next_ip; }
        Opcode::Stc => { set_flag(cpu, FLAG_CF, true); cpu.eip = next_ip; }
        Opcode::Cmc => { let cf = get_flag(cpu, FLAG_CF); set_flag(cpu, FLAG_CF, !cf); cpu.eip = next_ip; }

        // === CPUID ===
        Opcode::Cpuid => {
            match cpu.eax {
                0 => {
                    cpu.eax = 1; // max leaf
                    // "kokoa86!x86E" = EBX:EDX:ECX
                    cpu.ebx = u32::from_le_bytes(*b"koko");
                    cpu.edx = u32::from_le_bytes(*b"a86!");
                    cpu.ecx = u32::from_le_bytes(*b"x86E");
                }
                1 => {
                    cpu.eax = 0x00000600; // family 6
                    cpu.ebx = 0;
                    cpu.ecx = 0;
                    cpu.edx = 0x00008011; // FPU + PSE + TSC + CMOV
                }
                _ => {
                    cpu.eax = 0;
                    cpu.ebx = 0;
                    cpu.ecx = 0;
                    cpu.edx = 0;
                }
            }
            cpu.eip = next_ip;
        }

        // === WBINVD / INVD ===
        Opcode::Wbinvd | Opcode::Invd => {
            cpu.eip = next_ip;
        }

        // === RDTSC (stub) ===
        Opcode::Rdtsc => {
            cpu.eax = 0;
            cpu.edx = 0;
            cpu.eip = next_ip;
        }

        // === RDMSR / WRMSR (no-op stubs) ===
        Opcode::Rdmsr | Opcode::Wrmsr => {
            cpu.eip = next_ip;
        }

        // === INSB ===
        Opcode::Insb => {
            let port = cpu.get_reg16(2);
            let val = ports.port_in(port, 1);
            let addr = cpu.linear_addr(cpu.es, cpu.get_reg16(7));
            mem.write_u8(addr, val as u8);
            let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFF } else { 1 };
            cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(d));
            cpu.eip = next_ip;
        }
        // === INSV ===
        Opcode::Insv => {
            let port = cpu.get_reg16(2);
            let addr = cpu.linear_addr(cpu.es, cpu.get_reg16(7));
            if is32 {
                let val = ports.port_in(port, 4);
                mem.write_u32(addr, val);
                let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFC } else { 4 };
                cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(d));
            } else {
                let val = ports.port_in(port, 2);
                mem.write_u16(addr, val as u16);
                let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFE } else { 2 };
                cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(d));
            }
            cpu.eip = next_ip;
        }
        // === OUTSB ===
        Opcode::Outsb => {
            let port = cpu.get_reg16(2);
            let addr = cpu.linear_addr(cpu.ds, cpu.get_reg16(6));
            let val = mem.read_u8(addr) as u32;
            ports.port_out(port, 1, val);
            let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFF } else { 1 };
            cpu.set_reg16(6, cpu.get_reg16(6).wrapping_add(d));
            cpu.eip = next_ip;
        }
        // === OUTSV ===
        Opcode::Outsv => {
            let port = cpu.get_reg16(2);
            let addr = cpu.linear_addr(cpu.ds, cpu.get_reg16(6));
            if is32 {
                let val = mem.read_u32(addr);
                ports.port_out(port, 4, val);
                let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFC } else { 4 };
                cpu.set_reg16(6, cpu.get_reg16(6).wrapping_add(d));
            } else {
                let val = mem.read_u16(addr) as u32;
                ports.port_out(port, 2, val);
                let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFE } else { 2 };
                cpu.set_reg16(6, cpu.get_reg16(6).wrapping_add(d));
            }
            cpu.eip = next_ip;
        }

        // === BSF ===
        Opcode::Bsf(reg, rm, _) => {
            let src = read_rmv(cpu, mem, rm, is32);
            if src == 0 {
                set_flag(cpu, FLAG_ZF, true);
            } else {
                set_flag(cpu, FLAG_ZF, false);
                let bit = src.trailing_zeros();
                if is32 { cpu.set_reg32(*reg, bit); } else { cpu.set_reg16(*reg, bit as u16); }
            }
            cpu.eip = next_ip;
        }
        // === BSR ===
        Opcode::Bsr(reg, rm, _) => {
            let src = read_rmv(cpu, mem, rm, is32);
            if src == 0 {
                set_flag(cpu, FLAG_ZF, true);
            } else {
                set_flag(cpu, FLAG_ZF, false);
                let bit = 31 - src.leading_zeros();
                if is32 { cpu.set_reg32(*reg, bit); } else { cpu.set_reg16(*reg, bit as u16); }
            }
            cpu.eip = next_ip;
        }

        // === BT r/m, reg ===
        Opcode::BtRmReg(reg, rm, _) => {
            let src = read_rmv(cpu, mem, rm, is32);
            let bit = if is32 { cpu.get_reg32(*reg) } else { cpu.get_reg16(*reg) as u32 };
            let bit_pos = bit & (width as u32 - 1);
            set_flag(cpu, FLAG_CF, (src >> bit_pos) & 1 != 0);
            cpu.eip = next_ip;
        }
        // === BTS r/m, reg ===
        Opcode::BtsRmReg(reg, rm, _) => {
            let src = read_rmv(cpu, mem, rm, is32);
            let bit = if is32 { cpu.get_reg32(*reg) } else { cpu.get_reg16(*reg) as u32 };
            let bit_pos = bit & (width as u32 - 1);
            set_flag(cpu, FLAG_CF, (src >> bit_pos) & 1 != 0);
            write_rmv(cpu, mem, rm, src | (1 << bit_pos), is32);
            cpu.eip = next_ip;
        }
        // === BTR r/m, reg ===
        Opcode::BtrRmReg(reg, rm, _) => {
            let src = read_rmv(cpu, mem, rm, is32);
            let bit = if is32 { cpu.get_reg32(*reg) } else { cpu.get_reg16(*reg) as u32 };
            let bit_pos = bit & (width as u32 - 1);
            set_flag(cpu, FLAG_CF, (src >> bit_pos) & 1 != 0);
            write_rmv(cpu, mem, rm, src & !(1 << bit_pos), is32);
            cpu.eip = next_ip;
        }
        // === BTC r/m, reg ===
        Opcode::BtcRmReg(reg, rm, _) => {
            let src = read_rmv(cpu, mem, rm, is32);
            let bit = if is32 { cpu.get_reg32(*reg) } else { cpu.get_reg16(*reg) as u32 };
            let bit_pos = bit & (width as u32 - 1);
            set_flag(cpu, FLAG_CF, (src >> bit_pos) & 1 != 0);
            write_rmv(cpu, mem, rm, src ^ (1 << bit_pos), is32);
            cpu.eip = next_ip;
        }
        // === BT group (0F BA) with imm8 ===
        Opcode::BtGroup(sub, rm, _, imm) => {
            let src = read_rmv(cpu, mem, rm, is32);
            let bit_pos = (*imm as u32) & (width as u32 - 1);
            set_flag(cpu, FLAG_CF, (src >> bit_pos) & 1 != 0);
            match sub {
                4 => {} // BT - just test
                5 => { write_rmv(cpu, mem, rm, src | (1 << bit_pos), is32); }  // BTS
                6 => { write_rmv(cpu, mem, rm, src & !(1 << bit_pos), is32); } // BTR
                7 => { write_rmv(cpu, mem, rm, src ^ (1 << bit_pos), is32); }  // BTC
                _ => log::warn!("Unimplemented BT group sub-op: {}", sub),
            }
            cpu.eip = next_ip;
        }

        // === BSWAP ===
        Opcode::Bswap(reg) => {
            let val = cpu.get_reg32(*reg);
            cpu.set_reg32(*reg, val.swap_bytes());
            cpu.eip = next_ip;
        }

        // === XADD r/m8, r8 ===
        Opcode::XaddRm8(reg, rm, _) => {
            let dst_val = read_rm8(cpu, mem, rm) as u32;
            let src_val = cpu.get_reg8(*reg) as u32;
            let result = dst_val.wrapping_add(src_val);
            update_flags_add(cpu, dst_val, src_val, result as u64, 8);
            cpu.set_reg8(*reg, dst_val as u8);
            write_rm8(cpu, mem, rm, result as u8);
            cpu.eip = next_ip;
        }
        // === XADD r/mv, rv ===
        Opcode::XaddRmv(reg, rm, _) => {
            let dst_val = read_rmv(cpu, mem, rm, is32);
            let src_val = if is32 { cpu.get_reg32(*reg) } else { cpu.get_reg16(*reg) as u32 };
            let result = dst_val.wrapping_add(src_val);
            update_flags_add(cpu, dst_val, src_val, result as u64, width);
            if is32 { cpu.set_reg32(*reg, dst_val); } else { cpu.set_reg16(*reg, dst_val as u16); }
            write_rmv(cpu, mem, rm, result, is32);
            cpu.eip = next_ip;
        }

        // === CMPXCHG r/m8, r8 ===
        Opcode::CmpxchgRm8(reg, rm, _) => {
            let dst_val = read_rm8(cpu, mem, rm);
            let al = cpu.get_reg8(0);
            if al == dst_val {
                set_flag(cpu, FLAG_ZF, true);
                write_rm8(cpu, mem, rm, cpu.get_reg8(*reg));
            } else {
                set_flag(cpu, FLAG_ZF, false);
                cpu.set_reg8(0, dst_val);
            }
            cpu.eip = next_ip;
        }
        // === CMPXCHG r/mv, rv ===
        Opcode::CmpxchgRmv(reg, rm, _) => {
            let dst_val = read_rmv(cpu, mem, rm, is32);
            let acc = if is32 { cpu.get_reg32(0) } else { cpu.get_reg16(0) as u32 };
            if acc == dst_val {
                set_flag(cpu, FLAG_ZF, true);
                let src = if is32 { cpu.get_reg32(*reg) } else { cpu.get_reg16(*reg) as u32 };
                write_rmv(cpu, mem, rm, src, is32);
            } else {
                set_flag(cpu, FLAG_ZF, false);
                if is32 { cpu.set_reg32(0, dst_val); } else { cpu.set_reg16(0, dst_val as u16); }
            }
            cpu.eip = next_ip;
        }

        // === AAD ===
        Opcode::Aad(imm) => {
            let al = cpu.get_reg8(0);
            let ah = cpu.get_reg8(4);
            let result = ah.wrapping_mul(*imm).wrapping_add(al);
            cpu.set_reg8(0, result); // AL
            cpu.set_reg8(4, 0);      // AH = 0
            update_flags_logic(cpu, result as u32, 8);
            cpu.eip = next_ip;
        }

        // === AAM ===
        Opcode::Aam(imm) => {
            if *imm == 0 {
                cpu.eip = next_ip;
                return ExecResult::DivideError;
            }
            let al = cpu.get_reg8(0);
            cpu.set_reg8(4, al / *imm);  // AH = AL / imm
            cpu.set_reg8(0, al % *imm);  // AL = AL % imm
            update_flags_logic(cpu, cpu.get_reg8(0) as u32, 8);
            cpu.eip = next_ip;
        }

        // === DAA ===
        Opcode::Daa => {
            let old_al = cpu.get_reg8(0);
            let old_cf = get_flag(cpu, FLAG_CF);
            let old_af = get_flag(cpu, FLAG_AF);
            set_flag(cpu, FLAG_CF, false);

            if (old_al & 0x0F) > 9 || old_af {
                let new_al = old_al.wrapping_add(6);
                cpu.set_reg8(0, new_al);
                set_flag(cpu, FLAG_CF, old_cf || old_al > 0xF9);
                set_flag(cpu, FLAG_AF, true);
            } else {
                set_flag(cpu, FLAG_AF, false);
            }

            let al = cpu.get_reg8(0);
            if al > 0x99 || old_cf {
                cpu.set_reg8(0, al.wrapping_add(0x60));
                set_flag(cpu, FLAG_CF, true);
            }

            let al = cpu.get_reg8(0);
            set_flag(cpu, FLAG_SF, (al & 0x80) != 0);
            set_flag(cpu, FLAG_ZF, al == 0);
            set_flag(cpu, FLAG_PF, al.count_ones() % 2 == 0);
            cpu.eip = next_ip;
        }

        // === DAS ===
        Opcode::Das => {
            let old_al = cpu.get_reg8(0);
            let old_cf = get_flag(cpu, FLAG_CF);
            let old_af = get_flag(cpu, FLAG_AF);
            set_flag(cpu, FLAG_CF, false);

            if (old_al & 0x0F) > 9 || old_af {
                let new_al = old_al.wrapping_sub(6);
                cpu.set_reg8(0, new_al);
                set_flag(cpu, FLAG_CF, old_cf || old_al < 6);
                set_flag(cpu, FLAG_AF, true);
            } else {
                set_flag(cpu, FLAG_AF, false);
            }

            let al = cpu.get_reg8(0);
            if old_al > 0x99 || old_cf {
                cpu.set_reg8(0, al.wrapping_sub(0x60));
                set_flag(cpu, FLAG_CF, true);
            }

            let al = cpu.get_reg8(0);
            set_flag(cpu, FLAG_SF, (al & 0x80) != 0);
            set_flag(cpu, FLAG_ZF, al == 0);
            set_flag(cpu, FLAG_PF, al.count_ones() % 2 == 0);
            cpu.eip = next_ip;
        }

        // === AAA ===
        Opcode::Aaa => {
            let al = cpu.get_reg8(0);
            let ah = cpu.get_reg8(4);
            if (al & 0x0F) > 9 || get_flag(cpu, FLAG_AF) {
                cpu.set_reg8(0, al.wrapping_add(6));
                cpu.set_reg8(4, ah.wrapping_add(1));
                set_flag(cpu, FLAG_AF, true);
                set_flag(cpu, FLAG_CF, true);
            } else {
                set_flag(cpu, FLAG_AF, false);
                set_flag(cpu, FLAG_CF, false);
            }
            cpu.set_reg8(0, cpu.get_reg8(0) & 0x0F);
            cpu.eip = next_ip;
        }

        // === AAS ===
        Opcode::Aas => {
            let al = cpu.get_reg8(0);
            let ah = cpu.get_reg8(4);
            if (al & 0x0F) > 9 || get_flag(cpu, FLAG_AF) {
                cpu.set_reg8(0, al.wrapping_sub(6));
                cpu.set_reg8(4, ah.wrapping_sub(1));
                set_flag(cpu, FLAG_AF, true);
                set_flag(cpu, FLAG_CF, true);
            } else {
                set_flag(cpu, FLAG_AF, false);
                set_flag(cpu, FLAG_CF, false);
            }
            cpu.set_reg8(0, cpu.get_reg8(0) & 0x0F);
            cpu.eip = next_ip;
        }

        // === XLAT ===
        Opcode::Xlat => {
            let bx = cpu.get_reg16(3); // BX
            let al = cpu.get_reg8(0) as u16;
            let addr = cpu.linear_addr(cpu.ds, bx.wrapping_add(al));
            cpu.set_reg8(0, mem.read_u8(addr));
            cpu.eip = next_ip;
        }

        // === PUSHA/PUSHAD ===
        Opcode::Pusha => {
            let sp_val = if is32 { cpu.esp } else { cpu.get_reg16(4) as u32 };
            if is32 {
                push32(cpu, mem, cpu.eax);
                push32(cpu, mem, cpu.ecx);
                push32(cpu, mem, cpu.edx);
                push32(cpu, mem, cpu.ebx);
                push32(cpu, mem, sp_val);
                push32(cpu, mem, cpu.ebp);
                push32(cpu, mem, cpu.esi);
                push32(cpu, mem, cpu.edi);
            } else {
                push16(cpu, mem, cpu.get_reg16(0)); // AX
                push16(cpu, mem, cpu.get_reg16(1)); // CX
                push16(cpu, mem, cpu.get_reg16(2)); // DX
                push16(cpu, mem, cpu.get_reg16(3)); // BX
                push16(cpu, mem, sp_val as u16);    // original SP
                push16(cpu, mem, cpu.get_reg16(5)); // BP
                push16(cpu, mem, cpu.get_reg16(6)); // SI
                push16(cpu, mem, cpu.get_reg16(7)); // DI
            }
            cpu.eip = next_ip;
        }

        // === POPA/POPAD ===
        Opcode::Popa => {
            if is32 {
                cpu.edi = pop32(cpu, mem);
                cpu.esi = pop32(cpu, mem);
                cpu.ebp = pop32(cpu, mem);
                let _skip_sp = pop32(cpu, mem);
                cpu.ebx = pop32(cpu, mem);
                cpu.edx = pop32(cpu, mem);
                cpu.ecx = pop32(cpu, mem);
                cpu.eax = pop32(cpu, mem);
            } else {
                let di = pop16(cpu, mem); cpu.set_reg16(7, di);
                let si = pop16(cpu, mem); cpu.set_reg16(6, si);
                let bp = pop16(cpu, mem); cpu.set_reg16(5, bp);
                let _skip_sp = pop16(cpu, mem);
                let bx = pop16(cpu, mem); cpu.set_reg16(3, bx);
                let dx = pop16(cpu, mem); cpu.set_reg16(2, dx);
                let cx = pop16(cpu, mem); cpu.set_reg16(1, cx);
                let ax = pop16(cpu, mem); cpu.set_reg16(0, ax);
            }
            cpu.eip = next_ip;
        }

        // === BOUND (just NOP - no bounds check) ===
        Opcode::Bound(_reg, _rm, _) => {
            cpu.eip = next_ip;
        }

        // === ARPL (NOP in real mode, stub in protected mode) ===
        Opcode::Arpl(_reg, rm, _) => {
            // In protected mode: adjust RPL of r/m to be >= RPL of reg
            // For now, just set ZF=0 (no adjustment needed)
            let _ = rm;
            set_flag(cpu, FLAG_ZF, false);
            cpu.eip = next_ip;
        }

        // === LES ===
        Opcode::Les(reg, rm, _) => {
            if let ModrmOperand::Mem(addr) = rm {
                if is32 {
                    let offset = mem.read_u32(*addr);
                    let seg = mem.read_u16(*addr + 4);
                    cpu.set_reg32(*reg, offset);
                    crate::descriptor::load_segment(cpu, mem, 0, seg); // ES = index 0
                } else {
                    let offset = mem.read_u16(*addr);
                    let seg = mem.read_u16(*addr + 2);
                    cpu.set_reg16(*reg, offset);
                    crate::descriptor::load_segment(cpu, mem, 0, seg);
                }
            }
            cpu.eip = next_ip;
        }

        // === LDS ===
        Opcode::Lds(reg, rm, _) => {
            if let ModrmOperand::Mem(addr) = rm {
                if is32 {
                    let offset = mem.read_u32(*addr);
                    let seg = mem.read_u16(*addr + 4);
                    cpu.set_reg32(*reg, offset);
                    crate::descriptor::load_segment(cpu, mem, 3, seg); // DS = index 3
                } else {
                    let offset = mem.read_u16(*addr);
                    let seg = mem.read_u16(*addr + 2);
                    cpu.set_reg16(*reg, offset);
                    crate::descriptor::load_segment(cpu, mem, 3, seg);
                }
            }
            cpu.eip = next_ip;
        }

        // === INT 3 ===
        Opcode::Int3 => {
            cpu.eip = next_ip;
            if !int_handler.handle_int(cpu, mem, 3) {
                log::warn!("Unhandled INT 3 (breakpoint)");
            }
        }

        // === INTO ===
        Opcode::Into => {
            cpu.eip = next_ip;
            if get_flag(cpu, FLAG_OF) {
                if !int_handler.handle_int(cpu, mem, 4) {
                    log::warn!("Unhandled INTO (overflow)");
                }
            }
        }

        // === LAR (stub: ZF=0 means not accessible) ===
        Opcode::Lar(_reg, _rm, _) => {
            set_flag(cpu, FLAG_ZF, false);
            cpu.eip = next_ip;
        }

        // === LSL (stub: ZF=0 means not accessible) ===
        Opcode::Lsl(_reg, _rm, _) => {
            set_flag(cpu, FLAG_ZF, false);
            cpu.eip = next_ip;
        }

        // === CLTS ===
        Opcode::Clts => {
            cpu.cr0 &= !(1 << 3); // Clear TS (Task Switched) flag
            cpu.eip = next_ip;
        }

        Opcode::Unknown(byte) => {
            log::error!("Unknown opcode: 0x{:02X} at {:04X}:{:04X}", byte, cpu.cs, cpu.eip);
            return ExecResult::UnknownOpcode(*byte);
        }
    }

    ExecResult::Continue
}

// ============================================================
// Helper functions
// ============================================================

/// Find the actual opcode byte (skipping prefixes) for direction detection
fn find_opcode_byte(cpu: &CpuState, mem: &MemoryBus, inst: &Instruction) -> u8 {
    let base = cpu.cs_ip();
    for i in 0..inst.len as u32 {
        let b = mem.read_u8(base + i);
        match b {
            0x26 | 0x2E | 0x36 | 0x3E | 0x64 | 0x65 | 0x66 | 0x67 | 0xF0 => continue,
            _ => return b,
        }
    }
    0
}

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

fn read_rm32(cpu: &CpuState, mem: &MemoryBus, rm: &ModrmOperand) -> u32 {
    match rm {
        ModrmOperand::Reg(idx) => cpu.get_reg32(*idx),
        ModrmOperand::Mem(addr) => mem.read_u32(*addr),
    }
}

fn read_rmv(cpu: &CpuState, mem: &MemoryBus, rm: &ModrmOperand, is32: bool) -> u32 {
    if is32 { read_rm32(cpu, mem, rm) } else { read_rm16(cpu, mem, rm) as u32 }
}

fn write_rm8(cpu: &mut CpuState, mem: &mut MemoryBus, rm: &ModrmOperand, val: u8) {
    match rm { ModrmOperand::Reg(idx) => cpu.set_reg8(*idx, val), ModrmOperand::Mem(addr) => mem.write_u8(*addr, val) }
}

fn write_rm16(cpu: &mut CpuState, mem: &mut MemoryBus, rm: &ModrmOperand, val: u16) {
    match rm { ModrmOperand::Reg(idx) => cpu.set_reg16(*idx, val), ModrmOperand::Mem(addr) => mem.write_u16(*addr, val) }
}

fn write_rm32(cpu: &mut CpuState, mem: &mut MemoryBus, rm: &ModrmOperand, val: u32) {
    match rm { ModrmOperand::Reg(idx) => cpu.set_reg32(*idx, val), ModrmOperand::Mem(addr) => mem.write_u32(*addr, val) }
}

fn write_rmv(cpu: &mut CpuState, mem: &mut MemoryBus, rm: &ModrmOperand, val: u32, is32: bool) {
    if is32 { write_rm32(cpu, mem, rm, val); } else { write_rm16(cpu, mem, rm, val as u16); }
}

/// Compute stack linear address: SS:SP in real mode, ss_cache.base+ESP in protected
fn stack_addr(cpu: &CpuState, offset: u32) -> u32 {
    if cpu.mode == crate::regs::CpuMode::ProtectedMode {
        cpu.ss_cache.base.wrapping_add(offset)
    } else {
        ((cpu.ss as u32) << 4).wrapping_add(offset & 0xFFFF)
    }
}

fn push16(cpu: &mut CpuState, mem: &mut MemoryBus, val: u16) {
    let sp = cpu.get_reg16(4).wrapping_sub(2);
    cpu.set_reg16(4, sp);
    let addr = stack_addr(cpu, sp as u32);
    mem.write_u16(addr, val);
}

fn pop16(cpu: &mut CpuState, mem: &mut MemoryBus) -> u16 {
    let sp = cpu.get_reg16(4);
    let addr = stack_addr(cpu, sp as u32);
    let val = mem.read_u16(addr);
    cpu.set_reg16(4, sp.wrapping_add(2));
    val
}

fn push32(cpu: &mut CpuState, mem: &mut MemoryBus, val: u32) {
    cpu.esp = cpu.esp.wrapping_sub(4);
    let addr = stack_addr(cpu, cpu.esp);
    mem.write_u32(addr, val);
}

fn pop32(cpu: &mut CpuState, mem: &mut MemoryBus) -> u32 {
    let addr = stack_addr(cpu, cpu.esp);
    let val = mem.read_u32(addr);
    cpu.esp = cpu.esp.wrapping_add(4);
    val
}

fn pushv(cpu: &mut CpuState, mem: &mut MemoryBus, val: u32, is32: bool) {
    if is32 { push32(cpu, mem, val); } else { push16(cpu, mem, val as u16); }
}

fn popv(cpu: &mut CpuState, mem: &mut MemoryBus, is32: bool) -> u32 {
    if is32 { pop32(cpu, mem) } else { pop16(cpu, mem) as u32 }
}

fn exec_alu(cpu: &mut CpuState, op: AluOp, a: u32, b: u32, width: u8) -> u32 {
    let mask: u64 = match width { 8 => 0xFF, 16 => 0xFFFF, 32 => 0xFFFFFFFF, _ => unreachable!() };
    let result = match op {
        AluOp::Add => {
            let r = (a as u64).wrapping_add(b as u64);
            update_flags_add(cpu, a, b, r, width);
            r
        }
        AluOp::Or => {
            let r = (a | b) as u64;
            update_flags_logic(cpu, r as u32, width);
            r
        }
        AluOp::Adc => {
            let carry = if get_flag(cpu, FLAG_CF) { 1u64 } else { 0 };
            let r = (a as u64).wrapping_add(b as u64).wrapping_add(carry);
            update_flags_add(cpu, a, b.wrapping_add(carry as u32), r, width);
            r
        }
        AluOp::Sbb => {
            let borrow = if get_flag(cpu, FLAG_CF) { 1u64 } else { 0 };
            let r = (a as u64).wrapping_sub(b as u64).wrapping_sub(borrow);
            update_flags_sub(cpu, a, b.wrapping_add(borrow as u32), r, width);
            r
        }
        AluOp::And => {
            let r = (a & b) as u64;
            update_flags_logic(cpu, r as u32, width);
            r
        }
        AluOp::Sub | AluOp::Cmp => {
            let r = (a as u64).wrapping_sub(b as u64);
            update_flags_sub(cpu, a, b, r, width);
            r
        }
        AluOp::Xor => {
            let r = (a ^ b) as u64;
            update_flags_logic(cpu, r as u32, width);
            r
        }
    };
    (result & mask) as u32
}

fn resolve_shift_count(cpu: &CpuState, count: &ShiftCount) -> u8 {
    match count {
        ShiftCount::One => 1,
        ShiftCount::CL => cpu.get_reg8(1), // CL
        ShiftCount::Imm(v) => *v,
    }
}

fn exec_shift(cpu: &mut CpuState, op: ShiftOp, val: u32, count: u8, width: u8) -> u32 {
    let mask: u32 = match width { 8 => 0xFF, 16 => 0xFFFF, 32 => 0xFFFFFFFF, _ => unreachable!() };
    let cnt = count as u32;

    let result = match op {
        ShiftOp::Shl | ShiftOp::Sal => {
            let r = (val << cnt) & mask;
            let last_out = (val >> (width as u32 - cnt)) & 1;
            set_flag(cpu, FLAG_CF, last_out != 0);
            if cnt == 1 {
                let msb = (r >> (width as u32 - 1)) & 1;
                set_flag(cpu, FLAG_OF, msb != last_out as u32);
            }
            update_flags_logic_preserve_cf(cpu, r, width);
            r
        }
        ShiftOp::Shr => {
            let last_out = (val >> (cnt - 1)) & 1;
            let r = (val >> cnt) & mask;
            set_flag(cpu, FLAG_CF, last_out != 0);
            if cnt == 1 {
                set_flag(cpu, FLAG_OF, (val >> (width as u32 - 1)) & 1 != 0);
            }
            update_flags_logic_preserve_cf(cpu, r, width);
            r
        }
        ShiftOp::Sar => {
            let sign_bit = 1u32 << (width as u32 - 1);
            let mut r = val;
            for _ in 0..cnt {
                let last = r & 1;
                r = (r >> 1) | (r & sign_bit);
                set_flag(cpu, FLAG_CF, last != 0);
            }
            r &= mask;
            if cnt == 1 { set_flag(cpu, FLAG_OF, false); }
            update_flags_logic_preserve_cf(cpu, r, width);
            r
        }
        ShiftOp::Rol => {
            let mut r = val;
            for _ in 0..cnt {
                let msb = (r >> (width as u32 - 1)) & 1;
                r = ((r << 1) | msb) & mask;
            }
            set_flag(cpu, FLAG_CF, r & 1 != 0);
            if cnt == 1 {
                let msb = (r >> (width as u32 - 1)) & 1;
                set_flag(cpu, FLAG_OF, msb ^ (r & 1) != 0);
            }
            r
        }
        ShiftOp::Ror => {
            let mut r = val;
            for _ in 0..cnt {
                let lsb = r & 1;
                r = (r >> 1) | (lsb << (width as u32 - 1));
                r &= mask;
            }
            set_flag(cpu, FLAG_CF, (r >> (width as u32 - 1)) & 1 != 0);
            if cnt == 1 {
                let msb = (r >> (width as u32 - 1)) & 1;
                let msb1 = (r >> (width as u32 - 2)) & 1;
                set_flag(cpu, FLAG_OF, msb ^ msb1 != 0);
            }
            r
        }
        ShiftOp::Rcl => {
            let mut r = val;
            for _ in 0..cnt {
                let cf = if get_flag(cpu, FLAG_CF) { 1u32 } else { 0 };
                let msb = (r >> (width as u32 - 1)) & 1;
                r = ((r << 1) | cf) & mask;
                set_flag(cpu, FLAG_CF, msb != 0);
            }
            if cnt == 1 {
                let msb = (r >> (width as u32 - 1)) & 1;
                set_flag(cpu, FLAG_OF, msb ^ (if get_flag(cpu, FLAG_CF) { 1 } else { 0 }) != 0);
            }
            r
        }
        ShiftOp::Rcr => {
            let mut r = val;
            for _ in 0..cnt {
                let cf = if get_flag(cpu, FLAG_CF) { 1u32 } else { 0 };
                let lsb = r & 1;
                r = (r >> 1) | (cf << (width as u32 - 1));
                r &= mask;
                set_flag(cpu, FLAG_CF, lsb != 0);
            }
            if cnt == 1 {
                let msb = (r >> (width as u32 - 1)) & 1;
                let msb1 = (r >> (width as u32 - 2)) & 1;
                set_flag(cpu, FLAG_OF, msb ^ msb1 != 0);
            }
            r
        }
    };
    result
}

fn update_flags_logic_preserve_cf(cpu: &mut CpuState, result: u32, width: u8) {
    let cf = get_flag(cpu, FLAG_CF);
    update_flags_logic(cpu, result, width);
    set_flag(cpu, FLAG_CF, cf);
}

// === String operations ===

fn exec_movsb(cpu: &mut CpuState, mem: &mut MemoryBus) {
    let src = cpu.linear_addr(cpu.ds, cpu.get_reg16(6));
    let dst = cpu.linear_addr(cpu.es, cpu.get_reg16(7));
    mem.write_u8(dst, mem.read_u8(src));
    let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFF } else { 1 };
    cpu.set_reg16(6, cpu.get_reg16(6).wrapping_add(d));
    cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(d));
}

fn exec_movsw(cpu: &mut CpuState, mem: &mut MemoryBus) {
    let src = cpu.linear_addr(cpu.ds, cpu.get_reg16(6));
    let dst = cpu.linear_addr(cpu.es, cpu.get_reg16(7));
    mem.write_u16(dst, mem.read_u16(src));
    let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFE } else { 2 };
    cpu.set_reg16(6, cpu.get_reg16(6).wrapping_add(d));
    cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(d));
}

fn exec_movsd(cpu: &mut CpuState, mem: &mut MemoryBus) {
    let src = cpu.linear_addr(cpu.ds, cpu.get_reg16(6));
    let dst = cpu.linear_addr(cpu.es, cpu.get_reg16(7));
    mem.write_u32(dst, mem.read_u32(src));
    let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFC } else { 4 };
    cpu.set_reg16(6, cpu.get_reg16(6).wrapping_add(d));
    cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(d));
}

fn exec_stosb(cpu: &mut CpuState, mem: &mut MemoryBus) {
    let addr = cpu.linear_addr(cpu.es, cpu.get_reg16(7));
    mem.write_u8(addr, cpu.get_reg8(0));
    let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFF } else { 1 };
    cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(d));
}

fn exec_stosw(cpu: &mut CpuState, mem: &mut MemoryBus) {
    let addr = cpu.linear_addr(cpu.es, cpu.get_reg16(7));
    mem.write_u16(addr, cpu.get_reg16(0));
    let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFE } else { 2 };
    cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(d));
}

fn exec_stosd(cpu: &mut CpuState, mem: &mut MemoryBus) {
    let addr = cpu.linear_addr(cpu.es, cpu.get_reg16(7));
    mem.write_u32(addr, cpu.get_reg32(0));
    let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFC } else { 4 };
    cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(d));
}

fn exec_lodsb(cpu: &mut CpuState, mem: &mut MemoryBus) {
    let addr = cpu.linear_addr(cpu.ds, cpu.get_reg16(6));
    cpu.set_reg8(0, mem.read_u8(addr));
    let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFF } else { 1 };
    cpu.set_reg16(6, cpu.get_reg16(6).wrapping_add(d));
}

fn exec_lodsw(cpu: &mut CpuState, mem: &mut MemoryBus) {
    let addr = cpu.linear_addr(cpu.ds, cpu.get_reg16(6));
    cpu.set_reg16(0, mem.read_u16(addr));
    let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFE } else { 2 };
    cpu.set_reg16(6, cpu.get_reg16(6).wrapping_add(d));
}

fn exec_lodsd(cpu: &mut CpuState, mem: &mut MemoryBus) {
    let addr = cpu.linear_addr(cpu.ds, cpu.get_reg16(6));
    cpu.set_reg32(0, mem.read_u32(addr));
    let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFC } else { 4 };
    cpu.set_reg16(6, cpu.get_reg16(6).wrapping_add(d));
}

fn exec_cmpsb(cpu: &mut CpuState, mem: &mut MemoryBus) {
    let src = cpu.linear_addr(cpu.ds, cpu.get_reg16(6));
    let dst = cpu.linear_addr(cpu.es, cpu.get_reg16(7));
    let a = mem.read_u8(src) as u32;
    let b = mem.read_u8(dst) as u32;
    let r = (a as u64).wrapping_sub(b as u64);
    update_flags_sub(cpu, a, b, r, 8);
    let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFF } else { 1 };
    cpu.set_reg16(6, cpu.get_reg16(6).wrapping_add(d));
    cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(d));
}

fn exec_cmpsw(cpu: &mut CpuState, mem: &mut MemoryBus) {
    let src = cpu.linear_addr(cpu.ds, cpu.get_reg16(6));
    let dst = cpu.linear_addr(cpu.es, cpu.get_reg16(7));
    let a = mem.read_u16(src) as u32;
    let b = mem.read_u16(dst) as u32;
    let r = (a as u64).wrapping_sub(b as u64);
    update_flags_sub(cpu, a, b, r, 16);
    let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFE } else { 2 };
    cpu.set_reg16(6, cpu.get_reg16(6).wrapping_add(d));
    cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(d));
}

fn exec_scasb(cpu: &mut CpuState, mem: &mut MemoryBus) {
    let addr = cpu.linear_addr(cpu.es, cpu.get_reg16(7));
    let a = cpu.get_reg8(0) as u32;
    let b = mem.read_u8(addr) as u32;
    let r = (a as u64).wrapping_sub(b as u64);
    update_flags_sub(cpu, a, b, r, 8);
    let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFF } else { 1 };
    cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(d));
}

fn exec_scasw(cpu: &mut CpuState, mem: &mut MemoryBus) {
    let addr = cpu.linear_addr(cpu.es, cpu.get_reg16(7));
    let a = cpu.get_reg16(0) as u32;
    let b = mem.read_u16(addr) as u32;
    let r = (a as u64).wrapping_sub(b as u64);
    update_flags_sub(cpu, a, b, r, 16);
    let d: u16 = if get_flag(cpu, FLAG_DF) { 0xFFFE } else { 2 };
    cpu.set_reg16(7, cpu.get_reg16(7).wrapping_add(d));
}
