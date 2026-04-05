use anyhow::{Context, Result};
use clap::Parser;
use kokoa86_core::Machine;
use kokoa86_dev::Serial8250;
use std::fs;

#[derive(Parser)]
#[command(name = "kokoa86", about = "x86 PC emulator", version)]
struct Args {
    /// Binary file to load (flat binary at --load-addr)
    binary: Option<String>,

    /// BIOS ROM image (e.g., SeaBIOS bios.bin)
    #[arg(short, long)]
    bios: Option<String>,

    /// Load address for flat binary (hex, default: 7C00)
    #[arg(short, long, default_value = "7c00")]
    load_addr: String,

    /// RAM size in KB (default: 1024 = 1MB)
    #[arg(short, long, default_value_t = 1024)]
    ram: usize,

    /// Disk image file
    #[arg(short, long)]
    disk: Option<String>,

    /// Disable BIOS interrupt stubs
    #[arg(long)]
    no_bios_stubs: bool,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let args = Args::parse();

    let mut machine = Machine::new(args.ram * 1024);
    machine.bios_stubs = !args.no_bios_stubs;

    // Register COM1
    machine.ports.register(Box::new(Serial8250::new(0x3F8)));

    // Load disk image if provided
    if let Some(ref disk_path) = args.disk {
        let disk_data = fs::read(disk_path)
            .with_context(|| format!("Failed to read disk image: {}", disk_path))?;
        log::info!("Disk: {} ({} bytes, {} sectors)", disk_path, disk_data.len(), disk_data.len() / 512);
        machine.load_disk(disk_data);
    }

    // Load BIOS or flat binary
    if let Some(ref bios_path) = args.bios {
        let bios_data = fs::read(bios_path)
            .with_context(|| format!("Failed to read BIOS: {}", bios_path))?;
        log::info!("BIOS: {} ({} KB)", bios_path, bios_data.len() / 1024);
        machine.load_bios(bios_data);
    } else if let Some(ref binary) = args.binary {
        let load_addr = usize::from_str_radix(&args.load_addr, 16)
            .context("Invalid load address")?;
        let data = fs::read(binary)
            .with_context(|| format!("Failed to read: {}", binary))?;
        log::info!("Loading {} ({} bytes) at 0x{:05X}", binary, data.len(), load_addr);
        machine.load_at(load_addr, &data);
        machine.cpu.eip = load_addr as u32;
        machine.cpu.cs = 0x0000;
        machine.cpu.esp = 0xFFFE;
        machine.cpu.ss = 0x0000;
    } else {
        anyhow::bail!("Specify a binary file or --bios option");
    }

    machine.run()?;

    println!();
    log::info!("Emulation finished");
    Ok(())
}
