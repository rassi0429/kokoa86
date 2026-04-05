//! Device integration tests: exercise the full Machine (CPU + devices together)

use kokoa86_core::Machine;
use kokoa86_dev::Serial8250;
use kokoa86_mem::MemoryAccess;

/// Helper: run machine with a max instruction count to avoid infinite loops.
fn run_limited(machine: &mut Machine, max_instructions: u64) -> Result<(), String> {
    for _ in 0..max_instructions {
        match machine.step() {
            Ok(kokoa86_cpu::execute::ExecResult::Continue) => {}
            Ok(kokoa86_cpu::execute::ExecResult::Halt) => return Ok(()),
            Ok(kokoa86_cpu::execute::ExecResult::DivideError) => {
                return Err(format!(
                    "Divide error at {:04X}:{:04X}",
                    machine.cpu.cs, machine.cpu.eip
                ));
            }
            Ok(kokoa86_cpu::execute::ExecResult::UnknownOpcode(b)) => {
                return Err(format!(
                    "Unknown opcode 0x{:02X} at {:04X}:{:04X} after {} instructions",
                    b, machine.cpu.cs, machine.cpu.eip, machine.instruction_count
                ));
            }
            Err(e) => return Err(format!("Error: {}", e)),
        }
    }
    Err(format!(
        "Did not halt within {} instructions",
        max_instructions
    ))
}

/// Helper: initialize master PIC with vector offset 0x20, all IRQs unmasked.
fn init_pic_master(machine: &mut Machine) {
    use kokoa86_dev::port_bus::PortDevice;
    // ICW1
    machine.pic_master.port_out(0x20, 1, 0x11);
    // ICW2: vector offset 0x20
    machine.pic_master.port_out(0x21, 1, 0x20);
    // ICW3: slave on IRQ2
    machine.pic_master.port_out(0x21, 1, 0x04);
    // ICW4: 8086 mode
    machine.pic_master.port_out(0x21, 1, 0x01);
    // Unmask all
    machine.pic_master.port_out(0x21, 1, 0x00);
}

// =============================================================================
// Test 1: PIC + PIT timer interrupt
// =============================================================================

/// Set up IVT entry for IRQ0, program PIT channel 0 with a short counter,
/// run machine, verify the interrupt handler was called (handler writes a
/// flag to memory).
#[test]
fn test_pic_timer_interrupt() {
    let mut machine = Machine::new(1024 * 1024);
    machine.bios_stubs = false;
    machine.ports.register(Box::new(Serial8250::new_capture(0x3F8)));

    // Initialize PIC: IRQ0 -> INT 0x20
    init_pic_master(&mut machine);

    // Set up IVT entry for INT 0x20 at address 0x20*4 = 0x80
    // Handler at 0x0000:0x8000
    machine.mem.write_u16(0x80, 0x8000); // offset
    machine.mem.write_u16(0x82, 0x0000); // segment

    // ISR at 0x8000:
    //   MOV BYTE [0x9000], 0x42    ; write flag to memory
    //   MOV AL, 0x20               ; EOI value
    //   OUT 0x20, AL               ; send EOI to PIC
    //   IRET
    let isr: &[u8] = &[
        0xC6, 0x06, 0x00, 0x90, 0x42, // MOV BYTE [0x9000], 0x42
        0xB0, 0x20,                     // MOV AL, 0x20
        0xE6, 0x20,                     // OUT 0x20, AL
        0xCF,                           // IRET
    ];
    machine.load_at(0x8000, isr);

    // Main program at 0x7C00:
    //   Program PIT channel 0 with counter=2 (fires almost immediately)
    //   STI, then NOP sled, then HLT
    let mut code: Vec<u8> = Vec::new();
    code.extend_from_slice(&[
        0xB0, 0x34, // MOV AL, 0x34  (ch0, lo/hi, mode 2)
        0xE6, 0x43, // OUT 0x43, AL
        0xB0, 0x02, // MOV AL, 0x02  (counter low = 2)
        0xE6, 0x40, // OUT 0x40, AL
        0xB0, 0x00, // MOV AL, 0x00  (counter high = 0)
        0xE6, 0x40, // OUT 0x40, AL
        0xFB,       // STI
    ]);
    // NOP sled so PIT has time to tick and fire
    for _ in 0..200 {
        code.push(0x90); // NOP
    }
    code.push(0xF4); // HLT

    machine.load_at(0x7C00, &code);
    machine.cpu.eip = 0x7C00;
    machine.cpu.esp = 0xFFFE;
    machine.cpu.eflags |= 0x0200; // IF

    let result = run_limited(&mut machine, 100_000);
    assert!(result.is_ok(), "Machine did not halt: {:?}", result);

    // Verify the ISR wrote 0x42 to address 0x9000
    let flag = machine.mem.read_u8(0x9000);
    assert_eq!(flag, 0x42, "Timer ISR did not write the flag byte");
}

