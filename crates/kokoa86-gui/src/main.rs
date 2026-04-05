use anyhow::Result;
use clap::Parser;
use eframe::egui;
use kokoa86_core::Machine;
use kokoa86_cpu::execute::ExecResult;
use kokoa86_dev::vga;
use kokoa86_dev::Serial8250;
use kokoa86_mem::MemoryAccess;
use std::fs;

#[derive(Parser)]
#[command(name = "kokoa86-gui", about = "kokoa86 x86 emulator with GUI")]
struct Args {
    /// Binary file to load
    binary: Option<String>,

    /// BIOS ROM image (e.g., SeaBIOS bios.bin)
    #[arg(short = 'B', long)]
    bios: Option<String>,

    /// Load address (hex, default: 7C00)
    #[arg(short, long, default_value = "7c00")]
    load_addr: String,

    /// RAM size in KB
    #[arg(short, long, default_value_t = 1024)]
    ram: usize,

    /// Disk image file
    #[arg(short, long)]
    disk: Option<String>,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let args = Args::parse();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 750.0])
            .with_title("kokoa86 - x86 Emulator"),
        ..Default::default()
    };

    eframe::run_native(
        "kokoa86",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(dark_theme());
            Ok(Box::new(EmulatorApp::new(args)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {}", e))
}

fn dark_theme() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = egui::Color32::from_rgb(0x1A, 0x1A, 0x2E);
    v.window_fill = egui::Color32::from_rgb(0x16, 0x16, 0x2B);
    v.extreme_bg_color = egui::Color32::from_rgb(0x0F, 0x0F, 0x1A);
    v
}

struct EmulatorApp {
    machine: Machine,
    running: bool,
    speed: usize,
    status: String,
    show_registers: bool,
    show_memory: bool,
    show_disasm: bool,
    show_serial: bool,
    memory_addr: String,
    error: Option<String>,
    #[allow(dead_code)]
    serial_output: String,
}

impl EmulatorApp {
    fn new(args: Args) -> Self {
        let mut machine = Machine::new(args.ram * 1024);
        machine.ports.register(Box::new(Serial8250::new(0x3F8)));

        // Load disk image if provided
        if let Some(ref disk_path) = args.disk {
            if let Ok(disk_data) = fs::read(disk_path) {
                log::info!("Loaded disk: {} ({} bytes)", disk_path, disk_data.len());
                machine.load_disk(disk_data);
            }
        }

        let status;

        if let Some(ref bios_path) = args.bios {
            match fs::read(bios_path) {
                Ok(bios_data) => {
                    let size = bios_data.len();
                    machine.load_bios(bios_data);
                    status = format!("BIOS loaded: {} ({} KB) - Press Run", bios_path, size / 1024);
                }
                Err(e) => {
                    status = format!("BIOS load error: {}", e);
                }
            }
        } else if let Some(ref path) = args.binary {
            match Self::load_binary(&mut machine, path, &args.load_addr) {
                Ok(msg) => status = msg,
                Err(e) => status = format!("Error: {}", e),
            }
        } else {
            let demo = kokoa86_core::demo::demo_program();
            machine.load_at(0x7C00, &demo);
            machine.cpu.eip = 0x7C00;
            machine.cpu.esp = 0xFFFE;
            status = format!(
                "Demo loaded ({} bytes) - Press Run or F5",
                demo.len()
            );
        }

        Self {
            machine,
            running: false,
            speed: 10000,
            status,
            show_registers: true,
            show_memory: false,
            show_disasm: true,
            show_serial: true,
            memory_addr: "7C00".to_string(),
            error: None,
            serial_output: String::new(),
        }
    }

    fn load_binary(machine: &mut Machine, path: &str, load_addr_hex: &str) -> Result<String> {
        let load_addr = usize::from_str_radix(load_addr_hex, 16)?;
        let data = fs::read(path)?;
        let len = data.len();
        machine.load_at(load_addr, &data);
        machine.cpu.eip = load_addr as u32;
        machine.cpu.cs = 0x0000;
        machine.cpu.esp = 0xFFFE;
        machine.cpu.ss = 0x0000;
        machine.cpu.halted = false;
        Ok(format!("Loaded {} ({} bytes) at 0x{:05X}", path, len, load_addr))
    }

    fn step_emulation(&mut self) {
        if !self.running || self.machine.cpu.halted {
            return;
        }

        match self.machine.step_n(self.speed) {
            Ok(ExecResult::Continue) => {}
            Ok(ExecResult::Halt) => {
                self.running = false;
                self.status = format!(
                    "CPU halted after {} instructions",
                    self.machine.instruction_count
                );
            }
            Ok(ExecResult::DivideError) => {
                self.running = false;
                self.error = Some("Divide error (#DE)".to_string());
            }
            Ok(ExecResult::UnknownOpcode(byte)) => {
                self.running = false;
                self.error = Some(format!(
                    "Unknown opcode 0x{:02X} at {:04X}:{:04X}",
                    byte, self.machine.cpu.cs, self.machine.cpu.eip
                ));
            }
            Err(e) => {
                self.running = false;
                self.error = Some(format!("{}", e));
            }
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            if i.key_pressed(egui::Key::F5) {
                self.running = !self.running;
                if self.running {
                    self.machine.cpu.halted = false;
                }
            }
            if i.key_pressed(egui::Key::F10) {
                self.running = false;
                let _ = self.machine.step();
                self.machine.sync_vga_from_ram();
            }
            if i.key_pressed(egui::Key::F2) {
                self.do_reset();
            }

            // Forward keyboard input to PS/2 controller (when running)
            if self.running {
                for event in &i.events {
                    if let egui::Event::Key { key, pressed, .. } = event {
                        if let Some(scancode) = egui_key_to_scancode(*key) {
                            if *pressed {
                                self.machine.ps2.send_scancode(scancode);
                            } else {
                                // Key release: send break code (0x80 | make code)
                                self.machine.ps2.send_scancode(0x80 | scancode);
                            }
                        }
                    }
                }
            }
        });
    }

    fn do_reset(&mut self) {
        self.running = false;
        self.machine.cpu = kokoa86_cpu::CpuState::default();
        self.machine.cpu.eip = 0x7C00;
        self.machine.cpu.esp = 0xFFFE;
        self.machine.instruction_count = 0;
        self.error = None;
        self.status = "Reset".to_string();
    }
}

