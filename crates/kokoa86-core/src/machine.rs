use anyhow::Result;
use kokoa86_cpu::decode;
use kokoa86_cpu::execute::{self, ExecResult, IntHandler, PortIo};
use kokoa86_cpu::CpuState;
use kokoa86_dev::{AtaDisk, Cmos, FwCfg, PciBus, Pic8259, Pit8253, PortBus, Ps2Controller, VgaText, vga};
use kokoa86_dev::port_bus::PortDevice;
use kokoa86_mem::{MemoryAccess, MemoryBus};

/// The main emulator machine — owns all components
pub struct Machine {
    pub cpu: CpuState,
    pub mem: MemoryBus,
    pub ports: PortBus,
    pub vga: VgaText,
    pub pic_master: Pic8259,
    pub pic_slave: Pic8259,
    pub pit: Pit8253,
    pub ps2: Ps2Controller,
    pub ata: AtaDisk,
    pub cmos: Cmos,
    pub pci: PciBus,
    pub bios_stubs: bool,
    pub instruction_count: u64,
    /// Captured serial output (COM1)
    pub serial_output: Vec<u8>,
}

impl Machine {
    pub fn new(ram_size: usize) -> Self {
        let mut m = Self {
            cpu: CpuState::default(),
            mem: MemoryBus::new(ram_size),
            ports: PortBus::new(),
            vga: VgaText::new(),
            pic_master: Pic8259::new(0x20, true),
            pic_slave: Pic8259::new(0xA0, false),
            pit: Pit8253::new(),
            ps2: Ps2Controller::new(),
            ata: AtaDisk::new(),
            cmos: Cmos::new_with_ram(ram_size),
            pci: PciBus::new().with_default_devices(),
            bios_stubs: true,
            instruction_count: 0,
            serial_output: Vec::new(),
        };
        // Register misc devices on PortBus
        m.ports.register(Box::new(kokoa86_dev::misc::PostPort::new()));
        m.ports.register(Box::new(kokoa86_dev::misc::SystemControlA::new()));
        m.ports.register(Box::new(kokoa86_dev::misc::SystemControlB::new()));
        m.ports.register(Box::new(kokoa86_dev::misc::DmaStub::new()));
        m.ports.register(Box::new(kokoa86_dev::misc::DmaPageRegs::new()));
        m.ports.register(Box::new(FwCfg::new(ram_size as u64)));
        m.ports.register(Box::new(kokoa86_dev::misc::Dma2Stub));
        m
    }

    /// Load binary data at a specific address
    pub fn load_at(&mut self, addr: usize, data: &[u8]) {
        self.mem.load(addr, data);
    }

    /// Load a BIOS ROM image (typically 256KB SeaBIOS).
    /// Maps at the top of the first 1MB (so 256KB ROM at 0xC0000-0xFFFFF,
    /// or 64KB ROM at 0xF0000-0xFFFFF). Sets reset vector.
    pub fn load_bios(&mut self, data: Vec<u8>) {
        let size = data.len() as u32;
        let base = 0x100000u32.saturating_sub(size); // e.g., 0xC0000 for 256KB
        log::info!("Mapping BIOS ROM ({} KB) at 0x{:05X}-0xFFFFF", size / 1024, base);
        self.mem.map_rom(base, data);

        // Set up BIOS Data Area (BDA) at 0x400
        // COM1 base address at 0x400
        self.mem.write_u16(0x400, 0x03F8); // COM1
        // Equipment word at 0x410: has serial ports
        self.mem.write_u16(0x410, 0x0021); // 1 serial port + VGA 80x25

        // Set CPU to BIOS reset vector: CS=0xF000, IP=0xFFF0
        // This is the standard x86 reset vector at physical 0xFFFF0
        self.cpu.cs = 0xF000;
        self.cpu.eip = 0xFFF0;
        // Real mode defaults
        self.cpu.ds = 0x0000;
        self.cpu.es = 0x0000;
        self.cpu.ss = 0x0000;
        self.cpu.esp = 0x0000;
        self.cpu.halted = false;
        self.bios_stubs = false; // BIOS handles its own interrupts
    }

    /// Load a disk image
    pub fn load_disk(&mut self, data: Vec<u8>) {
        self.ata.load_image(data);
    }

