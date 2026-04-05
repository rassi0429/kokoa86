#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CpuMode {
    #[default]
    RealMode,
    ProtectedMode,
    LongMode,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DescriptorTableReg {
    pub base: u32,
    pub limit: u16,
}

#[derive(Debug, Clone)]
pub struct CpuState {
    // General purpose registers
    pub eax: u32,
    pub ecx: u32,
    pub edx: u32,
    pub ebx: u32,
    pub esp: u32,
    pub ebp: u32,
    pub esi: u32,
    pub edi: u32,

    // Instruction pointer
    pub eip: u32,

    // Segment registers
    pub cs: u16,
    pub ds: u16,
    pub es: u16,
    pub fs: u16,
    pub gs: u16,
    pub ss: u16,

    // Flags
    pub eflags: u32,

    // Control registers
    pub cr0: u32,
    pub cr2: u32,
    pub cr3: u32,
    pub cr4: u32,

    // Descriptor table registers
    pub gdtr: DescriptorTableReg,
    pub idtr: DescriptorTableReg,
    pub ldtr: u16,
    pub tr: u16,

    // Mode
    pub mode: CpuMode,

    // State
    pub halted: bool,
}

impl Default for CpuState {
    fn default() -> Self {
        Self {
            eax: 0,
            ecx: 0,
            edx: 0,
            ebx: 0,
            esp: 0,
            ebp: 0,
            esi: 0,
            edi: 0,
            eip: 0x7C00, // Boot sector load address
            cs: 0x0000,
            ds: 0x0000,
            es: 0x0000,
            fs: 0x0000,
            gs: 0x0000,
            ss: 0x0000,
            eflags: 0x0002, // Bit 1 always set
            cr0: 0,
            cr2: 0,
            cr3: 0,
            cr4: 0,
            gdtr: DescriptorTableReg::default(),
            idtr: DescriptorTableReg::default(),
            ldtr: 0,
            tr: 0,
            mode: CpuMode::RealMode,
            halted: false,
        }
    }
}

impl CpuState {
    /// Compute linear address from segment:offset in real mode
    pub fn linear_addr(&self, seg: u16, offset: u16) -> u32 {
        ((seg as u32) << 4).wrapping_add(offset as u32)
    }

    /// Current CS:IP linear address
    pub fn cs_ip(&self) -> u32 {
        self.linear_addr(self.cs, self.eip as u16)
    }

    /// Read 8-bit register by index (AL=0, CL=1, DL=2, BL=3, AH=4, CH=5, DH=6, BH=7)
    pub fn get_reg8(&self, idx: u8) -> u8 {
        match idx {
            0 => self.eax as u8,         // AL
            1 => self.ecx as u8,         // CL
            2 => self.edx as u8,         // DL
            3 => self.ebx as u8,         // BL
            4 => (self.eax >> 8) as u8,  // AH
            5 => (self.ecx >> 8) as u8,  // CH
            6 => (self.edx >> 8) as u8,  // DH
            7 => (self.ebx >> 8) as u8,  // BH
            _ => unreachable!(),
        }
    }

    /// Write 8-bit register by index
    pub fn set_reg8(&mut self, idx: u8, val: u8) {
        match idx {
            0 => self.eax = (self.eax & 0xFFFFFF00) | val as u32,
            1 => self.ecx = (self.ecx & 0xFFFFFF00) | val as u32,
            2 => self.edx = (self.edx & 0xFFFFFF00) | val as u32,
            3 => self.ebx = (self.ebx & 0xFFFFFF00) | val as u32,
            4 => self.eax = (self.eax & 0xFFFF00FF) | ((val as u32) << 8),
            5 => self.ecx = (self.ecx & 0xFFFF00FF) | ((val as u32) << 8),
            6 => self.edx = (self.edx & 0xFFFF00FF) | ((val as u32) << 8),
            7 => self.ebx = (self.ebx & 0xFFFF00FF) | ((val as u32) << 8),
            _ => unreachable!(),
        }
    }

    /// Read 16-bit register by index (AX=0..DI=7)
    pub fn get_reg16(&self, idx: u8) -> u16 {
        match idx {
            0 => self.eax as u16,
            1 => self.ecx as u16,
            2 => self.edx as u16,
            3 => self.ebx as u16,
            4 => self.esp as u16,
            5 => self.ebp as u16,
            6 => self.esi as u16,
            7 => self.edi as u16,
            _ => unreachable!(),
        }
    }

    /// Write 16-bit register by index
    pub fn set_reg16(&mut self, idx: u8, val: u16) {
        match idx {
            0 => self.eax = (self.eax & 0xFFFF0000) | val as u32,
            1 => self.ecx = (self.ecx & 0xFFFF0000) | val as u32,
            2 => self.edx = (self.edx & 0xFFFF0000) | val as u32,
            3 => self.ebx = (self.ebx & 0xFFFF0000) | val as u32,
            4 => self.esp = (self.esp & 0xFFFF0000) | val as u32,
            5 => self.ebp = (self.ebp & 0xFFFF0000) | val as u32,
            6 => self.esi = (self.esi & 0xFFFF0000) | val as u32,
            7 => self.edi = (self.edi & 0xFFFF0000) | val as u32,
            _ => unreachable!(),
        }
    }

    /// Read segment register by index (ES=0, CS=1, SS=2, DS=3, FS=4, GS=5)
    pub fn get_sreg(&self, idx: u8) -> u16 {
        match idx {
            0 => self.es,
            1 => self.cs,
            2 => self.ss,
            3 => self.ds,
            4 => self.fs,
            5 => self.gs,
            _ => unreachable!(),
        }
    }

    /// Write segment register by index
    pub fn set_sreg(&mut self, idx: u8, val: u16) {
        match idx {
            0 => self.es = val,
            1 => self.cs = val,
            2 => self.ss = val,
            3 => self.ds = val,
            4 => self.fs = val,
            5 => self.gs = val,
            _ => unreachable!(),
        }
    }
}