impl eframe::App for EmulatorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_keys(ctx);
        self.step_emulation();

        if self.running {
            ctx.request_repaint();
        }

        // Menu bar
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.show_registers, "Registers (right panel)");
                    ui.checkbox(&mut self.show_disasm, "Disassembly (bottom)");
                    ui.checkbox(&mut self.show_serial, "Serial Output");
                    ui.checkbox(&mut self.show_memory, "Memory Viewer");
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("Keyboard Shortcuts").clicked() {
                        ui.close_menu();
                    }
                    ui.label("F5: Run/Pause  F10: Step  F2: Reset");
                });
            });
        });

        // Control bar
        egui::TopBottomPanel::top("controls").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

                let btn_size = egui::vec2(70.0, 24.0);

                let run_label = if self.running { "Pause" } else { "Run" };
                let run_color = if self.running {
                    egui::Color32::from_rgb(0xFF, 0x99, 0x33)
                } else {
                    egui::Color32::from_rgb(0x33, 0xCC, 0x66)
                };
                if ui
                    .add_sized(
                        btn_size,
                        egui::Button::new(
                            egui::RichText::new(run_label).strong().color(run_color),
                        ),
                    )
                    .clicked()
                {
                    self.running = !self.running;
                    if self.running {
                        self.machine.cpu.halted = false;
                    }
                }

                if ui
                    .add_sized(btn_size, egui::Button::new("Step"))
                    .clicked()
                {
                    self.running = false;
                    let _ = self.machine.step();
                    self.machine.sync_vga_from_ram();
                }

                if ui
                    .add_sized(btn_size, egui::Button::new("Reset"))
                    .clicked()
                {
                    self.do_reset();
                }

                ui.separator();

                ui.label("Speed:");
                ui.add(
                    egui::Slider::new(&mut self.speed, 1..=500_000)
                        .logarithmic(true)
                        .suffix(" inst/frame"),
                );

                ui.separator();

                // Status indicator
                let (indicator, color) = if self.machine.cpu.halted {
                    ("HALTED", egui::Color32::from_rgb(0xFF, 0xCC, 0x00))
                } else if self.running {
                    ("RUNNING", egui::Color32::from_rgb(0x00, 0xFF, 0x80))
                } else {
                    ("PAUSED", egui::Color32::from_rgb(0x88, 0x88, 0x88))
                };
                ui.label(egui::RichText::new(indicator).monospace().strong().color(color));
            });
            ui.add_space(2.0);
        });

        // Status bar
        egui::TopBottomPanel::bottom("status").min_height(22.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(ref err) = self.error {
                    ui.colored_label(egui::Color32::from_rgb(0xFF, 0x55, 0x55), format!("Error: {}", err));
                } else {
                    ui.label(
                        egui::RichText::new(&self.status)
                            .small()
                            .color(egui::Color32::from_rgb(0x99, 0x99, 0xBB)),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "IP:{:04X}:{:04X}  Inst:{}",
                            self.machine.cpu.cs,
                            self.machine.cpu.eip,
                            self.machine.instruction_count
                        ))
                        .monospace()
                        .small()
                        .color(egui::Color32::from_rgb(0x88, 0xAA, 0xCC)),
                    );
                });
            });
        });

        // Right panel: registers + flags
        if self.show_registers {
            egui::SidePanel::right("registers")
                .default_width(200.0)
                .min_width(180.0)
                .show(ctx, |ui| {
                    render_registers(ui, &self.machine.cpu);
                });
        }

        // Bottom panel: disassembly
        if self.show_disasm {
            egui::TopBottomPanel::bottom("disasm")
                .default_height(150.0)
                .min_height(80.0)
                .resizable(true)
                .show(ctx, |ui| {
                    render_disassembly(ui, &self.machine);
                });
        }

        // Central: VGA display
        egui::CentralPanel::default().show(ctx, |ui| {
            render_vga_display(ui, &self.machine.vga);
        });

        // Serial output window
        if self.show_serial {
            let serial_text = String::from_utf8_lossy(&self.machine.serial_output).into_owned();
            let serial_len = self.machine.serial_output.len();
            egui::Window::new("Serial / Debug Output")
                .open(&mut self.show_serial)
                .default_size([600.0, 300.0])
                .show(ctx, |ui| {
                    ui.label(format!("{} bytes", serial_len));
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(&serial_text)
                                    .monospace()
                                    .size(12.0)
                                    .color(egui::Color32::from_rgb(0x00, 0xFF, 0x80)),
                            );
                        });
                });
        }

        // Memory viewer window
        if self.show_memory {
            egui::Window::new("Memory Viewer")
                .open(&mut self.show_memory)
                .default_size([480.0, 350.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Address:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.memory_addr)
                                .desired_width(80.0)
                                .font(egui::FontId::monospace(13.0)),
                        );
                        if ui.button("Go").clicked() {}
                    });
                    ui.separator();
                    if let Ok(addr) = u32::from_str_radix(self.memory_addr.trim(), 16) {
                        render_memory_dump(ui, &self.machine.mem, addr, 256);
                    }
                });
        }
    }
}

