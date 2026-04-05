use crate::Machine;
use kokoa86_cpu::decode;
use kokoa86_cpu::execute::ExecResult;
use kokoa86_mem::MemoryAccess;

pub fn trace_boot(machine: &mut Machine, max_inst: u64, _trace_first: u64) -> String {
    let mut output = String::new();
    let mut last_serial_len = 0usize;
    let mut alloc_count = 0u32;
    let mut trace_next = 0u64; // trace N instructions starting from this

    for i in 0..max_inst {
        let lip = machine.cpu.cs_ip();

        if machine.serial_output.len() > last_serial_len {
            let new = String::from_utf8_lossy(&machine.serial_output[last_serial_len..]);
            for line in new.split('\n') {
                if !line.is_empty() {
                    output.push_str(&format!("[serial @{:>8}] {}\n", i, line));
                }
            }
            last_serial_len = machine.serial_output.len();
        }

        // Trace when requested
        if i >= trace_next && i < trace_next + 200 && trace_next > 0 {
            let inst = decode::decode_at_addr(&machine.cpu, &machine.mem, lip);
            let mut bytes = String::new();
            for j in 0..inst.len.min(8) as u32 {
                bytes.push_str(&format!("{:02X} ", machine.mem.read_u8(lip + j)));
            }
            output.push_str(&format!("{:>8}: {:08X} {:<20} {:?}  A={:08X} B={:08X}\n",
                i, lip, bytes.trim(), inst.op, machine.cpu.eax, machine.cpu.ebx));
        }

        // Count alloc_new calls
        if lip == 0x0FFAD462 {
            alloc_count += 1;
            if alloc_count == 5 {
                // Trace 200 instructions BEFORE the next alloc to see the loop
                output.push_str(&format!("\n=== Tracing around 5th alloc (inst {}) ===\n", i));
                // We can't go back, so trace starting now
                trace_next = i;
            }
            // Don't stop — let it run
            if alloc_count % 100 == 0 {
                output.push_str(&format!("[alloc #{}]\n", alloc_count));
            }
        }

        match machine.step() {
            Ok(ExecResult::Continue) => {}
            Ok(ExecResult::Halt) => { output.push_str(&format!("\nHALT at {:08X}\n", lip)); break; }
            Ok(ExecResult::UnknownOpcode(b)) => { output.push_str(&format!("\nUNKNOWN 0x{:02X}\n", b)); break; }
            _ => {}
        }
    }

    output.push_str(&format!("\nSerial: {} bytes, Allocs: {}\n", machine.serial_output.len(), alloc_count));
    output
}