// =============================================================================
// Test 2: PS/2 keyboard scancode via IRQ1
// =============================================================================

/// Send a scancode to PS/2, set up IVT for IRQ1, run, verify handler reads
/// the scancode from port 0x60 and stores it in memory.
#[test]
fn test_ps2_keyboard_scancode() {
    let mut machine = Machine::new(1024 * 1024);
    machine.bios_stubs = false;
    machine.ports.register(Box::new(Serial8250::new_capture(0x3F8)));

    // Initialize PIC
    init_pic_master(&mut machine);

    // IVT entry for INT 0x21 (IRQ1) at 0x21*4 = 0x84
    machine.mem.write_u16(0x84, 0x8100); // offset
    machine.mem.write_u16(0x86, 0x0000); // segment

    // ISR at 0x8100:
    //   IN AL, 0x60        ; read scancode
    //   MOV [0x9000], AL   ; store it
    //   MOV AL, 0x20       ; EOI
    //   OUT 0x20, AL
    //   IRET
    let isr: &[u8] = &[
        0xE4, 0x60,       // IN AL, 0x60
        0xA2, 0x00, 0x90, // MOV [0x9000], AL
        0xB0, 0x20,       // MOV AL, 0x20
        0xE6, 0x20,       // OUT 0x20, AL
        0xCF,             // IRET
    ];
    machine.load_at(0x8100, isr);

    // Main: STI, NOP sled, HLT
    let mut code: Vec<u8> = Vec::new();
    code.push(0xFB); // STI
    for _ in 0..100 {
        code.push(0x90); // NOP
    }
    code.push(0xF4); // HLT

    machine.load_at(0x7C00, &code);
    machine.cpu.eip = 0x7C00;
    machine.cpu.esp = 0xFFFE;
    machine.cpu.eflags |= 0x0200; // IF

    // Send scancode 0x1E ('A' key make code)
    machine.ps2.send_scancode(0x1E);

    let result = run_limited(&mut machine, 100_000);
    assert!(result.is_ok(), "Machine did not halt: {:?}", result);

    // Verify ISR stored the scancode
    let stored = machine.mem.read_u8(0x9000);
    assert_eq!(
        stored, 0x1E,
        "Keyboard ISR should store scancode 0x1E, got 0x{:02X}",
        stored
    );
}

// =============================================================================
// Test 3: ATA PIO read boot sector
// =============================================================================

