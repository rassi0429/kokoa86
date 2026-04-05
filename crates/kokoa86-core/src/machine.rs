use anyhow::Result;
use kokoa86_cpu::decode;
use kokoa86_cpu::execute::{self, ExecResult, IntHandler, PortIo};
use kokoa86_cpu::CpuState;
use kokoa86_dev::{PortBus, VgaText, vga};
use kokoa86_mem::{MemoryAccess, MemoryBus};

/// The main emulator machine — owns all components
pub struct Machine {
    pub cpu: CpuState,
    pub mem: MemoryBus,
    pub ports: PortBus,
    pub vga: VgaText,
    pub bios_stubs: bool,
    pub instruction_count: u64,
}

impl Machine {
    pub fn new(ram_size: usize) -> Self {
        Self {
            cpu: CpuState::default(),
            mem: MemoryBus::new(ram_size),
            ports: PortBus::new(),
            vga: VgaText::new(),
            bios_stubs: true,
            instruction_count: 0,
        }
    }

    /// Load binary data at a specific address
    pub fn load_at(&mut self, addr: usize, data: &[u8]) {
        self.mem.load(addr, data);
    }

    /// Execute a single instruction
    pub fn step(&mut self) -> Result<ExecResult> {
        if self.cpu.halted {
            return Ok(ExecResult::Halt);
        }

        let inst = decode::decode(&self.cpu, &self.mem);

        let mut port_adapter = PortVgaAdapter {
            bus: &mut self.ports,
            vga: &mut self.vga,
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
    fn sync_vga_from_ram(&mut self) {
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

/// Adapter: PortBus + VGA I/O
struct PortVgaAdapter<'a> {
    bus: &'a mut PortBus,
    vga: &'a mut VgaText,
}

impl PortIo for PortVgaAdapter<'_> {
    fn port_in(&mut self, port: u16, size: u8) -> u32 {
        // VGA ports
        match port {
            0x3C0..=0x3CF | 0x3D4..=0x3DA => {
                return self.vga.port_in(port) as u32;
            }
            _ => {}
        }
        self.bus.port_in(port, size)
    }

    fn port_out(&mut self, port: u16, size: u8, val: u32) {
        match port {
            0x3C0..=0x3CF | 0x3D4..=0x3DA => {
                self.vga.port_out(port, val as u8);
                return;
            }
            _ => {}
        }
        self.bus.port_out(port, size, val);
    }
}

/// BIOS interrupt stub handler (INT 0x10 for teletype output)
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
                let ah = cpu.get_reg8(4); // AH
                match ah {
                    0x0E => {
                        // Teletype output: AL = character
                        let ch = cpu.get_reg8(0); // AL
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
