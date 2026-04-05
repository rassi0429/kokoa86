use anyhow::Result;
use kokoa86_cpu::decode;
use kokoa86_cpu::execute::{self, ExecResult, IntHandler, PortIo};
use kokoa86_cpu::CpuState;
use kokoa86_dev::PortBus;
use kokoa86_mem::MemoryBus;

/// The main emulator machine — owns all components
pub struct Machine {
    pub cpu: CpuState,
    pub mem: MemoryBus,
    pub ports: PortBus,
    pub bios_stubs: bool, // Enable INT 0x10 / 0x16 BIOS stubs
}

impl Machine {
    pub fn new(ram_size: usize) -> Self {
        Self {
            cpu: CpuState::default(),
            mem: MemoryBus::new(ram_size),
            ports: PortBus::new(),
            bios_stubs: true,
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

        let mut port_adapter = PortBusAdapter { bus: &mut self.ports };
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

        Ok(result)
    }

    /// Run until HLT or error
    pub fn run(&mut self) -> Result<()> {
        let mut count: u64 = 0;
        loop {
            match self.step()? {
                ExecResult::Continue => {}
                ExecResult::Halt => {
                    log::info!("CPU halted after {} instructions", count);
                    return Ok(());
                }
                ExecResult::UnknownOpcode(byte) => {
                    anyhow::bail!(
                        "Unknown opcode 0x{:02X} at {:04X}:{:04X} after {} instructions",
                        byte,
                        self.cpu.cs,
                        self.cpu.eip,
                        count
                    );
                }
            }
            count += 1;
            if count % 1_000_000 == 0 {
                log::debug!("Executed {} instructions", count);
            }
        }
    }
}

/// Adapter: PortBus -> PortIo trait
struct PortBusAdapter<'a> {
    bus: &'a mut PortBus,
}

impl PortIo for PortBusAdapter<'_> {
    fn port_in(&mut self, port: u16, size: u8) -> u32 {
        self.bus.port_in(port, size)
    }

    fn port_out(&mut self, port: u16, size: u8, val: u32) {
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
                        // Wait for keypress — just return a dummy for now
                        cpu.set_reg8(0, 0x0D); // AL = Enter
                        cpu.set_reg8(4, 0x1C); // AH = scan code
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }
}