/// Load a disk image with known data in sector 0, write a program that reads
/// sector 0 via ATA PIO, verify data matches.
#[test]
fn test_ata_read_boot_sector() {
    let mut machine = Machine::new(1024 * 1024);
    machine.bios_stubs = false;
    machine.ports.register(Box::new(Serial8250::new_capture(0x3F8)));

    // Create disk with known data in sector 0
    let mut disk_data = vec![0u8; 512];
    disk_data[0] = 0xDE;
    disk_data[1] = 0xAD;
    disk_data[2] = 0xBE;
    disk_data[3] = 0xEF;
    for i in 4..512 {
        disk_data[i] = (i & 0xFF) as u8;
    }
    machine.load_disk(disk_data);

    // Program: set up ATA PIO read of sector 0, then read 256 words to 0x8000
    // All ATA ports are > 0xFF so we must use OUT DX, AL / IN AL, DX
    let code: &[u8] = &[
        // Sector count = 1
        0xBA, 0xF2, 0x01, // MOV DX, 0x1F2
        0xB0, 0x01,       // MOV AL, 1
        0xEE,             // OUT DX, AL
        // LBA low = 0
        0xBA, 0xF3, 0x01, // MOV DX, 0x1F3
        0xB0, 0x00,       // MOV AL, 0
        0xEE,             // OUT DX, AL
        // LBA mid = 0
        0xBA, 0xF4, 0x01, // MOV DX, 0x1F4
        0xB0, 0x00,       // MOV AL, 0
        0xEE,             // OUT DX, AL
        // LBA high = 0
        0xBA, 0xF5, 0x01, // MOV DX, 0x1F5
        0xB0, 0x00,       // MOV AL, 0
        0xEE,             // OUT DX, AL
        // Drive/Head = 0xE0 (LBA mode, drive 0)
        0xBA, 0xF6, 0x01, // MOV DX, 0x1F6
        0xB0, 0xE0,       // MOV AL, 0xE0
        0xEE,             // OUT DX, AL
        // Command = 0x20 (READ SECTORS)
        0xBA, 0xF7, 0x01, // MOV DX, 0x1F7
        0xB0, 0x20,       // MOV AL, 0x20
        0xEE,             // OUT DX, AL
        // Poll status for DRQ
        // .wait: IN AL, DX; TEST AL, 0x08; JZ .wait
        0xEC,             // IN AL, DX  (DX=0x1F7)
        0xA8, 0x08,       // TEST AL, 0x08
        0x74, 0xFB,       // JZ -5 (back to IN)
        // Read 256 words from 0x1F0 into ES:DI (0x0000:0x8000)
        0xBA, 0xF0, 0x01, // MOV DX, 0x1F0
        0xBF, 0x00, 0x80, // MOV DI, 0x8000
        0xB9, 0x00, 0x01, // MOV CX, 256
        // .read: IN AX, DX; STOSW; LOOP .read
        0xED,             // IN AX, DX  (16-bit read)
        0xAB,             // STOSW
        0xE2, 0xFC,       // LOOP -4
        // HLT
        0xF4,
    ];

    machine.load_at(0x7C00, code);
    machine.cpu.eip = 0x7C00;
    machine.cpu.esp = 0xFFFE;
    machine.cpu.es = 0x0000;

    let result = run_limited(&mut machine, 100_000);
    assert!(result.is_ok(), "Machine did not halt: {:?}", result);

    // Verify data at 0x8000
    assert_eq!(machine.mem.read_u8(0x8000), 0xDE, "Byte 0 mismatch");
    assert_eq!(machine.mem.read_u8(0x8001), 0xAD, "Byte 1 mismatch");
    assert_eq!(machine.mem.read_u8(0x8002), 0xBE, "Byte 2 mismatch");
    assert_eq!(machine.mem.read_u8(0x8003), 0xEF, "Byte 3 mismatch");
    for i in 4u32..16 {
        let expected = (i & 0xFF) as u8;
        let actual = machine.mem.read_u8(0x8000 + i);
        assert_eq!(
            actual, expected,
            "Byte {} mismatch: expected 0x{:02X}, got 0x{:02X}",
            i, expected, actual
        );
    }
}

// =============================================================================
// Test 4: CMOS memory size
// =============================================================================

