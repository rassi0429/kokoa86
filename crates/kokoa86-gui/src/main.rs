use anyhow::Result;
use clap::Parser;
use eframe::egui;
use kokoa86_core::Machine;
use kokoa86_cpu::execute::ExecResult;
use kokoa86_dev::Serial8250;
use kokoa86_dev::vga;
use std::fs;

#[derive(Parser)]
#[command(name = "kokoa86-gui", about = "kokoa86 x86 emulator with GUI")]
struct Args {
    /// Binary file to load
    binary: Option<String>,

    /// Load address (hex, default: 7C00)
    #[arg(short, long, default_value = "7c00")]
    load_addr: String,

    /// RAM size in KB
    #[arg(short, long, default_value_t = 1024)]
    ram: usize,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let args = Args::parse();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 700.0])
            .with_title("kokoa86 — x86 Emulator"),
        ..Default::default()
    };

    eframe::run_native(
        "kokoa86",
        options,
        Box::new(move |cc| {
            // Set dark theme
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(EmulatorApp::new(args)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {}", e))
}

struct EmulatorApp {
    machine: Machine,
    running: bool,
    speed: usize, // instructions per frame
    status: String,
    show_registers: bool,
    show_memory: bool,
    memory_addr: String,
    error: Option<String>,
}

impl EmulatorApp {
    fn new(args: Args) -> Self {
        let mut machine = Machine::new(args.ram * 1024);
        machine.ports.register(Box::new(Serial8250::new(0x3F8)));

        let status;

        if let Some(ref path) = args.binary {
            match Self::load_binary(&mut machine, path, &args.load_addr) {
                Ok(msg) => {
                    status = msg;
                }
                Err(e) => {
                    status = format!("Error: {}", e);
                }
            }
        } else {
            // Load built-in demo program
            let demo = kokoa86_core::demo::demo_program();
            machine.load_at(0x7C00, &demo);
            machine.cpu.eip = 0x7C00;
            machine.cpu.esp = 0xFFFE;
            status = format!("Demo program loaded ({} bytes). Press Run to start!", demo.len());
        }

        Self {
            machine,
            running: false,
            speed: 10000,
            status,
            show_registers: true,
            show_memory: false,
            memory_addr: "7C00".to_string(),
            error: None,
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

        // Also write a demo to VGA buffer if no file loaded
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
            Ok(ExecResult::UnknownOpcode(byte)) => {
                self.running = false;
                self.error = Some(format!(
                    "Unknown opcode 0x{:02X} at {:04X}:{:04X}",
                    byte, self.machine.cpu.cs, self.machine.cpu.eip
                ));
            }
            Err(e) => {
                self.running = false;
                self.error = Some(format!("Error: {}", e));
            }
        }
    }
}

impl eframe::App for EmulatorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Run emulation
        self.step_emulation();

        // Request repaint if running
        if self.running {
            ctx.request_repaint();
        }

        // Top menu bar
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Load Binary...").clicked() {
                        if let Some(path) = rfd_open_file() {
                            match Self::load_binary(
                                &mut self.machine,
                                &path,
                                "7c00",
                            ) {
                                Ok(msg) => {
                                    self.status = msg;
                                    self.error = None;
                                    self.running = false;
                                }
                                Err(e) => {
                                    self.error = Some(format!("Load error: {}", e));
                                }
                            }
                        }
                        ui.close_menu();
                    }
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.show_registers, "Registers");
                    ui.checkbox(&mut self.show_memory, "Memory Viewer");
                });
            });
        });

        // Bottom status bar
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(ref err) = self.error {
                    ui.colored_label(egui::Color32::RED, err);
                } else {
                    ui.label(&self.status);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("Instructions: {}", self.machine.instruction_count));
                });
            });
        });

        // Control panel
        egui::TopBottomPanel::top("controls").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;

                let run_text = if self.running { "\u{23F8} Pause" } else { "\u{25B6} Run" };
                if ui.button(run_text).clicked() {
                    self.running = !self.running;
                    if self.running {
                        self.machine.cpu.halted = false;
                    }
                }

                if ui.button("\u{23ED} Step").clicked() {
                    self.running = false;
                    let _ = self.machine.step();
                }

                if ui.button("\u{23F9} Reset").clicked() {
                    self.running = false;
                    self.machine.cpu = kokoa86_cpu::CpuState::default();
                    self.machine.cpu.eip = 0x7C00;
                    self.machine.cpu.esp = 0xFFFE;
                    self.machine.instruction_count = 0;
                    self.error = None;
                    self.status = "Reset".to_string();
                }

                ui.separator();
                ui.label("Speed:");
                ui.add(egui::Slider::new(&mut self.speed, 1..=100_000).logarithmic(true));

                if self.machine.cpu.halted {
                    ui.colored_label(egui::Color32::YELLOW, "HALTED");
                } else if self.running {
                    ui.colored_label(egui::Color32::GREEN, "RUNNING");
                } else {
                    ui.label("PAUSED");
                }
            });
        });

        // Side panel: registers
        if self.show_registers {
            egui::SidePanel::right("registers")
                .default_width(220.0)
                .show(ctx, |ui| {
                    ui.heading("Registers");
                    ui.separator();

                    let cpu = &self.machine.cpu;
                    egui::Grid::new("reg_grid")
                        .num_columns(2)
                        .spacing([10.0, 4.0])
                        .show(ui, |ui| {
                            reg_row(ui, "AX", cpu.eax as u16);
                            reg_row(ui, "BX", cpu.ebx as u16);
                            reg_row(ui, "CX", cpu.ecx as u16);
                            reg_row(ui, "DX", cpu.edx as u16);
                            ui.end_row();
                            reg_row(ui, "SP", cpu.esp as u16);
                            reg_row(ui, "BP", cpu.ebp as u16);
                            reg_row(ui, "SI", cpu.esi as u16);
                            reg_row(ui, "DI", cpu.edi as u16);
                            ui.end_row();
                            reg_row(ui, "CS", cpu.cs);
                            reg_row(ui, "DS", cpu.ds);
                            reg_row(ui, "ES", cpu.es);
                            reg_row(ui, "SS", cpu.ss);
                            ui.end_row();
                            reg_row(ui, "IP", cpu.eip as u16);
                            ui.label("FLAGS");
                            ui.label(format!("{:016b}", cpu.eflags as u16));
                            ui.end_row();
                        });

                    ui.separator();
                    ui.heading("Flags");
                    ui.horizontal_wrapped(|ui| {
                        let f = cpu.eflags;
                        flag_label(ui, "CF", f & 1 != 0);
                        flag_label(ui, "ZF", f & (1 << 6) != 0);
                        flag_label(ui, "SF", f & (1 << 7) != 0);
                        flag_label(ui, "OF", f & (1 << 11) != 0);
                        flag_label(ui, "IF", f & (1 << 9) != 0);
                        flag_label(ui, "DF", f & (1 << 10) != 0);
                    });
                });
        }

        // Central panel: VGA display
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("VGA Display (Text Mode 80x25)");
            ui.separator();

            render_vga_text(ui, &self.machine.vga);
        });

        // Memory viewer window
        if self.show_memory {
            egui::Window::new("Memory Viewer")
                .open(&mut self.show_memory)
                .default_size([400.0, 300.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Address (hex):");
                        ui.text_edit_singleline(&mut self.memory_addr);
                    });
                    ui.separator();

                    if let Ok(addr) = u32::from_str_radix(self.memory_addr.trim(), 16) {
                        render_memory_dump(ui, &self.machine.mem, addr, 256);
                    }
                });
        }
    }
}

