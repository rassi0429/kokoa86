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

        // Dump PCIDevices list after pci_probe_devices completes (~inst 66000)
        // Track malloc_tmp results
        if lip == 0x3FFAD462 { // alloc_new entry
            let ret = machine.mem.read_u32(machine.cpu.esp);
            output.push_str(&format!("[alloc_new @{:>8}] size={} ret={:08X}\n", i, machine.cpu.edx, ret));
        }
        if i == 65500 {
            output.push_str("\n=== PCIDevices list at inst 66000 ===\n");
            // Scan a range around 0x0E95xx for non-zero values
            output.push_str(&format!("PCIDevices.first [0E95E0] = {:08X}\n", machine.mem.read_u32(0x0E95E0)));
            for off in (0..0x100).step_by(4) {
                let addr = 0x0E9500u32 + off;
                let val = machine.mem.read_u32(addr);
                if val != 0 {
                    output.push_str(&format!("  [{:06X}] = {:08X}\n", addr, val));
                }
            }
            // Try several candidate addresses for PCIDevices.first
            for &addr in &[0x0E95E0u32] {
                let first = machine.mem.read_u32(addr);
                if first != 0 {
                    output.push_str(&format!("PCIDevices candidate [{:06X}].first = {:08X}\n", addr, first));
                    // Walk the hlist
                    let mut node = first;
                    for j in 0..10 {
                        if node == 0 { break; }
                        // hlist_node: { next: u32, pprev: u32 }
                        let next = machine.mem.read_u32(node);
                        let pprev = machine.mem.read_u32(node + 4);
                        // pci_device: bdf is at offset -4 from node (node is at offset 4 in struct)
                        // struct pci_device { u16 bdf; u8 rootbus; hlist_node node; ... }
                        // node is at offset 4 (after bdf u16 + rootbus u8 + padding)
                        let bdf = machine.mem.read_u16(node - 4);
                        let vendor = machine.mem.read_u16(node + 8); // after node (8 bytes)
                        output.push_str(&format!(
                            "  [{:>2}] node={:08X} next={:08X} pprev={:08X} bdf={:04X} vendor={:04X}\n",
                            j, node, next, pprev, bdf, vendor
                        ));
                        if next == node { output.push_str("  CIRCULAR!\n"); break; }
                        node = next;
                    }
                }
            }
        }

        match machine.step() {
            Ok(ExecResult::Continue) => {}
            Ok(ExecResult::Halt) => {
                output.push_str(&format!("\nHALT at {:08X}\n", lip));
                break;
            }
            Ok(ExecResult::UnknownOpcode(b)) => {
                output.push_str(&format!("\nUNKNOWN 0x{:02X} at {:08X}\n", b, lip));
                break;
            }
            _ => {}
        }

        if i > 100_000 { break; }
    }

    output.push_str(&format!("\nSerial: {} bytes\n", machine.serial_output.len()));
    output
}
