use crate::Machine;
use kokoa86_cpu::execute::ExecResult;
use kokoa86_mem::MemoryAccess;

pub fn trace_boot(machine: &mut Machine, max_inst: u64, _trace_first: u64) -> String {
    let mut output = String::new();
    let mut last_serial_len = 0usize;

    // Monitor PCI config address writes via port 0xCF8
    let mut last_pci_cfg = 0u32;
    let mut pci_vendor_reads = 0u32;

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

        // Detect OUT to 0xCF8 (PCI config address) via OutDxAx/OutDxAl pattern
        // Simpler: just check PCI state after each step
        let pre_pci_cfg = machine.pci.config_address;

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

        // Detect PCI config address change
        let post_pci_cfg = machine.pci.config_address;
        if post_pci_cfg != pre_pci_cfg && (post_pci_cfg & 0x80000000) != 0 {
            let reg = post_pci_cfg & 0xFC;
            let bdf = (post_pci_cfg >> 8) & 0xFFFF;
            let bus = (bdf >> 8) & 0xFF;
            let dev = (bdf >> 3) & 0x1F;
            let func = bdf & 0x07;

            if reg == 0 && i > 67000 { // vendor reads after probing message
                pci_vendor_reads += 1;
                if pci_vendor_reads <= 100 {
                    output.push_str(&format!(
                        "[pci_vendor #{:>3} @{:>8}] bdf={:04X} bus={} dev={} fn={}\n",
                        pci_vendor_reads, i, bdf, bus, dev, func
                    ));
                }
                if pci_vendor_reads == 100 {
                    output.push_str("[... stopping at 100 vendor reads]\n");
                    break;
                }
            }
        }
    }

    output.push_str(&format!("\nSerial: {} bytes, vendor_reads: {}\n",
        machine.serial_output.len(), pci_vendor_reads));
    output
}
