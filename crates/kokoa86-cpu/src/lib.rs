pub mod regs;
pub mod flags;
pub mod modrm;
pub mod decode;
pub mod execute;

pub use regs::{CpuMode, CpuState};