// ============================================================
// Rendering helpers
// ============================================================

fn render_registers(ui: &mut egui::Ui, cpu: &kokoa86_cpu::CpuState) {
    let header_color = egui::Color32::from_rgb(0xCC, 0x99, 0xFF);
    let val_color = egui::Color32::from_rgb(0x80, 0xDD, 0xFF);

    ui.label(egui::RichText::new("CPU Registers").strong().color(header_color));
    ui.separator();

    egui::Grid::new("regs")
        .num_columns(2)
        .spacing([8.0, 3.0])
        .show(ui, |ui| {
            for (name, val) in [
                ("AX", cpu.eax as u16),
                ("BX", cpu.ebx as u16),
                ("CX", cpu.ecx as u16),
                ("DX", cpu.edx as u16),
            ] {
                ui.label(egui::RichText::new(name).monospace().strong().small());
                ui.label(egui::RichText::new(format!("{:04X}", val)).monospace().color(val_color).small());
                ui.end_row();
            }
            ui.label("");
            ui.label("");
            ui.end_row();
            for (name, val) in [
                ("SP", cpu.esp as u16),
                ("BP", cpu.ebp as u16),
                ("SI", cpu.esi as u16),
                ("DI", cpu.edi as u16),
            ] {
                ui.label(egui::RichText::new(name).monospace().strong().small());
                ui.label(egui::RichText::new(format!("{:04X}", val)).monospace().color(val_color).small());
                ui.end_row();
            }
            ui.label("");
            ui.label("");
            ui.end_row();
            for (name, val) in [
                ("CS", cpu.cs),
                ("DS", cpu.ds),
                ("ES", cpu.es),
                ("SS", cpu.ss),
                ("FS", cpu.fs),
                ("GS", cpu.gs),
            ] {
                ui.label(egui::RichText::new(name).monospace().strong().small());
                ui.label(egui::RichText::new(format!("{:04X}", val)).monospace().color(val_color).small());
                ui.end_row();
            }
            ui.label("");
            ui.label("");
            ui.end_row();
            ui.label(egui::RichText::new("IP").monospace().strong().small());
            ui.label(
                egui::RichText::new(format!("{:04X}", cpu.eip as u16))
                    .monospace()
                    .color(egui::Color32::from_rgb(0xFF, 0xCC, 0x55))
                    .small(),
            );
            ui.end_row();
        });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("Flags").strong().color(header_color));
    ui.separator();

    ui.horizontal_wrapped(|ui| {
        let f = cpu.eflags;
        for (name, bit) in [
            ("CF", 0),
            ("PF", 2),
            ("AF", 4),
            ("ZF", 6),
            ("SF", 7),
            ("IF", 9),
            ("DF", 10),
            ("OF", 11),
        ] {
            let set = (f >> bit) & 1 != 0;
            let color = if set {
                egui::Color32::from_rgb(0x00, 0xFF, 0x80)
            } else {
                egui::Color32::from_rgb(0x44, 0x44, 0x55)
            };
            ui.label(egui::RichText::new(name).monospace().small().color(color));
        }
    });

    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(format!("EFLAGS: {:08X}", cpu.eflags))
            .monospace()
            .small()
            .color(egui::Color32::from_rgb(0x77, 0x77, 0x99)),
    );
}

