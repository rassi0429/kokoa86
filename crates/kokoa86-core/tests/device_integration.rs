//! Integration tests: CPU + devices working together

use kokoa86_core::Machine;
use kokoa86_dev::Serial8250;
use kokoa86_mem::MemoryAccess;

fn make_machine() -> Machine {
    let mut m = Machine::new(1024 * 1024);
    m.bios_stubs = false;
    m.ports.register(Box::new(Serial8250::new(0x3F8)));
    m
}

/// Test: Write directly to VGA buffer at 0xB8000 via ES segment
#[test]
fn test_vga_direct_write() {
    let mut m = make_machine();

    // MOV AX, 0xB800; MOV ES, AX; XOR DI, DI
    // MOV AL, 'H'; STOSB; MOV AL, 0x0F; STOSB
    // MOV AL, 'i'; STOSB; MOV AL, 0x0F; STOSB
    // HLT
    let code: &[u8] = &[
        0xB8, 0x00, 0xB8,     // MOV AX, 0xB800
        0x8E, 0xC0,           // MOV ES, AX
        0x31, 0xFF,           // XOR DI, DI
        0xB0, b'H', 0xAA,    // MOV AL, 'H'; STOSB
        0xB0, 0x0F, 0xAA,    // MOV AL, 0x0F; STOSB
        0xB0, b'i', 0xAA,    // MOV AL, 'i'; STOSB
        0xB0, 0x0F, 0xAA,    // MOV AL, 0x0F; STOSB
        0xF4,                 // HLT
    ];

    m.load_at(0x7C00, code);
    m.cpu.eip = 0x7C00;
    m.cpu.esp = 0xFFFE;
    m.run().unwrap();

    // Sync VGA from RAM
    m.sync_vga_from_ram();

    // Verify VGA buffer
    assert_eq!(m.vga.buffer[0], b'H');
    assert_eq!(m.vga.buffer[1], 0x0F);
    assert_eq!(m.vga.buffer[2], b'i');
    assert_eq!(m.vga.buffer[3], 0x0F);
}

/// Test: CMOS memory size reading via IN/OUT
#[test]
fn test_cmos_read_via_cpu() {
    let mut m = make_machine();

    // OUT 0x70, 0x15  (select base memory low)
    // IN AL, 0x71     (read value)
    // MOV BL, AL      (save it)
    // OUT 0x70, 0x16  (select base memory high)
    // IN AL, 0x71
    // MOV BH, AL
    // HLT
    let code: &[u8] = &[
        0xB0, 0x15,       // MOV AL, 0x15
        0xE6, 0x70,       // OUT 0x70, AL
        0xE4, 0x71,       // IN AL, 0x71
        0x88, 0xC3,       // MOV BL, AL  (0x88 = MOV r/m8,r8; C3 = mod=11, reg=AL(0), rm=BL(3))
        0xB0, 0x16,       // MOV AL, 0x16
        0xE6, 0x70,       // OUT 0x70, AL
        0xE4, 0x71,       // IN AL, 0x71
        0x88, 0xC7,       // MOV BH, AL  (C7 = mod=11, reg=AL(0), rm=BH(7))
        0xF4,             // HLT
    ];

    m.load_at(0x7C00, code);
    m.cpu.eip = 0x7C00;
    m.cpu.esp = 0xFFFE;
    m.run().unwrap();

    // 1MB RAM -> base memory = 640 KB = 0x0280
    assert_eq!(m.cpu.get_reg16(3), 640); // BX = 640
}

/// Test: Protected mode entry (LGDT + MOV CR0 + JMP FAR)
#[test]
fn test_protected_mode_transition() {
    let mut m = Machine::new(1024 * 1024);
    m.bios_stubs = false;
    m.ports.register(Box::new(Serial8250::new(0x3F8)));

    // GDT at 0x1000:
    //   Entry 0 (0x1000): Null descriptor (8 bytes of 0)
    //   Entry 1 (0x1008): Code segment - base=0, limit=0xFFFFF, G=1, D=0 (16-bit), P=1
    //   Entry 2 (0x1010): Data segment - base=0, limit=0xFFFFF, G=1, D=0, P=1, writable

    // Null descriptor
    m.mem.load(0x1000, &[0; 8]);

    // Code segment: base=0, limit=0xFFFFF, G=1, D=0 (16-bit code), P=1, DPL=0, code+read
    m.mem.load(0x1008, &[0xFF, 0xFF, 0x00, 0x00, 0x00, 0x9A, 0x8F, 0x00]);
    // flags_limit_hi = 0x8F: G=1, D=0, L=0, AVL=0, limit_hi=0xF

    // Data segment: base=0, limit=0xFFFFF, G=1, D=0, P=1, DPL=0, data+writable
    m.mem.load(0x1010, &[0xFF, 0xFF, 0x00, 0x00, 0x00, 0x92, 0x8F, 0x00]);

    // GDT pointer at 0x1100: limit=0x17 (3*8-1), base=0x1000
    m.mem.write_u16(0x1100, 0x0017);
    m.mem.write_u32(0x1102, 0x00001000);

    // Code at 0x7C00:
    // LGDT [0x1100]
    // MOV EAX, CR0
    // OR AL, 1
    // MOV CR0, EAX
    // JMP FAR 0x08:pm_entry
    // pm_entry: HLT
    let mut code: Vec<u8> = Vec::new();
    // LGDT [0x1100]: 0F 01 16 00 11  (5 bytes)
    code.extend_from_slice(&[0x0F, 0x01, 0x16, 0x00, 0x11]);
    // MOV EAX, CR0: 0F 20 C0  (3 bytes)
    code.extend_from_slice(&[0x0F, 0x20, 0xC0]);
    // OR AL, 1: 0C 01  (2 bytes)
    code.extend_from_slice(&[0x0C, 0x01]);
    // MOV CR0, EAX: 0F 22 C0  (3 bytes)
    code.extend_from_slice(&[0x0F, 0x22, 0xC0]);
    // JMP FAR 0x0008:pm_entry: EA xx xx 08 00  (5 bytes)
    // pm_entry is at 0x7C00 + 5+3+2+3+5 = 0x7C00 + 18 = 0x7C12
    let pm_entry_offset: u16 = 0x7C00 + 5 + 3 + 2 + 3 + 5;
    code.push(0xEA);
    code.extend_from_slice(&pm_entry_offset.to_le_bytes());
    code.extend_from_slice(&[0x08, 0x00]);
    // pm_entry: HLT
    code.push(0xF4);

    m.load_at(0x7C00, &code);
    m.cpu.eip = 0x7C00;
    m.cpu.esp = 0xFFFE;
    m.run().unwrap();

    // Verify we're in protected mode
    assert_eq!(m.cpu.mode, kokoa86_cpu::CpuMode::ProtectedMode);
    assert_eq!(m.cpu.cr0 & 1, 1);
    assert_eq!(m.cpu.cs, 0x08);
    assert!(m.cpu.halted);
}

