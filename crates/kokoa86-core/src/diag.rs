use crate::Machine;
use kokoa86_cpu::decode;
use kokoa86_cpu::execute::ExecResult;
use kokoa86_mem::MemoryAccess;

pub fn trace_boot(machine: &mut Machine, max_inst: u64, _trace_first: u64) -> String {
    let mut output = String::new();
    let mut last_serial_len = 0usize;
    let mut alloc_count = 0u32;

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

        // alloc tracking disabled — address changes with relocation

        // Trace the caller of alloc #6 (the 3rd pci_device alloc)
        // alloc #5 is size=32 at inst ~73411
        // Trace 700 instructions from inst 73411 to see the full iteration
        if false && alloc_count == 5 && lip == 0x0FFAD462 {
            output.push_str(&format!("\n=== Tracing iteration around alloc #5 (inst {}) ===\n", i));
            // Trace backwards is impossible, so trace forward 700 inst
            for j in 0..1200u64 {
                let ii = i + j;
                let ll = machine.cpu.cs_ip();
                let inst = decode::decode_at_addr(&machine.cpu, &machine.mem, ll);
                let mut bytes = String::new();
                for k in 0..inst.len.min(6) as u32 {
                    bytes.push_str(&format!("{:02X} ", machine.mem.read_u8(ll + k)));
                }
                // Only show CALL, RET, JMP, and key MOV/CMP
                // Show everything except memset inner loop and alloc_new inner loop
                let show = !(ll >= 0x000EA973 && ll <= 0x000EA97B)  // skip memset
                    && !(ll >= 0x0FFAD46E && ll <= 0x0FFAD4A3);  // skip alloc walk
                if show {
                    output.push_str(&format!("{:>8}: {:08X} {:<18} {:?}  A={:08X} B={:08X} DI={:08X} FL={:04X}\n",
                        ii, ll, bytes.trim(), inst.op,
                        machine.cpu.eax, machine.cpu.ebx, machine.cpu.edi, machine.cpu.eflags as u16));
                }
                match machine.step() {
                    Ok(ExecResult::Continue) => {}
                    Ok(ExecResult::Halt) => { output.push_str("HALT\n"); break; }
                    _ => {}
                }
            }
            break;
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