/// Read CMOS base memory registers (0x15-0x16), verify it returns 640 KB.
#[test]
fn test_cmos_memory_size() {
    let mut machine = Machine::new(1024 * 1024);
    machine.bios_stubs = false;
    machine.ports.register(Box::new(Serial8250::new_capture(0x3F8)));

    // Select register 0x15, read low byte into BL
    // Select register 0x16, read high byte into BH
    // Result in BX should be 640
    let code: &[u8] = &[
        0xB0, 0x15,       // MOV AL, 0x15
        0xE6, 0x70,       // OUT 0x70, AL
        0xE4, 0x71,       // IN AL, 0x71
        0x88, 0xC3,       // MOV BL, AL
        0xB0, 0x16,       // MOV AL, 0x16
        0xE6, 0x70,       // OUT 0x70, AL
        0xE4, 0x71,       // IN AL, 0x71
        0x88, 0xC7,       // MOV BH, AL
        0xF4,             // HLT
    ];

    machine.load_at(0x7C00, code);
    machine.cpu.eip = 0x7C00;
    machine.cpu.esp = 0xFFFE;

    let result = run_limited(&mut machine, 1000);
    assert!(result.is_ok(), "Machine did not halt: {:?}", result);

    let bx = machine.cpu.ebx as u16;
    assert_eq!(bx, 640, "CMOS base memory should be 640 KB, got {}", bx);
}

// =============================================================================
// Test 5: Protected mode entry with VGA write
// =============================================================================

/// Set up GDT, load LGDT, set CR0 PE, far JMP to reload CS, write "PM"
/// to VGA buffer, halt. Verify VGA buffer contains "PM".
#[test]
fn test_protected_mode_entry() {
    let mut machine = Machine::new(1024 * 1024);
    machine.bios_stubs = false;
    machine.ports.register(Box::new(Serial8250::new_capture(0x3F8)));

    // GDT at 0x1000:
    //   Entry 0: Null
    //   Entry 1: Code seg (base=0, limit=4GB, G=1, D=1, code+read)
    //   Entry 2: Data seg (base=0, limit=4GB, G=1, D=1, data+write)
    machine.load_at(0x1000, &[0u8; 8]); // null
    machine.load_at(0x1008, &[0xFF, 0xFF, 0x00, 0x00, 0x00, 0x9A, 0xCF, 0x00]); // code
    machine.load_at(0x1010, &[0xFF, 0xFF, 0x00, 0x00, 0x00, 0x92, 0xCF, 0x00]); // data

    // GDT pointer at 0x0F00
    machine.mem.write_u16(0x0F00, 0x0017); // limit = 3*8-1 = 23
    machine.mem.write_u32(0x0F02, 0x00001000); // base

    // Real-mode code at 0x7C00:
    let mut code: Vec<u8> = Vec::new();

    // CLI
    code.push(0xFA);
    // LGDT [0x0F00]: 0F 01 16 00 0F
    code.extend_from_slice(&[0x0F, 0x01, 0x16, 0x00, 0x0F]);
    // MOV EAX, CR0: 0F 20 C0
    code.extend_from_slice(&[0x0F, 0x20, 0xC0]);
    // OR AL, 1: 0C 01
    code.extend_from_slice(&[0x0C, 0x01]);
    // MOV CR0, EAX: 0F 22 C0
    code.extend_from_slice(&[0x0F, 0x22, 0xC0]);

    // JMP FAR 0x08:pm_target (16-bit offset in real mode)
    let jmp_pos = code.len();
    code.push(0xEA);
    code.push(0x00); // placeholder offset low
    code.push(0x00); // placeholder offset high
    code.extend_from_slice(&[0x08, 0x00]); // segment = 0x0008

    let pm_target = 0x7C00u16 + code.len() as u16;
    code[jmp_pos + 1] = pm_target as u8;
    code[jmp_pos + 2] = (pm_target >> 8) as u8;

    // --- Now in 32-bit protected mode (D=1 code segment) ---

    // MOV AX, 0x10; MOV DS, AX; MOV ES, AX
    code.extend_from_slice(&[0x66, 0xB8, 0x10, 0x00]); // MOV AX, 0x10
    code.extend_from_slice(&[0x8E, 0xD8]);               // MOV DS, AX
    code.extend_from_slice(&[0x8E, 0xC0]);               // MOV ES, AX

    // MOV BYTE [0xB8000], 'P'   (C6 05 00 80 0B 00 50)
    code.extend_from_slice(&[0xC6, 0x05]);
    code.extend_from_slice(&0x000B8000u32.to_le_bytes());
    code.push(b'P');

    // MOV BYTE [0xB8001], 0x0F
    code.extend_from_slice(&[0xC6, 0x05]);
    code.extend_from_slice(&0x000B8001u32.to_le_bytes());
    code.push(0x0F);

    // MOV BYTE [0xB8002], 'M'
    code.extend_from_slice(&[0xC6, 0x05]);
    code.extend_from_slice(&0x000B8002u32.to_le_bytes());
    code.push(b'M');

    // MOV BYTE [0xB8003], 0x0F
    code.extend_from_slice(&[0xC6, 0x05]);
    code.extend_from_slice(&0x000B8003u32.to_le_bytes());
    code.push(0x0F);

    // HLT
    code.push(0xF4);

    machine.load_at(0x7C00, &code);
    machine.cpu.eip = 0x7C00;
    machine.cpu.esp = 0xFFFE;

    let result = run_limited(&mut machine, 100_000);
    assert!(result.is_ok(), "Machine did not halt: {:?}", result);

    // Verify protected mode
    assert_eq!(machine.cpu.mode, kokoa86_cpu::CpuMode::ProtectedMode);
    assert_eq!(machine.cpu.cs, 0x08);

    // Sync VGA from RAM and check "PM"
    machine.sync_vga_from_ram();
    assert_eq!(machine.vga.buffer[0], b'P', "VGA[0] should be 'P'");
    assert_eq!(machine.vga.buffer[1], 0x0F, "VGA[1] should be 0x0F");
    assert_eq!(machine.vga.buffer[2], b'M', "VGA[2] should be 'M'");
    assert_eq!(machine.vga.buffer[3], 0x0F, "VGA[3] should be 0x0F");
}