fn render_vga_display(ui: &mut egui::Ui, vga_state: &kokoa86_dev::VgaText) {
    let cells = vga_state.render_cells();

    // VGA display with a dark frame
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(0x00, 0x00, 0x00))
        .inner_margin(8.0)
        .outer_margin(4.0)
        .corner_radius(4.0)
        .stroke(egui::Stroke::new(2.0, egui::Color32::from_rgb(0x33, 0x33, 0x55)))
        .show(ui, |ui| {
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let font_id = egui::FontId::monospace(13.0);

                    for row in 0..vga::VGA_ROWS {
                        let mut job = egui::text::LayoutJob::default();
                        for col in 0..vga::VGA_COLS {
                            let (ch, fg, bg) = cells[row * vga::VGA_COLS + col];
                            let fg_color = egui::Color32::from_rgb(fg[0], fg[1], fg[2]);
                            let bg_color = egui::Color32::from_rgb(bg[0], bg[1], bg[2]);

                            job.append(
                                &ch.to_string(),
                                0.0,
                                egui::TextFormat {
                                    font_id: font_id.clone(),
                                    color: fg_color,
                                    background: bg_color,
                                    ..Default::default()
                                },
                            );
                        }
                        ui.label(job);
                    }
                });
        });
}

fn render_disassembly(ui: &mut egui::Ui, machine: &Machine) {
    let header_color = egui::Color32::from_rgb(0xCC, 0x99, 0xFF);
    ui.label(egui::RichText::new("Disassembly").strong().color(header_color));
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        let font = egui::FontId::monospace(12.0);
        let current_ip = machine.cpu.cs_ip();

        // Show ~15 instructions around current IP
        let mut addr = current_ip;
        for _ in 0..15 {
            let inst = kokoa86_cpu::decode::decode_at_addr(&machine.cpu, &machine.mem, addr);

            let is_current = addr == current_ip;
            let prefix = if is_current { ">" } else { " " };

            // Read raw bytes
            let mut bytes_str = String::new();
            for i in 0..inst.len as u32 {
                bytes_str.push_str(&format!("{:02X} ", machine.mem.read_u8(addr + i)));
            }

            let line = format!(
                "{} {:04X}:{:04X}  {:<18} {:?}",
                prefix,
                machine.cpu.cs,
                addr as u16,
                bytes_str.trim(),
                inst.op
            );

            let color = if is_current {
                egui::Color32::from_rgb(0xFF, 0xFF, 0x55)
            } else {
                egui::Color32::from_rgb(0xAA, 0xBB, 0xCC)
            };

            ui.label(egui::RichText::new(line).font(font.clone()).color(color));

            addr += inst.len as u32;
        }
    });
}

