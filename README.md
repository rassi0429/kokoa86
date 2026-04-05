# kokoa86

x86 PC emulator written in Rust.

**Goal:** A single-binary x86 emulator that can eventually run Windows and Android x86.

## Current Status: Phase 2 Complete

### CPU (~90 opcodes)
- Full 16/32-bit support via 0x66 operand-size prefix
- 32-bit addressing with SIB byte via 0x67 address-size prefix
- ALU: ADD, SUB, CMP, AND, OR, XOR, ADC, SBB, TEST, NOT, NEG
- Shift/Rotate: SHL, SHR, SAR, ROL, ROR, RCL, RCR
- Multiply/Divide: MUL, IMUL (1/2/3-operand), DIV, IDIV
- Data: MOV, MOVZX, MOVSX, LEA, XCHG, BSWAP, XADD, CMPXCHG
- Bit ops: BSF, BSR, BT, BTS, BTR, BTC
- String: MOVSB/W/D, STOSB/W/D, LODSB/W/D, CMPSB/W, SCASB/W, INSB/W/D, OUTSB/W/D + REP/REPNE
- Control: JMP, Jcc (short+near), CALL, RET, JMP FAR, CALL FAR, RETF, LOOP, ENTER, LEAVE
- Stack: PUSH, POP, PUSHF, POPF (16/32-bit)
- System: LGDT, LIDT, SGDT, SIDT, MOV CRn, CPUID, RDTSC, RDMSR, WRMSR, WBINVD, INVD
- Misc: CBW/CWDE, CWD/CDQ, SAHF, LAHF, SETcc, INT, IRET, HLT, CLI, STI, CLD, STD

### Protected Mode
- CR0 PE bit mode switching
- GDT segment descriptor loading (base, limit, flags, granularity)
- Segment descriptor caches on all segment registers
- Far JMP/CALL/RETF reload CS descriptor
- Mode-aware cs_ip() for instruction fetch

### Devices (12)
- **VGA** text mode (80x25, 16-color, memory-mapped at 0xB8000)
- **COM1** serial (8250 UART)
- **PIC** 8259 x2 (master + slave cascade, ICW/OCW, IRQ masking, EOI)
- **PIT** 8253 timer (3 channels, modes 0/2/3, IRQ0)
- **PS/2** keyboard (8042 controller, scancodes, IRQ1)
- **ATA/IDE** disk (PIO read/write, IDENTIFY, LBA28)
- **CMOS/RTC** (MC146818, time, memory size, equipment)
- **PCI** bus (CONFIG_ADDRESS/CONFIG_DATA, host bridge, ISA bridge)
- **DMA** 8237 stubs (DMA1 + DMA2 + page registers)
- **POST** diagnostic port, **A20** gate, **System Control B**

### GUI (eframe/egui)
- VGA display with 16-color rendering
- CPU register panel with flag indicators
- Disassembly view (15 instructions from current IP)
- Memory hex dump viewer
- Run/Pause/Step/Reset + speed slider
- Keyboard input forwarded to PS/2 (Set 1 scancodes)
- Keyboard shortcuts: F5/F10/F2
- Custom dark theme with accent colors

### Tests: 132 passing
- CPU instruction unit tests (39 + 26 Phase 2)
- Flag computation tests (9)
- Device unit tests (PIC, PIT, PS/2, ATA, CMOS, PCI, serial, port bus)
- Descriptor parsing tests
- ModR/M 32-bit + SIB tests
- Integration tests (VGA write, CMOS read, protected mode, CPUID, PIC init, REP MOVSB)

## Roadmap

| Phase | Target | Status |
|-------|--------|--------|
| 1 | Real mode CPU + VGA text + serial | Done |
| 2 | Protected mode + devices + 32-bit | **Done** |
| 3 | Paging + PCI + disk -> Linux boot | Planned |
| 4 | x86-64 + ACPI + JIT -> Windows / Android x86 | Planned |

## Build & Run

```bash
# GUI (built-in demo)
cargo run --release -p kokoa86-gui

# GUI with binary + disk
cargo run --release -p kokoa86-gui -- program.bin --disk disk.img

# CLI
cargo run --release -p kokoa86 -- program.bin --disk disk.img

# Run all tests
cargo test
```

## Architecture

```
kokoa86/
  crates/
    kokoa86-cpu/   - x86 CPU: decoder, executor, flags, modrm, descriptors
    kokoa86-mem/   - Memory bus (flat RAM)
    kokoa86-dev/   - Devices: VGA, serial, PIC, PIT, PS/2, ATA, CMOS, PCI, DMA, misc
    kokoa86-core/  - Machine: CPU + memory + devices integration
    kokoa86-cli/   - CLI binary
    kokoa86-gui/   - GUI binary (eframe/egui)
```

## License

MIT