// =============================================================================
// Test 6: 32-bit addressing with 0x67 prefix in 16-bit mode
// =============================================================================

/// Use 0x67 prefix (32-bit addressing in 16-bit mode) to access memory via EBX.
#[test]
fn test_32bit_addressing() {
    let mut machine = Machine::new(1024 * 1024);
    machine.bios_stubs = false;
    machine.ports.register(Box::new(Serial8250::new_capture(0x3F8)));

    // Put 0x42 at linear address 0xA000 (DS=0, so linear = offset)
    machine.mem.write_u8(0xA000, 0x42);

    // MOV EBX, 0xA000          ; 66 BB 00 A0 00 00
    // 67 MOV AL, [EBX]         ; 67 8A 03  (32-bit addr: modrm 03 = [EBX])
    // MOV [0x9000], AL         ; A2 00 90
    // HLT
    let code: &[u8] = &[
        0x66, 0xBB, 0x00, 0xA0, 0x00, 0x00, // MOV EBX, 0x0000A000
        0x67, 0x8A, 0x03,                     // MOV AL, [EBX] (32-bit addressing)
        0xA2, 0x00, 0x90,                     // MOV [0x9000], AL
        0xF4,                                 // HLT
    ];

    machine.load_at(0x7C00, code);
    machine.cpu.eip = 0x7C00;
    machine.cpu.esp = 0xFFFE;

    let result = run_limited(&mut machine, 1000);
    assert!(result.is_ok(), "Machine did not halt: {:?}", result);

    assert_eq!(machine.cpu.eax as u8, 0x42, "AL should be 0x42");
    assert_eq!(
        machine.mem.read_u8(0x9000),
        0x42,
        "Memory at 0x9000 should be 0x42"
    );
}

