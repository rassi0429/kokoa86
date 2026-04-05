use crate::Machine;
use kokoa86_cpu::execute::ExecResult;
use kokoa86_mem::MemoryAccess;

pub fn trace_boot(machine: &mut Machine, max_inst: u64, _trace_first: u64) -> String {
    let mut output = String::new();
    let mut last_serial_len = 0usize;
    let mut sample_count = 0u32;

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

        // Sample IP every 10M instructions
        // Count alloc_new calls
        if lip == 0x3FFAD462 {
            sample_count += 1;
            if sample_count <= 10 {
                let ret = machine.mem.read_u32(machine.cpu.esp);
                output.push_str(&format!("[alloc #{} ret={:08X} size={}]\n", sample_count, ret, machine.cpu.edx));
            }
            if sample_count == 100 {
                output.push_str(&format!("[100 allocs reached at inst {}]\n", i));
                break;
            }
        }
        if i == 100_000 || i == 1_000_000 {
            // Count free list nodes in ZoneTmpHigh
            // ZoneTmpHigh head is at a known address. Find it.
            // The alloc function reads [EAX] as the zone's first free block.
            // At alloc entry, EAX = zone pointer. Let's read it.
            // Actually, just count the list from whatever EAX points to now.
            let mut ptr = machine.mem.read_u32(machine.cpu.eax);
            let mut count = 0u32;
            let mut last = 0u32;
            while ptr != 0 && count < 100000 {
                last = ptr;
                ptr = machine.mem.read_u32(ptr);
                count += 1;
            }
            output.push_str(&format!(
                "[zone list] head_from_EAX={:08X} nodes={} last_node={:08X}\n",
                machine.cpu.eax, count, last
            ));
        }
        if i % 10_000_000 == 0 && i > 0 {
            sample_count += 1;
            let max_pci_bus = machine.mem.read_u32(0x0F5D0C);
            output.push_str(&format!(
                "[sample {:>3} @{:>10}] IP={:08X} serial={} MaxPCIBus?[0F5D0C]={:08X}\n",
                sample_count, i, lip, machine.serial_output.len(), max_pci_bus
            ));
            if sample_count > 20 { break; }
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

    output.push_str(&format!("\nSerial: {} bytes\n", machine.serial_output.len()));
    output
}
