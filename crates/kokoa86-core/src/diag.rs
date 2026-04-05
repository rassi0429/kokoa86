use crate::Machine;
use kokoa86_cpu::execute::ExecResult;
use kokoa86_mem::MemoryAccess;

pub fn trace_boot(machine: &mut Machine, max_inst: u64, _trace_first: u64) -> String {
    let mut output = String::new();
    let mut last_serial_len = 0usize;

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

        // Dump zone info once
        if i == 1_000_000 {
            output.push_str("\n=== Zone info @100K ===\n");
            // ZoneTmpHigh head pointer
            // ZoneTmpHigh pointer (relocated)
            // reloc_base = 0x3FFAB420 for 1GB RAM
            // zone_ptr original ≈ 0x0E???? + delta
            let zone_ptr_addr = 0x3FFBFE78u32; // 1GB version
            let zone_head = machine.mem.read_u32(zone_ptr_addr);
            // Check MaxPCIBus and extraroots
            let maxpci_reloc = 0x3FFCC50Cu32;
            output.push_str(&format!("MaxPCIBus [0x{:08X}] = {:08X}\n", maxpci_reloc, machine.mem.read_u32(maxpci_reloc)));
            output.push_str(&format!("ZoneTmpHigh ptr addr: 0x{:08X}\n", zone_ptr_addr));
            output.push_str(&format!("ZoneTmpHigh head: 0x{:08X}\n", zone_head));
            if zone_head != 0 {
                // Walk first 5 nodes
                let mut node = zone_head;
                let mut total_nodes = 0u32;
                let mut count_node = zone_head;
                while count_node != 0 && total_nodes < 1000000 {
                    count_node = machine.mem.read_u32(count_node);
                    total_nodes += 1;
                }
                output.push_str(&format!("Total nodes: {} (terminated: {})\n", total_nodes, count_node == 0));
                for j in 0..100000 {
                    if node == 0 { break; }
                    let next = machine.mem.read_u32(node);
                    let prev = machine.mem.read_u32(node + 4);
                    let base = machine.mem.read_u32(node + 8);
                    let size_end = machine.mem.read_u32(node + 0x0C);
                    let block_size = machine.mem.read_u32(node + 0x10);
                    if j < 5 || (j >= total_nodes - 5 && j < total_nodes) || j % 100000 == 0 {
                        output.push_str(&format!(
                            "  [{:>6}] node={:08X} next={:08X} [+08]={:08X} [+0C]={:08X} [+10]={:08X}\n",
                            j, node, next, base, size_end, block_size
                        ));
                    }
                    node = next;
                }
            }
        }
        // Detect while loop condition check
        let lip_check = machine.cpu.cs_ip();
        if lip_check == 0x3FFAFE91 {
            output.push_str(&format!(
                "[CMP_A @{:>8}] EBX={:08X} [MaxPCI]={:08X}\n",
                i, machine.cpu.ebx, machine.mem.read_u32(0x3FFCC50C)
            ));
        }
        if lip_check == 0x3FFAFF0C {
            output.push_str(&format!(
                "[CMP_B @{:>8}] EBX={:08X} [MaxPCI]={:08X}\n",
                i, machine.cpu.ebx, machine.mem.read_u32(0x3FFCC50C)
            ));
        }
        // Sample every 50M instructions
        if i % 50_000_000 == 0 && i > 0 {
            let lip = machine.cpu.cs_ip();
            output.push_str(&format!("[sample @{:>10}] IP={:08X} serial={}\n", i, lip, machine.serial_output.len()));
        }

        match machine.step() {
            Ok(ExecResult::Continue) => {}
            Ok(ExecResult::Halt) => {
                output.push_str(&format!("\nHALT at {:08X} after {} inst\n", machine.cpu.cs_ip(), i));
                break;
            }
            Ok(ExecResult::UnknownOpcode(b)) => {
                output.push_str(&format!("\nUNKNOWN 0x{:02X} at {:08X}\n", b, machine.cpu.cs_ip()));
                break;
            }
            _ => {}
        }
    }

    output.push_str(&format!("\nSerial: {} bytes\n", machine.serial_output.len()));
    output
}
