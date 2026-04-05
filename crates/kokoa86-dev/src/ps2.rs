/// PS/2 Keyboard Controller (Intel 8042)
///
/// Ports:
/// - 0x60: Data port (read: scancode, write: command to keyboard)
/// - 0x64: Status/Command port (read: status, write: controller command)
///
/// Status register bits:
///   bit 0: Output buffer full (OBF) — data available to read
///   bit 1: Input buffer full (IBF) — controller busy
///   bit 2: System flag
///   bit 3: Command/data (0=data written to 0x60, 1=command to 0x64)
///   bit 4: Keyboard enabled
///   bit 5: Transmit timeout
///   bit 6: Receive timeout
///   bit 7: Parity error

use crate::port_bus::PortDevice;
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct Ps2Controller {
    /// Output buffer (scancodes waiting to be read)
    output_buffer: VecDeque<u8>,
    /// Status register
    status: u8,
    /// Controller configuration byte
    config: u8,
    /// Pending controller command (from port 0x64)
    pending_cmd: Option<u8>,
    /// Whether keyboard is enabled
    keyboard_enabled: bool,
    /// IRQ1 pending
    pub irq1_pending: bool,
}

impl Ps2Controller {
    pub fn new() -> Self {
        Self {
            output_buffer: VecDeque::new(),
            status: 0x14, // system flag set, keyboard enabled
            config: 0x47, // default config: IRQ1 enabled, translation enabled
            pending_cmd: None,
            keyboard_enabled: true,
            irq1_pending: false,
        }
    }

    /// Queue a scancode (e.g., from GUI keyboard input)
    pub fn send_scancode(&mut self, code: u8) {
        if self.keyboard_enabled {
            self.output_buffer.push_back(code);
            self.status |= 0x01; // OBF
            if self.config & 0x01 != 0 {
                self.irq1_pending = true;
            }
        }
    }

    /// Check and clear IRQ1
    pub fn check_irq1(&mut self) -> bool {
        let p = self.irq1_pending;
        self.irq1_pending = false;
        p
    }

    fn read_output(&mut self) -> u8 {
        if let Some(byte) = self.output_buffer.pop_front() {
            if self.output_buffer.is_empty() {
                self.status &= !0x01; // clear OBF
            }
            byte
        } else {
            0
        }
    }
}

impl PortDevice for Ps2Controller {
    fn port_in(&mut self, port: u16, _size: u8) -> u32 {
        match port {
            0x60 => self.read_output() as u32,
            0x64 => self.status as u32,
            _ => 0,
        }
    }

    fn port_out(&mut self, port: u16, _size: u8, val: u32) {
        let val = val as u8;
        match port {
            0x60 => {
                if let Some(cmd) = self.pending_cmd.take() {
                    match cmd {
                        0x60 => {
                            // Write configuration byte
                            self.config = val;
                        }
                        0xD1 => {
                            // Write output port (A20 gate control, etc.)
                            // bit 1 = A20 gate — just ignore for now
                        }
                        _ => {}
                    }
                } else {
                    // Data to keyboard — send command to keyboard
                    match val {
                        0xFF => {
                            // Reset — respond with ACK + BAT success
                            self.output_buffer.push_back(0xFA); // ACK
                            self.output_buffer.push_back(0xAA); // BAT OK
                            self.status |= 0x01;
                        }
                        0xF5 => {
                            // Disable scanning
                            self.output_buffer.push_back(0xFA);
                            self.status |= 0x01;
                            self.keyboard_enabled = false;
                        }
                        0xF4 => {
                            // Enable scanning
                            self.output_buffer.push_back(0xFA);
                            self.status |= 0x01;
                            self.keyboard_enabled = true;
                        }
                        0xED => {
                            // Set LEDs — next byte is LED state, just ACK
                            self.output_buffer.push_back(0xFA);
                            self.status |= 0x01;
                        }
                        0xF0 => {
                            // Set scancode set — ACK, then wait for param
                            self.output_buffer.push_back(0xFA);
                            self.status |= 0x01;
                        }
                        _ => {
                            // Unknown — ACK anyway
                            self.output_buffer.push_back(0xFA);
                            self.status |= 0x01;
                        }
                    }
                }
            }
            0x64 => {
                // Controller command
                match val {
                    0x20 => {
                        // Read configuration byte
                        self.output_buffer.push_back(self.config);
                        self.status |= 0x01;
                    }
                    0x60 => {
                        // Write configuration byte (next byte to port 0x60)
                        self.pending_cmd = Some(0x60);
                    }
                    0xA7 => {
                        // Disable second PS/2 port
                    }
                    0xA8 => {
                        // Enable second PS/2 port
                    }
                    0xAA => {
                        // Self-test: respond with 0x55 (pass)
                        self.output_buffer.push_back(0x55);
                        self.status |= 0x01;
                    }
                    0xAB => {
                        // Interface test: respond with 0x00 (pass)
                        self.output_buffer.push_back(0x00);
                        self.status |= 0x01;
                    }
                    0xAD => {
                        // Disable keyboard
                        self.keyboard_enabled = false;
                    }
                    0xAE => {
                        // Enable keyboard
                        self.keyboard_enabled = true;
                    }
                    0xD1 => {
                        // Write output port (next byte to port 0x60)
                        self.pending_cmd = Some(0xD1);
                    }
                    0xFE => {
                        // System reset — ignore for now
                        log::warn!("PS/2: System reset requested");
                    }
                    _ => {
                        log::trace!("PS/2: Unknown controller command: 0x{:02X}", val);
                    }
                }
            }
            _ => {}
        }
    }

    fn port_range(&self) -> (u16, u16) {
        (0x60, 0x64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ps2_self_test() {
        let mut ps2 = Ps2Controller::new();
        ps2.port_out(0x64, 1, 0xAA); // self-test
        let result = ps2.port_in(0x60, 1);
        assert_eq!(result, 0x55); // pass
    }

    #[test]
    fn test_ps2_scancode() {
        let mut ps2 = Ps2Controller::new();
        ps2.send_scancode(0x1E); // 'A' make code
        assert_eq!(ps2.port_in(0x64, 1) & 0x01, 1); // OBF set
        assert_eq!(ps2.port_in(0x60, 1), 0x1E);
        assert_eq!(ps2.port_in(0x64, 1) & 0x01, 0); // OBF cleared
    }

    #[test]
    fn test_ps2_keyboard_reset() {
        let mut ps2 = Ps2Controller::new();
        ps2.port_out(0x60, 1, 0xFF); // reset keyboard
        assert_eq!(ps2.port_in(0x60, 1), 0xFA); // ACK
        assert_eq!(ps2.port_in(0x60, 1), 0xAA); // BAT OK
    }

    #[test]
    fn test_ps2_config() {
        let mut ps2 = Ps2Controller::new();
        ps2.port_out(0x64, 1, 0x60); // write config command
        ps2.port_out(0x60, 1, 0x45); // new config
        assert_eq!(ps2.config, 0x45);

        ps2.port_out(0x64, 1, 0x20); // read config
        assert_eq!(ps2.port_in(0x60, 1), 0x45);
    }
}
