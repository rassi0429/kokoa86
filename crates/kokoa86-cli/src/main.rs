use anyhow::{Context, Result};
use clap::Parser;
use kokoa86_core::Machine;
use kokoa86_dev::Serial8250;
use std::fs;

#[derive(Parser)]
#[command(name = "kokoa86", about = "x86 PC emulator", version)]
struct Args {
    /// Binary file to load (flat binary, loaded at 0x7C00 by default)
    binary: String,

    /// Load address (hex, default: 0x7C00)
    #[arg(short, long, default_value = "7c00")]
    load_addr: String,

    /// RAM size in KB (default: 1024 = 1MB)
    #[arg(short, long, default_value_t = 1024)]
    ram: usize,

    /// Disable BIOS interrupt stubs
    #[arg(long)]
    no_bios_stubs: bool,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let args = Args::parse();

    let load_addr =
        usize::from_str_radix(&args.load_addr, 16).context("Invalid load address")?;

    let data = fs::read(&args.binary)
        .with_context(|| format!("Failed to read binary: {}", args.binary))?;

    log::info!(
        "Loading {} ({} bytes) at 0x{:05X}",
        args.binary,
        data.len(),
        load_addr
    );

    let mut machine = Machine::new(args.ram * 1024);
    machine.bios_stubs = !args.no_bios_stubs;

    // Register COM1
    machine
        .ports
        .register(Box::new(Serial8250::new(0x3F8)));

    // Load binary
    machine.load_at(load_addr, &data);

    // Set initial SP to top of conventional memory
    machine.cpu.esp = 0xFFFE;
    machine.cpu.ss = 0x0000;

    // Set IP to load address
    machine.cpu.eip = load_addr as u32;
    machine.cpu.cs = 0x0000;

    // Run
    machine.run()?;

    println!();
    log::info!("Emulation finished");

    Ok(())
}
