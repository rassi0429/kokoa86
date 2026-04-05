# kokoa86

x86 PC emulator written in Rust.

**Goal:** A single-binary x86 emulator that can eventually run Windows and Android x86.

## Current Status: Phase 2 (Protected Mode)

### CPU
- ~70 opcodes: MOV, ALU, shift/rotate, MUL/IMUL/DIV/IDIV, stack, control flow,
  string ops (MOVS/STOS/LODS/CMPS/SCAS + REP), MOVZX/MOVSX, SETcc, Jcc near
- 0x66 operand-size prefix (32-bit operations in 16-bit mode)
- 0x0F two-byte opcodes (LGDT/LIDT, MOV CRn, SETcc, MOVZX/MOVSX)
- Protected mode entry (CR0 PE bit, segment descriptor caches)
- JMP/CALL FAR, RETF, IRET

### Devices
- **VGA** text mode (80x25, 16-color, memory-mapped at 0xB8000)
- **COM1** serial (8250 UART, transmit + scratch register)
- **PIC** 8259 dual cascade (master + slave, full ICW/OCW init, IRQ masking, EOI)
- **PIT** 8253 timer (3 channels, modes 0/2/3, IRQ0 generation)
- **PS/2** keyboard controller (8042, scancodes, self-test, IRQ1)
- **ATA/IDE** disk (PIO read/write, IDENTIFY, LBA28, multi-sector)

### GUI (eframe/egui)
- VGA display with 16-color rendering
- Register panel with flag indicators
- Disassembly view (15 instructions from current IP)
- Memory hex dump viewer
- Run/Pause/Step/Reset controls + speed slider
- Keyboard shortcuts: F5 (Run/Pause), F10 (Step), F2 (Reset)
- Keyboard input forwarded to PS/2 controller
- Custom dark theme

### Tests
- 111 tests passing across all crates
- CPU instruction tests, flag tests, device tests, integration tests

## Roadmap

| Phase | Target | Status |
|-------|--------|--------|
| 1 | Real mode CPU + VGA text + serial | Done |
| 2 | Protected mode + devices + BIOS boot | **In Progress** |
| 3 | Paging + PCI + disk -> Linux boot | Planned |
| 4 | x86-64 + ACPI + JIT -> Windows / Android x86 | Planned |

## Build & Run

```bash
# CLI (loads flat binary)
cargo run --release -p kokoa86 -- program.bin

# CLI with disk image
cargo run --release -p kokoa86 -- bootloader.bin --disk disk.img

# GUI (built-in demo if no file given)
cargo run --release -p kokoa86-gui

# GUI with disk image
cargo run --release -p kokoa86-gui -- --disk disk.img program.bin

# Run tests
cargo test
```

## Architecture

```
kokoa86/
  crates/
    kokoa86-cpu/   - x86 CPU: decoder, executor, flags, modrm
    kokoa86-mem/   - Memory bus (flat RAM)
    kokoa86-dev/   - Devices: serial, VGA, PIC, PIT, PS/2, ATA
    kokoa86-core/  - Machine: ties CPU + memory + devices
    kokoa86-cli/   - CLI binary
    kokoa86-gui/   - GUI binary (eframe/egui)
```

## License

MIT
