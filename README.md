# kokoa86

x86 PC emulator written in Rust.

**Goal:** A single-binary x86 emulator that can eventually run Windows and Android x86.

## Current Status: Phase 1 (Real Mode)

- x86 CPU: ~30 real-mode instructions (MOV, ALU, stack, control flow, string ops, I/O)
- 1MB flat RAM with memory-mapped VGA text buffer
- VGA text mode (80x25, 16-color palette)
- COM1 serial output (8250 UART)
- BIOS INT 0x10 stub (teletype output)
- **GUI** with egui: VGA display, register panel, disassembly view, memory viewer
- Keyboard shortcuts: F5 (Run/Pause), F10 (Step), F2 (Reset)
- 71 tests passing

## Roadmap

| Phase | Target | Status |
|-------|--------|--------|
| 1 | Real mode CPU + VGA text + serial | **Done** |
| 2 | Protected mode + BIOS (SeaBIOS) boot | Planned |
| 3 | Paging + PCI + disk -> Linux boot | Planned |
| 4 | x86-64 + ACPI + JIT -> Windows / Android x86 | Planned |

## Build & Run

```bash
# CLI (loads flat binary)
cargo run --release -p kokoa86 -- program.bin

# GUI (built-in demo if no file given)
cargo run --release -p kokoa86-gui

# GUI with a binary
cargo run --release -p kokoa86-gui -- program.bin

# Run tests
cargo test
```

## Architecture

```
kokoa86/
  crates/
    kokoa86-cpu/   - x86 CPU: decoder, executor, flags, modrm
    kokoa86-mem/   - Memory bus (flat RAM)
    kokoa86-dev/   - Devices: serial, VGA text, port bus
    kokoa86-core/  - Machine: ties CPU + memory + devices
    kokoa86-cli/   - CLI binary
    kokoa86-gui/   - GUI binary (eframe/egui)
```

## License

MIT
