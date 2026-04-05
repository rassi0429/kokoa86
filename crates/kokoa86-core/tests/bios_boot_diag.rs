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

    let mut machine = Machine::new(1024 * 1024); // 1MB RAM
    machine.ports.register(Box::new(Serial8250::new_capture(0x3F8)));
    machine.load_bios(bios_data);

    let report = kokoa86_core::diag::trace_boot(&mut machine, 2_000_000, 50);
    println!("{}", report);

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
