use kokoa86_core::Machine;
use kokoa86_dev::Serial8250;
use std::fs;

#[test]
fn diag_seabios_boot() {
    let bios_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../roms/bios.bin");
    let bios_data = match fs::read(bios_path) {
        Ok(d) => d,
        Err(_) => {
            eprintln!("Skipping: bios.bin not found at {}", bios_path);
            return;
        }
    };

    let mut machine = Machine::new(32 * 1024 * 1024); // 32MB RAM
    let serial = Serial8250::new_capture(0x3F8);
    machine.ports.register(Box::new(serial));
    machine.load_bios(bios_data);

    let report = kokoa86_core::diag::trace_boot(&mut machine, 50_000_000, 50);
    println!("{}", report);

    // Serial output from SeaBIOS
    println!("\n=== Serial Output ({} bytes) ===", machine.serial_output.len());
    if !machine.serial_output.is_empty() {
        let s = String::from_utf8_lossy(&machine.serial_output);
        println!("{}", s);
    } else {
        println!("(no serial output)");
    }

    println!("\n=== Halt analysis ===");
    let halt_addr = machine.cpu.eip;
    // Check the 50 bytes before halt for string data
    let str_addr = machine.cpu.get_reg32(0); // EAX often has format string ptr
    println!("EAX (possible string ptr): 0x{:08X}", str_addr);
    if str_addr > 0 && str_addr < 0x100000 {
        let mut s = Vec::new();
        for j in 0..80 {
            let b = machine.mem.read_u8(str_addr + j);
            if b == 0 { break; }
            s.push(b);
        }
        println!("String at EAX: {:?}", String::from_utf8_lossy(&s));
    }
    // Check EDX which was the 2nd param
    let edx = machine.cpu.get_reg32(2);
    println!("EDX (2nd param): 0x{:08X}", edx);
    // Check memory at what should be stack with fmt string
    // Check CMOS RAM registers directly
    use kokoa86_dev::port_bus::PortDevice;
    machine.cmos.port_out(0x70, 1, 0x34);
    println!("CMOS[0x34] = {:02X}", machine.cmos.port_in(0x71, 1));
    machine.cmos.port_out(0x70, 1, 0x35);
    println!("CMOS[0x35] = {:02X}", machine.cmos.port_in(0x71, 1));
    println!("mem[0xF6034] (serial debug port): {:08X}", machine.mem.read_u32(0xF6034));
    println!("mem[0x400] (BDA COM1): {:04X}", machine.mem.read_u16(0x400));
    println!("mem[0x6FD0] = {:08X}", machine.mem.read_u32(0x6FD0));
    println!("mem[0x6FCC] = {:08X}", machine.mem.read_u32(0x6FCC));
    println!("mem[0x6FD4] = {:08X}", machine.mem.read_u32(0x6FD4));
    println!("mem[0x6FD8] = {:08X}", machine.mem.read_u32(0x6FD8));

    // Dump stack (return addresses)
    println!("\nStack dump:");
    let sp = machine.cpu.esp;
    for j in 0..16 {
        let addr = sp + j * 4;
        let val = machine.mem.read_u32(addr);
        println!("  ESP+{:02X}: {:08X}", j*4, val);
    }
    if edx > 0 && edx < 0x100000 {
        let mut s = Vec::new();
        for j in 0..80 {
            let b = machine.mem.read_u8(edx + j);
            if b == 0 { break; }
            s.push(b);
        }
        println!("String at EDX: {:?}", String::from_utf8_lossy(&s));
    }

    // Verify ROM is readable at the expected address
    println!("\n=== ROM verification ===");
    println!("Byte at 0xD2AE0 AFTER execution: {:02X} (expect 0xD5)", machine.mem.read_u8(0xD2AE0));
    println!("Byte at 0xD2AE1: {:02X} (expect 0x06)", machine.mem.read_u8(0xD2AE1));
    println!("Byte at 0xFCFA9: {:02X}", machine.mem.read_u8(0xFCFA9));
    println!("Byte at 0xFFFF0: {:02X} (expect 0xEA)", machine.mem.read_u8(0xFFFF0));

    // Dump GDT entries
    use kokoa86_mem::MemoryAccess;
    let gdtr_base = machine.cpu.gdtr.base;
    let gdtr_limit = machine.cpu.gdtr.limit;
    println!("\n=== GDT at {:08X}, limit {} ===", gdtr_base, gdtr_limit);
    let num_entries = ((gdtr_limit + 1) / 8) as usize;
    for i in 0..num_entries.min(8) {
        let addr = gdtr_base + (i as u32 * 8);
        let mut raw = [0u8; 8];
        for j in 0..8 {
            raw[j] = machine.mem.read_u8(addr + j as u32);
        }
        let base = (raw[7] as u32) << 24 | (raw[4] as u32) << 16 | (raw[3] as u32) << 8 | raw[2] as u32;
        let limit = ((raw[6] & 0x0F) as u32) << 16 | (raw[1] as u32) << 8 | raw[0] as u32;
        let access = raw[5];
        let flags = raw[6] >> 4;
        println!("  Entry {}: base={:08X} limit={:05X} access={:02X} flags={:X} raw={:02X?}",
            i, base, limit, access, flags, raw);
    }
}
