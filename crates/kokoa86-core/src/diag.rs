use crate::Machine;
use kokoa86_cpu::decode;
use kokoa86_cpu::execute::ExecResult;
use kokoa86_mem::MemoryAccess;

pub fn trace_boot(machine: &mut Machine, max_inst: u64, trace_first: u64) -> String {
    let mut output = String::new();
    output.push_str(&format!("=== Boot Trace === Start: {:04X}:{:04X}\n\n",
        machine.cpu.cs, machine.cpu.eip));

    let mut last_serial_len = 0usize;

    // Track loop detection: if IP stays in same 256-byte range for too long
    let mut loop_range_start: u32 = 0;
    let mut loop_count: u64 = 0;
    let mut loop_traced = false;

    for i in 0..max_inst {
        let cs = machine.cpu.cs;
        let ip = machine.cpu.eip;
        let lip = machine.cpu.cs_ip();

        // Detect serial output growth
        if machine.serial_output.len() > last_serial_len {
            let new_text = String::from_utf8_lossy(&machine.serial_output[last_serial_len..]);
            for line in new_text.split('\n') {
                if !line.is_empty() {
                    output.push_str(&format!("[serial @{:>8}] {}\n", i, line));
                }
            }
            last_serial_len = machine.serial_output.len();
        }

        // Trace first N instructions
        if i < trace_first {
            trace_one(&mut output, machine, i);
        }

        // Loop detection: if IP stays in same 0x100 range
        let range = lip & !0xFF;
        if range == loop_range_start {
            loop_count += 1;
        } else {
            if loop_count > 10000 && !loop_traced {
                output.push_str(&format!(
                    "\n[loop detected: {} iterations in range {:08X}-{:08X}, left at inst {}]\n",
                    loop_count, loop_range_start, loop_range_start + 0xFF, i
                ));
            }
            loop_range_start = range;
            loop_count = 0;
        }

        // When stuck in loop for 100K iterations, trace 100 instructions then break
        if loop_count == 15_000 && !loop_traced {
            loop_traced = true;
            output.push_str(&format!(
                "\n!!! Stuck in loop at {:08X} for 1M iterations (inst {})\n",
                lip, i
            ));
            output.push_str(&format!(
                "Regs: A={:08X} B={:08X} C={:08X} D={:08X} SI={:08X} DI={:08X} BP={:08X} SP={:08X}\n",
                machine.cpu.eax, machine.cpu.ebx, machine.cpu.ecx, machine.cpu.edx,
                machine.cpu.esi, machine.cpu.edi, machine.cpu.ebp, machine.cpu.esp
            ));

            // Trace 100 instructions
            for j in 0..100 {
                trace_one(&mut output, machine, i + j);
                match machine.step() {
                    Ok(ExecResult::Continue) => {}
                    Ok(ExecResult::Halt) => { output.push_str("  HALT\n"); break; }
                    Ok(ExecResult::UnknownOpcode(b)) => {
                        output.push_str(&format!("  UNKNOWN 0x{:02X}\n", b)); break;
                    }
                    _ => break,
                }
            }
            break;
        }

        match machine.step() {
            Ok(ExecResult::Continue) => {}
            Ok(ExecResult::Halt) => {
                output.push_str(&format!("\nHALT at {:04X}:{:04X} after {} inst\n", cs, ip, i));
                break;
            }
            Ok(ExecResult::UnknownOpcode(byte)) => {
                output.push_str(&format!("\n!!! Unknown 0x{:02X} at {:04X}:{:04X} after {} inst\n", byte, cs, ip, i));
                break;
            }
            Ok(ExecResult::DivideError) => {}
            Err(e) => {
                output.push_str(&format!("\n!!! Error: {} after {} inst\n", e, i));
                break;
            }
        }
    }

    output.push_str(&format!(
        "\n=== Final ===\nSerial: {} bytes | VGA: {} | Inst: {}\n",
        machine.serial_output.len(),
        if (0..4000u32).any(|i| machine.mem.read_u8(0xB8000 + i) != 0) { "has content" } else { "empty" },
        machine.instruction_count,
    ));

    output
}

fn trace_one(output: &mut String, machine: &Machine, i: u64) {
    let lip = machine.cpu.cs_ip();
    let inst = decode::decode_at_addr(&machine.cpu, &machine.mem, lip);
    let mut bytes = String::new();
    for j in 0..inst.len.min(8) as u32 {
        bytes.push_str(&format!("{:02X} ", machine.mem.read_u8(lip + j)));
    }
    output.push_str(&format!(
        "{:>8}: {:08X}  {:<24} {:?}  A={:08X} B={:08X} BP={:08X} FL={:04X}\n",
        i, lip, bytes.trim(), inst.op,
        machine.cpu.eax, machine.cpu.ebx, machine.cpu.ebp, machine.cpu.eflags as u16
    ));
}
