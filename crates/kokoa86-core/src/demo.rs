/// Generate a demo real-mode program that writes colorful text to VGA buffer
///
/// The program writes directly to 0xB8000 (VGA text mode buffer)
/// to display a welcome message, then halts.
pub fn demo_program() -> Vec<u8> {
    let mut code: Vec<u8> = Vec::new();

    // Set up segment registers
    // MOV AX, 0xB800
    code.extend_from_slice(&[0xB8, 0x00, 0xB8]);
    // MOV ES, AX (8E C0)
    code.extend_from_slice(&[0x8E, 0xC0]);

    // Write "kokoa86 x86 PC Emulator" with colors to ES:0000
    let title = b"kokoa86 x86 PC Emulator";
    let colors: &[u8] = &[
        0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0E, 0x0E, // kokoa86 (red+yellow)
        0x07, // space
        0x0B, 0x0B, 0x0B, // x86 (cyan)
        0x07, // space
        0x0A, 0x0A, // PC (green)
        0x07, // space
        0x0F, 0x0F, 0x0F, 0x0F, 0x0F, 0x0F, 0x0F, 0x0F, // Emulator (white)
    ];

    // XOR DI, DI
    code.extend_from_slice(&[0x31, 0xFF]);

    // Write each character: ES:[DI] = char, ES:[DI+1] = attr
    for (i, &ch) in title.iter().enumerate() {
        let attr = if i < colors.len() { colors[i] } else { 0x07 };
        // MOV AL, ch
        code.extend_from_slice(&[0xB0, ch]);
        // STOSB (writes to ES:DI, DI++)
        code.push(0xAA);
        // MOV AL, attr
        code.extend_from_slice(&[0xB0, attr]);
        // STOSB
        code.push(0xAA);
    }

    // Move to row 2 (offset 160 * 2 = 320 = 0x140)
    // MOV DI, 0x00A0 (row 1, 160 bytes per row)
    code.extend_from_slice(&[0xBF, 0xA0, 0x00]);

    let line2 = b"Written in Rust - Phase 1: Real Mode";
    for &ch in line2.iter() {
        code.extend_from_slice(&[0xB0, ch]);
        code.push(0xAA);
        code.extend_from_slice(&[0xB0, 0x07]); // light gray
        code.push(0xAA);
    }

    // Row 3: progress bar
    // MOV DI, 0x0140 (row 2)
    code.extend_from_slice(&[0xBF, 0x40, 0x01]);

    let bar = b"[####............] Phase 1/4";
    let bar_colors: Vec<u8> = bar.iter().map(|&ch| {
        if ch == b'#' { 0x0A } // green
        else if ch == b'.' { 0x08 } // dark gray
        else { 0x07 } // default
    }).collect();

    for (i, &ch) in bar.iter().enumerate() {
        let attr = bar_colors[i];
        code.extend_from_slice(&[0xB0, ch]);
        code.push(0xAA);
        code.extend_from_slice(&[0xB0, attr]);
        code.push(0xAA);
    }

    // Row 5: instructions
    // MOV DI, 0x0280 (row 4)
    code.extend_from_slice(&[0xBF, 0x80, 0x02]);
    let line4 = b"Run/Pause/Step with controls above";
    for &ch in line4.iter() {
        code.extend_from_slice(&[0xB0, ch]);
        code.push(0xAA);
        code.extend_from_slice(&[0xB0, 0x0E]); // yellow
        code.push(0xAA);
    }

    // Row 7: colorful demo
    // MOV DI, 0x03C0 (row 6)
    code.extend_from_slice(&[0xBF, 0xC0, 0x03]);

    // Draw colored blocks
    // MOV CX, 16
    code.extend_from_slice(&[0xB9, 0x10, 0x00]);
    // MOV AL, 0xDB (full block character)
    code.extend_from_slice(&[0xB0, 0xDB]);
    // MOV AH, 0 (start color)
    code.extend_from_slice(&[0xB4, 0x00]);

    // .color_loop:
    // STOSB (write char)
    code.push(0xAA);

    // We need to write AH as the attribute but STOSB writes AL
    // Instead: MOV ES:[DI], AH; INC DI  — but we don't have that easily
    // Simpler: use explicit MOV byte to ES:[DI]
    // Actually, let's just use a different approach: pre-compute colors

    // Scrap the loop approach, just write 16 colored blocks directly
    code.truncate(code.len() - 1); // remove the STOSB
    // Remove the MOV CX, MOV AL, MOV AH
    code.truncate(code.len() - 6);

    for i in 0..16u8 {
        code.extend_from_slice(&[0xB0, 0xDB]); // MOV AL, block char
        code.push(0xAA); // STOSB
        code.extend_from_slice(&[0xB0, i | (i << 4)]); // MOV AL, color (fg=bg=i)
        code.push(0xAA); // STOSB
    }

    // Add a space then legend
    code.extend_from_slice(&[0xB0, b' ']);
    code.push(0xAA);
    code.extend_from_slice(&[0xB0, 0x07]);
    code.push(0xAA);

    let legend = b"<- 16 VGA colors";
    for &ch in legend.iter() {
        code.extend_from_slice(&[0xB0, ch]);
        code.push(0xAA);
        code.extend_from_slice(&[0xB0, 0x07]);
        code.push(0xAA);
    }

    // HLT
    code.push(0xF4);

    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_demo_program_halts() {
        let code = demo_program();
        let mut machine = crate::Machine::new(1024 * 1024);
        machine.bios_stubs = false;
        machine.load_at(0x7C00, &code);
        machine.cpu.eip = 0x7C00;
        machine.cpu.esp = 0xFFFE;

        // Should halt without error
        let result = machine.run();
        assert!(result.is_ok(), "Demo program should halt cleanly: {:?}", result);
        assert!(machine.cpu.halted);
    }

    #[test]
    fn test_demo_writes_to_vga_region() {
        use kokoa86_mem::MemoryAccess;

        let code = demo_program();
        let mut machine = crate::Machine::new(1024 * 1024);
        machine.bios_stubs = false;
        machine.load_at(0x7C00, &code);
        machine.cpu.eip = 0x7C00;
        machine.cpu.esp = 0xFFFE;

        machine.run().unwrap();

        // Check that VGA buffer at 0xB8000 has 'k' (first char of "kokoa86")
        let first_char = machine.mem.read_u8(0xB8000);
        assert_eq!(first_char, b'k');

        // Check attribute byte
        let first_attr = machine.mem.read_u8(0xB8001);
        assert_eq!(first_attr, 0x0C); // red
    }
}
