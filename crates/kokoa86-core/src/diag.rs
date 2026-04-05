/// Diagnostic utilities for debugging BIOS boot
use crate::Machine;
use kokoa86_cpu::decode;
use kokoa86_cpu::execute::ExecResult;
use kokoa86_mem::MemoryAccess;

/// Run the machine for N instructions, printing a trace of the first `trace_count`
/// and returning a diagnostic summary.
pub fn trace_boot(machine: &mut Machine, max_inst: u64, trace_count: u64) -> String {
    let mut output = String::new();
    let mut unknown_opcodes: Vec<(u8, u16, u32)> = Vec::new();
    let mut last_cs_ip = (0u16, 0u32);
    let mut loop_count = 0u64;

    output.push_str(&format!(
        "=== Boot Trace ===\nStart: {:04X}:{:04X}\n\n",
        machine.cpu.cs, machine.cpu.eip
    ));

    for i in 0..max_inst {
        let cs = machine.cpu.cs;
        let ip = machine.cpu.eip;

        // Detect infinite loops
        if (cs, ip) == last_cs_ip {
            loop_count += 1;
            if loop_count > 100_000 {
                output.push_str(&format!(
                    "\n!!! Infinite loop detected at {:04X}:{:04X} after {} instructions\n",
                    cs, ip, i
                ));
                break;
            }
        } else {
            loop_count = 0;
            last_cs_ip = (cs, ip);
        }

        // Trace first N instructions
        if i < trace_count {
            let inst = decode::decode(&machine.cpu, &machine.mem);
            let mut bytes = String::new();
            let linear = machine.cpu.cs_ip();
            for j in 0..inst.len.min(8) as u32 {
                bytes.push_str(&format!("{:02X} ", machine.mem.read_u8(linear + j)));
            }
            output.push_str(&format!(
                "{:5}: {:04X}:{:04X}  {:<24} {:?}\n",
                i, cs, ip, bytes.trim(), inst.op
            ));
        }

        match machine.step() {
            Ok(ExecResult::Continue) => {}
            Ok(ExecResult::Halt) => {
                output.push_str(&format!(
                    "\nCPU halted at {:04X}:{:04X} after {} instructions\n",
                    machine.cpu.cs, machine.cpu.eip, i
                ));
                break;
            }
            Ok(ExecResult::UnknownOpcode(byte)) => {
                unknown_opcodes.push((byte, cs, ip));
                output.push_str(&format!(
                    "\n!!! Unknown opcode 0x{:02X} at {:04X}:{:04X} after {} instructions\n",
                    byte, cs, ip, i
                ));
                break;
            }
            Ok(ExecResult::DivideError) => {
                output.push_str(&format!(
                    "\n!!! Divide error at {:04X}:{:04X} after {} instructions\n",
                    cs, ip, i
                ));
                break;
            }
            Err(e) => {
                output.push_str(&format!("\n!!! Error: {} after {} instructions\n", e, i));
                break;
            }
        }
    }

    // Dump 10 instructions at final IP
    output.push_str("\n=== Instructions at final IP ===\n");
    {
        let mut addr = machine.cpu.cs_ip();
        for _ in 0..10 {
            let inst = decode::decode_at_addr(&machine.cpu, &machine.mem, addr);
            let mut bytes = String::new();
            for j in 0..inst.len.min(8) as u32 {
                bytes.push_str(&format!("{:02X} ", machine.mem.read_u8(addr + j)));
            }
            output.push_str(&format!(
                "  {:08X}  {:<24} {:?}\n",
                addr, bytes.trim(), inst.op
            ));
            addr += inst.len as u32;
        }
    }

    // Dump final state
    output.push_str(&format!(
        "\n=== Final State ===\n\
         AX={:08X} BX={:08X} CX={:08X} DX={:08X}\n\
         SP={:08X} BP={:08X} SI={:08X} DI={:08X}\n\
         CS={:04X} DS={:04X} ES={:04X} SS={:04X}\n\
         IP={:08X} FLAGS={:08X}\n\
         CR0={:08X} Mode={:?}\n\
         Instructions executed: {}\n",
        machine.cpu.eax, machine.cpu.ebx, machine.cpu.ecx, machine.cpu.edx,
        machine.cpu.esp, machine.cpu.ebp, machine.cpu.esi, machine.cpu.edi,
        machine.cpu.cs, machine.cpu.ds, machine.cpu.es, machine.cpu.ss,
        machine.cpu.eip, machine.cpu.eflags,
        machine.cpu.cr0, machine.cpu.mode,
        machine.instruction_count,
    ));

    // Check VGA buffer
    let mut vga_has_content = false;
    for i in 0..(80 * 25 * 2) as u32 {
        if machine.mem.read_u8(0xB8000 + i) != 0 {
            vga_has_content = true;
            break;
        }
    }
    output.push_str(&format!("VGA buffer has content: {}\n", vga_has_content));

    // Check serial output (POST codes)
    output.push_str(&format!("POST code (port 0x80): check device\n"));

    output
}