fn reg_row(ui: &mut egui::Ui, name: &str, val: u16) {
    ui.label(egui::RichText::new(name).monospace().strong());
    ui.label(
        egui::RichText::new(format!("{:04X}", val))
            .monospace()
            .color(egui::Color32::from_rgb(0x80, 0xCC, 0xFF)),
    );
    ui.end_row();
}

fn flag_label(ui: &mut egui::Ui, name: &str, set: bool) {
    let color = if set {
        egui::Color32::from_rgb(0x00, 0xFF, 0x80)
    } else {
        egui::Color32::from_rgb(0x60, 0x60, 0x60)
    };
    ui.label(egui::RichText::new(name).monospace().color(color));
}

fn render_vga_text(ui: &mut egui::Ui, vga: &kokoa86_dev::VgaText) {
    let cells = vga.render_cells();

    // Use a monospace layout
    egui::ScrollArea::both().show(ui, |ui| {
        let font_id = egui::FontId::monospace(14.0);

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
}

fn render_memory_dump(ui: &mut egui::Ui, mem: &kokoa86_mem::MemoryBus, start: u32, len: u32) {
    use kokoa86_mem::MemoryAccess;

    egui::ScrollArea::vertical().show(ui, |ui| {
        let font = egui::FontId::monospace(12.0);
        for row_start in (start..start + len).step_by(16) {
            let mut hex = format!("{:05X}: ", row_start);
            let mut ascii = String::new();
            for i in 0..16 {
                let byte = mem.read_u8(row_start + i);
                hex.push_str(&format!("{:02X} ", byte));
                if byte >= 0x20 && byte < 0x7F {
                    ascii.push(byte as char);
                } else {
                    ascii.push('.');
                }
            }
            hex.push_str(&format!(" {}", ascii));
            ui.label(egui::RichText::new(hex).font(font.clone()));
        }
    });
}

/// Simple file dialog (no rfd crate — just returns None for now, user can use CLI)
fn rfd_open_file() -> Option<String> {
    // Would use rfd crate for native file dialog, but keeping deps minimal
    // Users can pass binary path via CLI argument
    None
}