fn render_memory_dump(ui: &mut egui::Ui, mem: &kokoa86_mem::MemoryBus, start: u32, len: u32) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        let font = egui::FontId::monospace(12.0);
        let addr_color = egui::Color32::from_rgb(0x88, 0x88, 0xAA);
        let hex_color = egui::Color32::from_rgb(0xBB, 0xDD, 0xFF);
        let ascii_color = egui::Color32::from_rgb(0x88, 0xCC, 0x88);

        for row_start in (start..start + len).step_by(16) {
            let mut job = egui::text::LayoutJob::default();

            // Address
            job.append(
                &format!("{:05X}: ", row_start),
                0.0,
                egui::TextFormat {
                    font_id: font.clone(),
                    color: addr_color,
                    ..Default::default()
                },
            );

            // Hex bytes
            let mut ascii = String::new();
            for i in 0..16u32 {
                let byte = mem.read_u8(row_start + i);
                job.append(
                    &format!("{:02X} ", byte),
                    0.0,
                    egui::TextFormat {
                        font_id: font.clone(),
                        color: hex_color,
                        ..Default::default()
                    },
                );
                if byte >= 0x20 && byte < 0x7F {
                    ascii.push(byte as char);
                } else {
                    ascii.push('.');
                }
            }

            // ASCII
            job.append(
                &format!(" {}", ascii),
                0.0,
                egui::TextFormat {
                    font_id: font.clone(),
                    color: ascii_color,
                    ..Default::default()
                },
            );

            ui.label(job);
        }
    });
}

/// Map egui key to PS/2 Set 1 scancode (make code)
fn egui_key_to_scancode(key: egui::Key) -> Option<u8> {
    Some(match key {
        egui::Key::Escape => 0x01,
        egui::Key::Num1 => 0x02,
        egui::Key::Num2 => 0x03,
        egui::Key::Num3 => 0x04,
        egui::Key::Num4 => 0x05,
        egui::Key::Num5 => 0x06,
        egui::Key::Num6 => 0x07,
        egui::Key::Num7 => 0x08,
        egui::Key::Num8 => 0x09,
        egui::Key::Num9 => 0x0A,
        egui::Key::Num0 => 0x0B,
        egui::Key::Minus => 0x0C,
        egui::Key::Equals => 0x0D,
        egui::Key::Backspace => 0x0E,
        egui::Key::Tab => 0x0F,
        egui::Key::Q => 0x10,
        egui::Key::W => 0x11,
        egui::Key::E => 0x12,
        egui::Key::R => 0x13,
        egui::Key::T => 0x14,
        egui::Key::Y => 0x15,
        egui::Key::U => 0x16,
        egui::Key::I => 0x17,
        egui::Key::O => 0x18,
        egui::Key::P => 0x19,
        egui::Key::OpenBracket => 0x1A,
        egui::Key::CloseBracket => 0x1B,
        egui::Key::Enter => 0x1C,
        egui::Key::A => 0x1E,
        egui::Key::S => 0x1F,
        egui::Key::D => 0x20,
        egui::Key::F => 0x21,
        egui::Key::G => 0x22,
        egui::Key::H => 0x23,
        egui::Key::J => 0x24,
        egui::Key::K => 0x25,
        egui::Key::L => 0x26,
        egui::Key::Semicolon => 0x27,
        egui::Key::Z => 0x2C,
        egui::Key::X => 0x2D,
        egui::Key::C => 0x2E,
        egui::Key::V => 0x2F,
        egui::Key::B => 0x30,
        egui::Key::N => 0x31,
        egui::Key::M => 0x32,
        egui::Key::Comma => 0x33,
        egui::Key::Period => 0x34,
        egui::Key::Slash => 0x35,
        egui::Key::Space => 0x39,
        egui::Key::ArrowUp => 0x48,
        egui::Key::ArrowLeft => 0x4B,
        egui::Key::ArrowRight => 0x4D,
        egui::Key::ArrowDown => 0x50,
        egui::Key::Home => 0x47,
        egui::Key::End => 0x4F,
        egui::Key::PageUp => 0x49,
        egui::Key::PageDown => 0x51,
        egui::Key::Insert => 0x52,
        egui::Key::Delete => 0x53,
        _ => return None,
    })
}
