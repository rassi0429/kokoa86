use crate::Machine;
use kokoa86_cpu::decode;
use kokoa86_cpu::execute::ExecResult;
use kokoa86_mem::MemoryAccess;

pub fn trace_boot(machine: &mut Machine, max_inst: u64, trace_first: u64) -> String {
    let mut output = String::new();
    let mut last_serial_len = 0usize;
    let mut alloc_count = 0u32;

    for i in 0..max_inst {
        let lip = machine.cpu.cs_ip();

        // Serial output
        if machine.serial_output.len() > last_serial_len {
            let new = String::from_utf8_lossy(&machine.serial_output[last_serial_len..]);
            for line in new.split('\n') {
                if !line.is_empty() {
                    output.push_str(&format!("[serial @{:>8}] {}\n", i, line));
                }
            }
            last_serial_len = machine.serial_output.len();
        }

        // Trace first N
        if i < trace_first {
            let inst = decode::decode_at_addr(&machine.cpu, &machine.mem, lip);
            let mut bytes = String::new();
            for j in 0..inst.len.min(8) as u32 {
                bytes.push_str(&format!("{:02X} ", machine.mem.read_u8(lip + j)));
            }
            output.push_str(&format!("{:>8}: {:08X}  {:<24} {:?}\n", i, lip, bytes.trim(), inst.op));
        }

        // Track alloc entry (0x0FFAD462) and alloc return (ret to caller, check EAX)
        // Entry: CALL to 0x0FFAD462
        if lip == 0x0FFAD462 {
            alloc_count += 1;
            if alloc_count <= 10 || alloc_count % 100 == 0 {
                // Before alloc, dump the PCI bdf being processed
                // The caller's local variable should have the bdf
                // ECX typically has the bdf or the zone count
                output.push_str(&format!(
                    "[alloc #{:>5} @inst {:>8}] EAX={:08X} EBX={:08X} ECX={:08X} EDX={:08X} ESP={:08X}\n",
                    alloc_count, i,
                    machine.cpu.eax, machine.cpu.ebx, machine.cpu.ecx, machine.cpu.edx, machine.cpu.esp
                ));
            }
            if alloc_count > 1000 {
                output.push_str(&format!("\n!!! >1000 allocs, stopping. Total allocs so far: {}\n", alloc_count));
                break;
            }
        }

        match machine.step() {
            Ok(ExecResult::Continue) => {}
            Ok(ExecResult::Halt) => {
                output.push_str(&format!("\nHALT at {:08X} after {} inst\n", lip, i));
                break;
            }
            Ok(ExecResult::UnknownOpcode(b)) => {
                output.push_str(&format!("\nUNKNOWN 0x{:02X} at {:08X}\n", b, lip));
                break;
            }
            _ => {}
        }
    }

    output.push_str(&format!("\nSerial: {} bytes, Allocs: {}\n", machine.serial_output.len(), alloc_count));
    output
}
