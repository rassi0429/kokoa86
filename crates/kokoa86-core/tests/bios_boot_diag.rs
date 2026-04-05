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

    let mut machine = Machine::new(2u64 as usize * 1024 * 1024 * 1024); // 2GB RAM
    let serial = Serial8250::new_capture(0x3F8);
    machine.ports.register(Box::new(serial));
    machine.load_bios(bios_data);

    let report = kokoa86_core::diag::trace_boot(&mut machine, 2_000_000_000, 50);
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
    // Test REP INSB from fw_cfg port
    {
        let mut m2 = Machine::new(128 * 1024 * 1024);
        m2.bios_stubs = false;
        // 32-bit flat code segment (simulate PM)
        m2.cpu.mode = kokoa86_cpu::CpuMode::ProtectedMode;
        m2.cpu.cr0 = 1;
        m2.cpu.cs = 0x08;
        m2.cpu.cs_cache = kokoa86_cpu::SegmentCache { selector: 0x08, base: 0, limit: 0xFFFFFFFF, access: 0x9B, flags: 0xC, dpl: 0, big: true, present: true };
        m2.cpu.ds_cache = kokoa86_cpu::SegmentCache { selector: 0x10, base: 0, limit: 0xFFFFFFFF, access: 0x93, flags: 0xC, dpl: 0, big: true, present: true };
        m2.cpu.es_cache = m2.cpu.ds_cache;
        m2.cpu.ss_cache = m2.cpu.ds_cache;
        m2.cpu.ds = 0x10; m2.cpu.es = 0x10; m2.cpu.ss = 0x10;
        m2.cpu.esp = 0x7000;

        // Code: select key 0x0019 via OUT DX, AX; then REP INSB to read file dir
        // MOV DX, 0x510; MOV AX, 0x0019; OUT DX, AX
        // MOV DX, 0x511; MOV ECX, 200; MOV EDI, 0x2000; REP INSB
        // HLT
        m2.load_at(0x7C00, &[
            0xBA, 0x10, 0x05, 0x00, 0x00, // MOV EDX, 0x00000510
            0x66, 0xB8, 0x19, 0x00,       // MOV AX, 0x0019 (16-bit with 0x66 prefix)
            0x66, 0xEF,                   // OUT DX, AX (16-bit)
            0xBA, 0x11, 0x05, 0x00, 0x00, // MOV EDX, 0x00000511
            0xB9, 0xC8, 0x00, 0x00, 0x00, // MOV ECX, 200
            0xBF, 0x00, 0x20, 0x00, 0x00, // MOV EDI, 0x2000
            0xF3, 0x6C,                   // REP INSB
            0xF4,                         // HLT
        ]);
        m2.cpu.eip = 0x7C00;
        m2.run().unwrap();

        // Direct fw_cfg read test (bypass CPU)
        {
            use kokoa86_dev::port_bus::PortDevice;
            let mut fw = kokoa86_dev::FwCfg::new(128 * 1024 * 1024);
            fw.port_out(0x510, 2, 0x0019);
            let b0 = fw.port_in(0x511, 1);
            println!("Direct fw_cfg read after SELECT: first byte = 0x{:02X}", b0);
        }

        // Check CPU state after INSB
        println!("After INSB: EDI=0x{:08X}, ECX=0x{:08X}", m2.cpu.edi, m2.cpu.ecx);
        println!("mem[0x2000] = 0x{:02X}", m2.mem.read_u8(0x2000));
        println!("mem[0x2001] = 0x{:02X}", m2.mem.read_u8(0x2001));

        // Check file count at 0x2000 (4 bytes BE)
        let count_be = m2.mem.read_u32(0x2000);
        let count = count_be.swap_bytes(); // BE to LE
        println!("REP INSB fw_cfg test: file count = {} (raw BE: {:08X})", count, count_be);
        // Check first file name at 0x2000 + 4 + 4 + 2 + 2 = 0x200C
        let mut name = [0u8; 20];
        m2.mem.read_bytes(0x200C, &mut name);
        println!("  First file name: {:?}", String::from_utf8_lossy(&name));
    }

    // Quick test: SHL with 0x66 prefix in 16-bit code
    {
        let mut m2 = Machine::new(64 * 1024);
        m2.bios_stubs = false;
        m2.load_at(0x7C00, &[
            0x66, 0xB8, 0x07, 0x00, 0x00, 0x00, // MOV EAX, 7
            0x66, 0xC1, 0xE0, 0x18,              // SHL EAX, 24
            0xF4,
        ]);
        m2.cpu.eip = 0x7C00;
        m2.run().unwrap();
        println!("SHL test: EAX=0x{:08X} (expect 0x07000000)", m2.cpu.eax);
    }

    // Check RamSize global variable at 0x0E9718 (found from ROM analysis)
    println!("\n=== RamSize check ===");
    println!("RamSize @ 0x0E9718 (orig) = 0x{:08X}", machine.mem.read_u32(0x0E9718));
    println!("RamSize @ 0x0FFBFF18 (reloc) = 0x{:08X}", machine.mem.read_u32(0x0FFBFF18));

    // Verify fw_cfg file directory
    {
        use kokoa86_dev::port_bus::PortDevice;
        // Re-create a fw_cfg instance for inspection
        let mut fw = kokoa86_dev::FwCfg::new(128 * 1024 * 1024);
        // Select file directory (key 0x0019)
        fw.port_out(0x510, 2, 0x0019);
        // Read file count (4 bytes BE)
        let b0 = fw.port_in(0x511, 1) as u8;
        let b1 = fw.port_in(0x511, 1) as u8;
        let b2 = fw.port_in(0x511, 1) as u8;
        let b3 = fw.port_in(0x511, 1) as u8;
        let count = ((b0 as u32) << 24) | ((b1 as u32) << 16) | ((b2 as u32) << 8) | (b3 as u32);
        println!("fw_cfg file count: {}", count);
        for i in 0..count {
            // Each entry: u32 size (BE), u16 select (BE), u16 reserved, [56] name
            let s0 = fw.port_in(0x511, 1) as u8;
            let s1 = fw.port_in(0x511, 1) as u8;
            let s2 = fw.port_in(0x511, 1) as u8;
            let s3 = fw.port_in(0x511, 1) as u8;
            let size = ((s0 as u32) << 24) | ((s1 as u32) << 16) | ((s2 as u32) << 8) | (s3 as u32);
            let k0 = fw.port_in(0x511, 1) as u8;
            let k1 = fw.port_in(0x511, 1) as u8;
            let key = ((k0 as u16) << 8) | (k1 as u16);
            let _r0 = fw.port_in(0x511, 1);
            let _r1 = fw.port_in(0x511, 1);
            let mut name = [0u8; 56];
            for j in 0..56 {
                name[j] = fw.port_in(0x511, 1) as u8;
            }
            let name_str = std::str::from_utf8(&name).unwrap_or("?").trim_end_matches('\0');
            println!("  File {}: name='{}' key=0x{:04X} size={}", i, name_str, key, size);
        }
    }

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
