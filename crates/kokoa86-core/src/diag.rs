use crate::Machine;
use kokoa86_cpu::execute::ExecResult;
use kokoa86_mem::MemoryAccess;

pub fn trace_boot(machine: &mut Machine, max_inst: u64, _trace_first: u64) -> String {
    let mut output = String::new();
    let mut last_serial_len = 0usize;

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

        // Check PCIDevices at inst 68000 (after pci_probe_devices should be done)
        if i == 200_000 {
            // Use relocation base from the "Relocating init" message
            // Search serial output for the relocation address
            let serial_str = String::from_utf8_lossy(&machine.serial_output);
            let reloc_base = if let Some(pos) = serial_str.find("to 0x") {
                let hex = &serial_str[pos+5..pos+13];
                u32::from_str_radix(hex, 16).unwrap_or(0)
            } else { 0 };
            output.push_str(&format!("  reloc_base from serial: 0x{:08X}\n", reloc_base));
            let delta = reloc_base.wrapping_sub(0x0D4C20);
            let reloc = 0x0E95E0u32.wrapping_add(delta);
            output.push_str(&format!(
                "\n=== PCIDevices @{} ===\n  orig [0x0E95E0] = {:08X}\n  relocated [0x{:08X}] = {:08X}\n",
                i, machine.mem.read_u32(0x0E95E0), reloc, machine.mem.read_u32(reloc)
            ));
            // Walk the list from relocated address
            let first = machine.mem.read_u32(reloc);
            if first != 0 {
                output.push_str("  List:\n");
                let mut node = first;
                for j in 0..10 {
                    if node == 0 { break; }
                    let next = machine.mem.read_u32(node);
                    // bdf is at node - 4 (hlist_node is at offset 4 in pci_device)
                    let bdf_m4 = machine.mem.read_u16(node.wrapping_sub(4));
                    let bdf_m8 = machine.mem.read_u16(node.wrapping_sub(8));
                    let bdf_p8 = machine.mem.read_u16(node.wrapping_add(8));
                    let bdf = bdf_m4;
                    output.push_str(&format!("    [{}] node={:08X} next={:08X} bdf(-4)={:04X} bdf(-8)={:04X} bdf(+8)={:04X}\n",
                        j, node, next, bdf_m4, bdf_m8, bdf_p8));
                    if next == node { output.push_str("    CIRCULAR!\n"); break; }
                    node = next;
                }
            } else {
                output.push_str("  (empty list)\n");
            }
        }

        if i > 500_000 { break; }

        match machine.step() {
            Ok(ExecResult::Continue) => {}
            Ok(ExecResult::Halt) => { output.push_str(&format!("\nHALT at {:08X}\n", lip)); break; }
            Ok(ExecResult::UnknownOpcode(b)) => { output.push_str(&format!("\nUNKNOWN 0x{:02X}\n", b)); break; }
            _ => {}
        }
    }

    output.push_str(&format!("\nSerial: {} bytes\n", machine.serial_output.len()));
    output
}