    /// Execute a single instruction
    pub fn step(&mut self) -> Result<ExecResult> {
        if self.cpu.halted {
            return Ok(ExecResult::Halt);
        }

        // Tick PIT (roughly 1 PIT tick per ~10 CPU instructions)
        if self.instruction_count % 10 == 0 {
            self.pit.tick(1);
        }

        // Check device IRQs and feed to PIC
        if self.pit.check_irq0() {
            self.pic_master.raise_irq(0);
        }
        if self.ps2.check_irq1() {
            self.pic_master.raise_irq(1);
        }
        if self.ata.irq14_pending {
            self.ata.irq14_pending = false;
            self.pic_slave.raise_irq(6); // IRQ14 = slave IRQ6
        }
        // Cascade: if slave has interrupt, raise IRQ2 on master
        if self.pic_slave.has_interrupt() {
            self.pic_master.raise_irq(2);
        }

        // Check for pending hardware interrupt
        if kokoa86_cpu::flags::get_flag(&self.cpu, kokoa86_cpu::flags::FLAG_IF) {
            if let Some(vector) = self.pic_master.get_interrupt() {
                if vector == self.pic_master.vector_offset + 2 {
                    // Cascade: acknowledge from slave
                    if let Some(slave_vec) = self.pic_slave.get_interrupt() {
                        // Dispatch slave interrupt
                        dispatch_hw_interrupt(&mut self.cpu, &mut self.mem, slave_vec);
                    }
                } else {
                    dispatch_hw_interrupt(&mut self.cpu, &mut self.mem, vector);
                }
            }
        }

        let inst = decode::decode(&self.cpu, &self.mem);

        let mut port_adapter = DevicePortAdapter {
            bus: &mut self.ports,
            vga: &mut self.vga,
            pic_master: &mut self.pic_master,
            pic_slave: &mut self.pic_slave,
            pit: &mut self.pit,
            ps2: &mut self.ps2,
            ata: &mut self.ata,
            cmos: &mut self.cmos,
            pci: &mut self.pci,
            serial_capture: &mut self.serial_output,
        };
        let mut int_adapter = BiosStubHandler {
            enabled: self.bios_stubs,
        };

        let result = execute::execute(
            &mut self.cpu,
            &mut self.mem,
            &mut port_adapter,
            &mut int_adapter,
            &inst,
        );

        self.instruction_count += 1;
        Ok(result)
    }

    /// Execute N instructions (for GUI frame-based stepping)
    pub fn step_n(&mut self, n: usize) -> Result<ExecResult> {
        for _ in 0..n {
            match self.step()? {
                ExecResult::Continue => {}
                other => {
                    self.sync_vga_from_ram();
                    return Ok(other);
                }
            }
        }
        self.sync_vga_from_ram();
        Ok(ExecResult::Continue)
    }

    /// Copy RAM at 0xB8000 into VGA text buffer (for display sync)
    pub fn sync_vga_from_ram(&mut self) {
        let base = vga::VGA_TEXT_BASE;
        let size = (vga::VGA_COLS * vga::VGA_ROWS * 2) as u32;
        for i in 0..size {
            self.vga.buffer[i as usize] = self.mem.read_u8(base + i);
        }
    }

    /// Run until HLT or error
    pub fn run(&mut self) -> Result<()> {
        loop {
            match self.step()? {
                ExecResult::Continue => {}
                ExecResult::Halt => {
                    log::info!("CPU halted after {} instructions", self.instruction_count);
                    return Ok(());
                }
                ExecResult::DivideError => {
                    log::warn!("Divide error at {:04X}:{:04X}", self.cpu.cs, self.cpu.eip);
                }
                ExecResult::UnknownOpcode(byte) => {
                    anyhow::bail!(
                        "Unknown opcode 0x{:02X} at {:04X}:{:04X} after {} instructions",
                        byte,
                        self.cpu.cs,
                        self.cpu.eip,
                        self.instruction_count
                    );
                }
            }
        }
    }
}

