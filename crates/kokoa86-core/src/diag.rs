use crate::Machine;
use kokoa86_cpu::decode;
use kokoa86_cpu::execute::ExecResult;
use kokoa86_mem::MemoryAccess;

pub fn trace_boot(machine: &mut Machine, max_inst: u64, _trace_first: u64) -> String {
    let mut output = String::new();
    let mut last_serial_len = 0usize;
    let mut while_hit = 0u32;

    for i in 0..max_inst {
        if machine.serial_output.len() > last_serial_len {
            let new = String::from_utf8_lossy(&machine.serial_output[last_serial_len..]);
            for line in new.split('\n') {
                if !line.is_empty() {
                    output.push_str(&format!("[serial @{:>8}] {}\n", i, line));
                }
            }
            last_serial_len = machine.serial_output.len();
        }

        // Detect CMP_A (while loop condition) at 0x3FFAFE91
        if machine.cpu.cs_ip() == 0x3FFAFE91 {
            while_hit += 1;
            if while_hit <= 3 {
                output.push_str(&format!(
                    "\n=== while loop hit #{} at inst {} ===\n  EBX(bus?)={:08X} FLAGS={:04X}\n",
                    while_hit, i, machine.cpu.ebx, machine.cpu.eflags as u16
                ));
                // Trace 30 instructions around this point
                for j in 0..30u64 {
                    let ll = machine.cpu.cs_ip();
                    let inst = decode::decode_at_addr(&machine.cpu, &machine.mem, ll);
                    let mut bytes = String::new();
                    for k in 0..inst.len.min(7) as u32 {
                        bytes.push_str(&format!("{:02X} ", machine.mem.read_u8(ll + k)));
                    }
                    output.push_str(&format!(
                        "  {:>2}: {:08X} {:<21} {:?}  A={:08X} B={:08X} FL={:04X}\n",
                        j, ll, bytes.trim(), inst.op,
                        machine.cpu.eax, machine.cpu.ebx, machine.cpu.eflags as u16
                    ));
                    match machine.step() {
                        Ok(ExecResult::Continue) => {}
                        _ => break,
                    }
                }
            }
            if while_hit >= 3 { break; }
        }

        match machine.step() {
            Ok(ExecResult::Continue) => {}
            Ok(ExecResult::Halt) => { output.push_str("HALT\n"); break; }
            Ok(ExecResult::UnknownOpcode(b)) => { output.push_str(&format!("UNKNOWN {:02X}\n", b)); break; }
            _ => {}
        }
    }

    output.push_str(&format!("\nwhile_hits: {}\n", while_hit));
    output
}