// =============================================================================
// Test 7: VGA direct write via ES=0xB800
// =============================================================================

/// Write colored text to VGA buffer using ES=0xB800 and STOSB.
/// STOSB writes AL to ES:DI and increments DI, which naturally targets
/// the VGA text buffer. Verify Machine.vga after sync_vga_from_ram().
#[test]
fn test_vga_direct_write() {
    let mut machine = Machine::new(1024 * 1024);
    machine.bios_stubs = false;
    machine.ports.register(Box::new(Serial8250::new_capture(0x3F8)));

    // MOV AX, 0xB800; MOV ES, AX; XOR DI, DI
    // Write 'H' (char) + 0x0A (green attr) + 'i' (char) + 0x0C (red attr) via STOSB
    let code: &[u8] = &[
        0xB8, 0x00, 0xB8, // MOV AX, 0xB800
        0x8E, 0xC0,       // MOV ES, AX
        0x31, 0xFF,       // XOR DI, DI
        0xB0, b'H', 0xAA, // MOV AL, 'H'; STOSB
        0xB0, 0x0A, 0xAA, // MOV AL, 0x0A; STOSB (green on black)
        0xB0, b'i', 0xAA, // MOV AL, 'i'; STOSB
        0xB0, 0x0C, 0xAA, // MOV AL, 0x0C; STOSB (red on black)
        0xF4,             // HLT
    ];

    machine.load_at(0x7C00, code);
    machine.cpu.eip = 0x7C00;
    machine.cpu.esp = 0xFFFE;

    let result = run_limited(&mut machine, 1000);
    assert!(result.is_ok(), "Machine did not halt: {:?}", result);

    machine.sync_vga_from_ram();

    assert_eq!(machine.vga.buffer[0], b'H', "VGA[0] should be 'H'");
    assert_eq!(machine.vga.buffer[1], 0x0A, "VGA[1] should be 0x0A (green)");
    assert_eq!(machine.vga.buffer[2], b'i', "VGA[2] should be 'i'");
    assert_eq!(machine.vga.buffer[3], 0x0C, "VGA[3] should be 0x0C (red)");
}

// =============================================================================
// Test 8: Serial output integration
// =============================================================================

/// Output "test" to COM1 (port 0x3F8) and verify CPU halts correctly.
#[test]
fn test_serial_output_integration() {
    let mut machine = Machine::new(1024 * 1024);
    machine.bios_stubs = false;
    machine.ports.register(Box::new(Serial8250::new_capture(0x3F8)));

    // MOV DX, 0x3F8
    // MOV AL, 't'; OUT DX, AL
    // MOV AL, 'e'; OUT DX, AL
    // MOV AL, 's'; OUT DX, AL
    // MOV AL, 't'; OUT DX, AL
    // HLT
    let code: &[u8] = &[
        0xBA, 0xF8, 0x03, // MOV DX, 0x3F8
        0xB0, b't',       // MOV AL, 't'
        0xEE,             // OUT DX, AL
        0xB0, b'e',       // MOV AL, 'e'
        0xEE,             // OUT DX, AL
        0xB0, b's',       // MOV AL, 's'
        0xEE,             // OUT DX, AL
        0xB0, b't',       // MOV AL, 't'
        0xEE,             // OUT DX, AL
        0xF4,             // HLT
    ];

    machine.load_at(0x7C00, code);
    machine.cpu.eip = 0x7C00;
    machine.cpu.esp = 0xFFFE;

    let result = run_limited(&mut machine, 1000);
    assert!(result.is_ok(), "Machine did not halt: {:?}", result);

    assert!(machine.cpu.halted, "CPU should be halted");
    assert_eq!(machine.cpu.edx as u16, 0x3F8, "DX should still be 0x3F8");
    assert_eq!(
        machine.cpu.eax as u8,
        b't',
        "AL should be 't' (last char written)"
    );
}