/// Dispatch a hardware interrupt in real mode (push FLAGS, CS, IP; load from IVT)
fn dispatch_hw_interrupt(cpu: &mut CpuState, mem: &mut MemoryBus, vector: u8) {
    // Save flags, CS, IP
    let sp = cpu.get_reg16(4).wrapping_sub(2);
    let ss_base = (cpu.ss as u32) << 4;
    mem.write_u16(ss_base + sp as u32, cpu.eflags as u16);
    let sp = sp.wrapping_sub(2);
    mem.write_u16(ss_base + sp as u32, cpu.cs);
    let sp = sp.wrapping_sub(2);
    mem.write_u16(ss_base + sp as u32, cpu.eip as u16);
    cpu.set_reg16(4, sp);

    // Clear IF and TF
    cpu.eflags &= !(kokoa86_cpu::flags::FLAG_IF | kokoa86_cpu::flags::FLAG_TF);

    // Load CS:IP from IVT
    let ivt_addr = (vector as u32) * 4;
    cpu.eip = mem.read_u16(ivt_addr) as u32;
    cpu.cs = mem.read_u16(ivt_addr + 2);
}

/// Unified port adapter that routes to all devices
struct DevicePortAdapter<'a> {
    bus: &'a mut PortBus,
    vga: &'a mut VgaText,
    pic_master: &'a mut Pic8259,
    pic_slave: &'a mut Pic8259,
    pit: &'a mut Pit8253,
    ps2: &'a mut Ps2Controller,
    ata: &'a mut AtaDisk,
    cmos: &'a mut Cmos,
    pci: &'a mut PciBus,
    serial_capture: &'a mut Vec<u8>,
}

impl PortIo for DevicePortAdapter<'_> {
    fn port_in(&mut self, port: u16, size: u8) -> u32 {
        match port {
            0x20..=0x21 => return self.pic_master.port_in(port, size),
            0xA0..=0xA1 => return self.pic_slave.port_in(port, size),
            0x40..=0x43 => return self.pit.port_in(port, size),
            0x60 | 0x64 => return self.ps2.port_in(port, size),
            0xE9 => return 0xE9, // QEMU ISA debug port (present)
            0x402 => return 0xE9, // QEMU debugcon port (present)
            0x70..=0x71 => return self.cmos.port_in(port, size),
            0xCF8..=0xCFF => return self.pci.port_in(port, size),
            0x1F0..=0x1F7 | 0x3F6 => return self.ata.port_in(port, size),
            0x3C0..=0x3CF | 0x3D4..=0x3DA => return self.vga.port_in(port) as u32,
            _ => {}
        }
        self.bus.port_in(port, size)
    }

    fn port_out(&mut self, port: u16, size: u8, val: u32) {
        // Capture serial output (COM1 THR, QEMU debug port 0xE9/0x402)
        if (port == 0x3F8 || port == 0xE9 || port == 0x402) && size == 1 {
            self.serial_capture.push(val as u8);
        }
        match port {
            0x20..=0x21 => { self.pic_master.port_out(port, size, val); return; }
            0xA0..=0xA1 => { self.pic_slave.port_out(port, size, val); return; }
            0x40..=0x43 => { self.pit.port_out(port, size, val); return; }
            0x60 | 0x64 => { self.ps2.port_out(port, size, val); return; }
            0x70..=0x71 => { self.cmos.port_out(port, size, val); return; }
            0xCF8..=0xCFF => { self.pci.port_out(port, size, val); return; }
            0x1F0..=0x1F7 | 0x3F6 => { self.ata.port_out(port, size, val); return; }
            0x3C0..=0x3CF | 0x3D4..=0x3DA => { self.vga.port_out(port, val as u8); return; }
            _ => {}
        }
        self.bus.port_out(port, size, val);
    }
}

/// BIOS interrupt stub handler
struct BiosStubHandler {
    enabled: bool,
}

impl IntHandler for BiosStubHandler {
    fn handle_int(&mut self, cpu: &mut CpuState, _mem: &mut MemoryBus, vector: u8) -> bool {
        if !self.enabled {
            return false;
        }

        match vector {
            0x10 => {
                let ah = cpu.get_reg8(4);
                match ah {
                    0x0E => {
                        let ch = cpu.get_reg8(0);
                        print!("{}", ch as char);
                        true
                    }
                    _ => false,
                }
            }
            0x16 => {
                let ah = cpu.get_reg8(4);
                match ah {
                    0x00 => {
                        cpu.set_reg8(0, 0x0D);
                        cpu.set_reg8(4, 0x1C);
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }
}
