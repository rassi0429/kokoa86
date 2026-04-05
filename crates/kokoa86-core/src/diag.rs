use crate::Machine;
use kokoa86_cpu::decode;
use kokoa86_cpu::execute::ExecResult;
use kokoa86_mem::MemoryAccess;

pub fn trace_boot(machine: &mut Machine, max_inst: u64, _trace_first: u64) -> String {
    let mut output = String::new();
    let mut last_serial_len = 0usize;
    let mut vendor_count = 0u32;
    let mut tracing = false;

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

        // Trace every instruction between vendor read #34 and #35
        if tracing {
            let inst = decode::decode_at_addr(&machine.cpu, &machine.mem, lip);
            let mut bytes = String::new();
            for k in 0..inst.len.min(7) as u32 {
                bytes.push_str(&format!("{:02X} ", machine.mem.read_u8(lip + k)));
            }
            output.push_str(&format!(
                "{:>8}: {:08X} {:<21} {:?}  A={:08X} B={:08X} C={:08X} SP={:08X}\n",
                i, lip, bytes.trim(), inst.op,
                machine.cpu.eax, machine.cpu.ebx, machine.cpu.ecx, machine.cpu.esp
            ));
        }

        let pre_cfg = machine.pci.config_address;

        match machine.step() {
            Ok(ExecResult::Continue) => {}
            Ok(ExecResult::Halt) => { output.push_str(&format!("\nHALT\n")); break; }
            Ok(ExecResult::UnknownOpcode(b)) => { output.push_str(&format!("\nUNKNOWN 0x{:02X}\n", b)); break; }
            _ => {}
        }

        // Detect vendor read
        let post_cfg = machine.pci.config_address;
        if post_cfg != pre_cfg && (post_cfg & 0x80000000) != 0 && (post_cfg & 0xFC) == 0 {
            vendor_count += 1;
            if vendor_count == 34 {
                let bdf = (post_cfg >> 8) & 0xFFFF;
                output.push_str(&format!("\n>>> VENDOR #{} bdf={:04X} at inst {} — START TRACING\n\n", vendor_count, bdf, i));
                tracing = true;
            }
            if vendor_count == 35 {
                let bdf = (post_cfg >> 8) & 0xFFFF;
                output.push_str(&format!("\n>>> VENDOR #{} bdf={:04X} at inst {} — STOP\n", vendor_count, bdf, i));
                break;
            }
        }

        if i > 500_000 { break; }
    }

    output.push_str(&format!("\nSerial: {} bytes\n", machine.serial_output.len()));
    output
}
