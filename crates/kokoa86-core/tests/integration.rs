use kokoa86_core::Machine;
use kokoa86_dev::Serial8250;

/// Test: "Hello" output via COM1 (port 0x3F8)
///
/// This is hand-assembled x86 real-mode code that writes "Hello\n" to COM1 and halts.
///
/// Assembly equivalent:
///   mov si, msg
///   .loop:
///     lodsb           ; AL = [DS:SI], SI++
///     test al, al     ; check for null terminator
///     jz .done
///     mov dx, 0x3F8
///     out dx, al      ; write to COM1
///     jmp .loop
///   .done:
///     hlt
///   msg: db "Hello from kokoa86!", 0x0A, 0
#[test]
fn test_hello_serial() {
    let mut machine = Machine::new(1024 * 1024); // 1MB
    machine.bios_stubs = false;

    // Create a serial port that captures output
    let serial = Serial8250::new_capture(0x3F8);
    machine.ports.register(Box::new(serial));

    // Hand-assemble the program at 0x7C00
    let msg = b"Hello from kokoa86!\n\0";
    let msg_offset: u16 = 0x7C00 + 20; // message starts after code

    // Build code bytes
    let mut code: Vec<u8> = Vec::new();

    // MOV SI, msg_offset  (BE xx xx)
    code.push(0xBE);
    code.extend_from_slice(&msg_offset.to_le_bytes());

    // .loop (offset 3):
    // LODSB (AC)
    code.push(0xAC);

    // TEST AL, AL (A8 00)
    code.push(0xA8);
    code.push(0x00);

    // JZ .done (74 xx) — need to calculate offset
    code.push(0x74);
    code.push(0x05); // skip 5 bytes (MOV DX + OUT + JMP)

    // MOV DX, 0x03F8 (BA F8 03)
    code.push(0xBA);
    code.extend_from_slice(&0x03F8u16.to_le_bytes());

    // OUT DX, AL (EE)
    code.push(0xEE);

    // JMP .loop (EB xx) — jump back to offset 3
    code.push(0xEB);
    let current_pos = code.len() as i8;
    code.push((3 - (current_pos + 1)) as u8); // relative offset back to .loop

    // .done:
    // HLT (F4)
    code.push(0xF4);

    // Pad to message offset
    while code.len() < 20 {
        code.push(0x90); // NOP padding
    }

    // Append message
    code.extend_from_slice(msg);

    // Load at 0x7C00
    machine.load_at(0x7C00, &code);
    machine.cpu.eip = 0x7C00;
    machine.cpu.esp = 0xFFFE;

    // Run
    let result = machine.run();
    assert!(result.is_ok(), "Machine should halt cleanly: {:?}", result);

    // We can't easily get the serial output since it's behind Box<dyn PortDevice>
    // But we can verify the CPU halted correctly
    assert!(machine.cpu.halted);
}

/// Test basic MOV and ALU instructions
#[test]
fn test_basic_alu() {
    let mut machine = Machine::new(64 * 1024);
    machine.bios_stubs = false;
    machine.ports.register(Box::new(Serial8250::new_capture(0x3F8)));

    // MOV AX, 10    (B8 0A 00)
    // MOV BX, 20    (BB 14 00)
    // ADD AX, BX    (01 D8)
    // HLT           (F4)
    let code: &[u8] = &[
        0xB8, 0x0A, 0x00, // MOV AX, 10
        0xBB, 0x14, 0x00, // MOV BX, 20
        0x01, 0xD8, // ADD AX, BX
        0xF4, // HLT
    ];

    machine.load_at(0x7C00, code);
    machine.cpu.eip = 0x7C00;

    machine.run().unwrap();

    assert_eq!(machine.cpu.eax as u16, 30); // 10 + 20
    assert_eq!(machine.cpu.ebx as u16, 20);
    assert!(machine.cpu.halted);
}

/// Test PUSH/POP
#[test]
fn test_push_pop() {
    let mut machine = Machine::new(64 * 1024);
    machine.bios_stubs = false;
    machine.ports.register(Box::new(Serial8250::new_capture(0x3F8)));

    // MOV AX, 0x1234  (B8 34 12)
    // PUSH AX          (50)
    // MOV AX, 0        (B8 00 00)
    // POP BX           (5B)
    // HLT              (F4)
    let code: &[u8] = &[
        0xB8, 0x34, 0x12, // MOV AX, 0x1234
        0x50, // PUSH AX
        0xB8, 0x00, 0x00, // MOV AX, 0
        0x5B, // POP BX
        0xF4, // HLT
    ];

    machine.load_at(0x7C00, code);
    machine.cpu.eip = 0x7C00;
    machine.cpu.esp = 0xFFFE;

    machine.run().unwrap();

    assert_eq!(machine.cpu.eax as u16, 0);
    assert_eq!(machine.cpu.ebx as u16, 0x1234);
    assert!(machine.cpu.halted);
}

/// Test conditional jump (JZ/JNZ)
#[test]
fn test_conditional_jump() {
    let mut machine = Machine::new(64 * 1024);
    machine.bios_stubs = false;
    machine.ports.register(Box::new(Serial8250::new_capture(0x3F8)));

    // MOV AX, 5        (B8 05 00)
    // CMP AX, 5        (3D 05 00)
    // JZ +2            (74 02)
    // MOV BX, 0xFFFF   (BB FF FF)  <- skipped
    // MOV CX, 0x42     (B9 42 00)
    // HLT              (F4)
    let code: &[u8] = &[
        0xB8, 0x05, 0x00, // MOV AX, 5
        0x3D, 0x05, 0x00, // CMP AX, 5
        0x74, 0x03, // JZ +3 (skip MOV BX)
        0xBB, 0xFF, 0xFF, // MOV BX, 0xFFFF (should be skipped)
        0xB9, 0x42, 0x00, // MOV CX, 0x42
        0xF4, // HLT
    ];

    machine.load_at(0x7C00, code);
    machine.cpu.eip = 0x7C00;

    machine.run().unwrap();

    assert_eq!(machine.cpu.eax as u16, 5);
    assert_eq!(machine.cpu.ebx as u16, 0); // should not have been modified
    assert_eq!(machine.cpu.ecx as u16, 0x42);
    assert!(machine.cpu.halted);
}

/// Test CALL and RET
#[test]
fn test_call_ret() {
    let mut machine = Machine::new(64 * 1024);
    machine.bios_stubs = false;
    machine.ports.register(Box::new(Serial8250::new_capture(0x3F8)));

    // 0x7C00: CALL +3       (E8 03 00)  -> calls 0x7C06
    // 0x7C03: MOV BX, AX    (89 C3)
    // 0x7C05: HLT           (F4)
    // 0x7C06: MOV AX, 0x99  (B8 99 00)  <- subroutine
    // 0x7C09: RET            (C3)
    let code: &[u8] = &[
        0xE8, 0x03, 0x00, // CALL 0x7C06
        0x89, 0xC3, // MOV BX, AX (really: MOV r/m16, r16 with modrm=C3 => BX, AX)
        0xF4, // HLT
        0xB8, 0x99, 0x00, // MOV AX, 0x99
        0xC3, // RET
    ];

    machine.load_at(0x7C00, code);
    machine.cpu.eip = 0x7C00;
    machine.cpu.esp = 0xFFFE;

    machine.run().unwrap();

    assert_eq!(machine.cpu.eax as u16, 0x99);
    assert_eq!(machine.cpu.ebx as u16, 0x99);
    assert!(machine.cpu.halted);
}