/// Test: CPUID instruction
#[test]
fn test_cpuid() {
    let mut m = make_machine();

    // MOV EAX, 0; CPUID; HLT
    let code: &[u8] = &[
        0x66, 0xB8, 0x00, 0x00, 0x00, 0x00, // MOV EAX, 0
        0x0F, 0xA2,                           // CPUID
        0xF4,
    ];

    m.load_at(0x7C00, code);
    m.cpu.eip = 0x7C00;
    m.cpu.esp = 0xFFFE;
    m.run().unwrap();

    // EAX should be max supported leaf (at least 1)
    assert!(m.cpu.eax >= 1);
}

/// Test: 32-bit operations with 0x66 prefix
#[test]
fn test_32bit_alu_integration() {
    let mut m = make_machine();

    // MOV EAX, 0x12345678; MOV EBX, 0x11111111; ADD EAX, EBX; HLT
    let code: &[u8] = &[
        0x66, 0xB8, 0x78, 0x56, 0x34, 0x12, // MOV EAX, 0x12345678
        0x66, 0xBB, 0x11, 0x11, 0x11, 0x11, // MOV EBX, 0x11111111
        0x66, 0x01, 0xD8,                     // ADD EAX, EBX
        0xF4,
    ];

    m.load_at(0x7C00, code);
    m.cpu.eip = 0x7C00;
    m.cpu.esp = 0xFFFE;
    m.run().unwrap();

    assert_eq!(m.cpu.eax, 0x23456789);
}

/// Test: PIC initialization sequence
#[test]
fn test_pic_init_via_cpu() {
    let mut m = make_machine();

    // Standard PIC initialization sequence:
    // ICW1: OUT 0x20, 0x11
    // ICW2: OUT 0x21, 0x20 (vector offset 0x20)
    // ICW3: OUT 0x21, 0x04 (slave on IRQ2)
    // ICW4: OUT 0x21, 0x01 (8086 mode)
    // OCW1: OUT 0x21, 0xFB (mask all except IRQ2)
    // HLT
    let code: &[u8] = &[
        0xB0, 0x11, 0xE6, 0x20, // MOV AL, 0x11; OUT 0x20, AL
        0xB0, 0x20, 0xE6, 0x21, // MOV AL, 0x20; OUT 0x21, AL
        0xB0, 0x04, 0xE6, 0x21, // MOV AL, 0x04; OUT 0x21, AL
        0xB0, 0x01, 0xE6, 0x21, // MOV AL, 0x01; OUT 0x21, AL
        0xB0, 0xFB, 0xE6, 0x21, // MOV AL, 0xFB; OUT 0x21, AL
        0xF4,                     // HLT
    ];

    m.load_at(0x7C00, code);
    m.cpu.eip = 0x7C00;
    m.cpu.esp = 0xFFFE;
    m.run().unwrap();

    // Verify PIC was initialized correctly
    assert_eq!(m.pic_master.vector_offset, 0x20);
}

/// Test: REP MOVSB (block memory copy)
#[test]
fn test_rep_movsb_integration() {
    let mut m = make_machine();

    // Put "Hello" at 0x200
    m.mem.load(0x200, b"Hello");

    // MOV SI, 0x200; MOV DI, 0x300; MOV CX, 5; REP MOVSB; HLT
    let code: &[u8] = &[
        0xBE, 0x00, 0x02, // MOV SI, 0x200
        0xBF, 0x00, 0x03, // MOV DI, 0x300
        0xB9, 0x05, 0x00, // MOV CX, 5
        0xF3, 0xA4,       // REP MOVSB
        0xF4,
    ];

    m.load_at(0x7C00, code);
    m.cpu.eip = 0x7C00;
    m.cpu.esp = 0xFFFE;
    m.run().unwrap();

    // Verify copy
    let mut buf = [0u8; 5];
    m.mem.read_bytes(0x300, &mut buf);
    assert_eq!(&buf, b"Hello");
}
