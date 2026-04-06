//! Basic Block Cache — decode once, execute many times
//!
//! This is a stepping stone toward full JIT. Instead of decoding
//! one instruction at a time, we decode entire basic blocks and
//! cache the decoded instruction sequences. Execution loops over
//! the cached instructions without re-decoding.
//!
//! Benefits over per-instruction decode cache:
//! - No cache lookup per instruction (just iterate Vec)
//! - Better branch prediction (tight loop)
//! - Foundation for future Cranelift JIT

use std::collections::HashMap;
use kokoa86_cpu::decode::{self, Instruction, Opcode};
use kokoa86_cpu::execute::{self, ExecResult, PortIo, IntHandler};
use kokoa86_cpu::regs::CpuState;
use kokoa86_mem::MemoryBus;

/// A decoded basic block
struct BasicBlock {
    /// Pre-decoded instructions
    instructions: Vec<Instruction>,
    /// Total byte length of the block
    byte_len: u32,
}

/// Basic block cache
pub struct BlockCache {
    blocks: HashMap<u32, BasicBlock>,
}

impl BlockCache {
    pub fn new() -> Self {
        Self {
            blocks: HashMap::with_capacity(4096),
        }
    }

    /// Decode and cache a basic block starting at `lip`
    fn decode_block(&mut self, lip: u32, cpu: &CpuState, mem: &MemoryBus) -> &BasicBlock {
        let mut instructions = Vec::with_capacity(32);
        let mut addr = lip;

        for _ in 0..128 {
            let inst = decode::decode_at_addr(cpu, mem, addr);
            let len = inst.len as u32;
            let is_terminal = is_block_terminator(&inst.op);
            instructions.push(inst);
            addr += len;
            if is_terminal { break; }
        }

        self.blocks.entry(lip).or_insert(BasicBlock {
            byte_len: addr - lip,
            instructions,
        })
    }

    /// Execute a basic block. Returns (ExecResult, instructions_executed).
    pub fn execute_block(
        &mut self,
        cpu: &mut CpuState,
        mem: &mut MemoryBus,
        ports: &mut dyn PortIo,
        int_handler: &mut dyn IntHandler,
    ) -> (ExecResult, u32) {
        let lip = cpu.cs_ip();

        // Decode if not cached
        if !self.blocks.contains_key(&lip) {
            self.decode_block(lip, cpu, mem);
        }

        // Execute cached block
        let block = self.blocks.get(&lip).unwrap();
        let mut count = 0u32;

        for inst in &block.instructions {
            let result = execute::execute(cpu, mem, ports, int_handler, inst);
            count += 1;
            match result {
                ExecResult::Continue => {}
                other => return (other, count),
            }
        }

        (ExecResult::Continue, count)
    }

    /// Invalidate all cached blocks (needed after code modification)
    pub fn invalidate(&mut self) {
        self.blocks.clear();
    }
}

/// Check if an opcode terminates a basic block
fn is_block_terminator(op: &Opcode) -> bool {
    matches!(op,
        Opcode::JmpShort(_) | Opcode::JmpNearRel(_) | Opcode::JmpFar(_, _) |
        Opcode::Jcc(_, _) | Opcode::JccNear(_, _) |
        Opcode::CallNearRel(_) | Opcode::CallFar(_, _) |
        Opcode::Ret | Opcode::RetImm16(_) | Opcode::Retf | Opcode::RetfImm16(_) |
        Opcode::Int(_) | Opcode::Iret | Opcode::Hlt |
        Opcode::Rep(_) | Opcode::Repne(_) |
        Opcode::Loop(_) | Opcode::Loope(_) | Opcode::Loopne(_) |
        Opcode::GroupFF(2, _, _) | Opcode::GroupFF(4, _, _) |
        Opcode::Unknown(_)
    )
}
