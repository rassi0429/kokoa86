use crate::port_bus::PortDevice;
use std::io::{self, Write};

/// 8250 UART (COM1) — Phase 1: transmit-only
///
/// Ports 0x3F8-0x3FF:
///   0x3F8 (THR/RBR): Transmit/Receive buffer
///   0x3F9 (IER): Interrupt Enable Register
///   0x3FA (IIR): Interrupt Identification Register
///   0x3FB (LCR): Line Control Register
///   0x3FC (MCR): Modem Control Register
///   0x3FD (LSR): Line Status Register
///   0x3FE (MSR): Modem Status Register
///   0x3FF (SCR): Scratch Register
pub struct Serial8250 {
    base_port: u16,
    /// Line Status Register
    lsr: u8,
    /// Line Control Register
    lcr: u8,
    /// Interrupt Enable Register
    ier: u8,
    /// Modem Control Register
    mcr: u8,
    /// Scratch Register
    scratch: u8,
    /// Collected output for testing
    output_buffer: Vec<u8>,
    /// Whether to write to stdout
    write_stdout: bool,
}

impl Serial8250 {
    pub fn new(base_port: u16) -> Self {
        Self {
            base_port,
            lsr: 0x60, // THRE + TEMT (transmitter empty and ready)
            lcr: 0,
            ier: 0,
            mcr: 0,
            scratch: 0,
            output_buffer: Vec::new(),
            write_stdout: true,
        }
    }

    /// Create a serial port that captures output (for testing)
    pub fn new_capture(base_port: u16) -> Self {
        Self {
            write_stdout: false,
            ..Self::new(base_port)
        }
    }

    /// Get captured output as string
    pub fn output(&self) -> &[u8] {
        &self.output_buffer
    }
}

impl PortDevice for Serial8250 {
    fn port_in(&mut self, port: u16, _size: u8) -> u32 {
        let offset = port - self.base_port;
        match offset {
            0 => 0, // RBR: no input in Phase 1
            1 => self.ier as u32,
            2 => 0x01, // IIR: no interrupt pending
            3 => self.lcr as u32,
            4 => self.mcr as u32,
            5 => self.lsr as u32, // LSR: always report transmitter empty
            6 => 0, // MSR
            7 => self.scratch as u32,
            _ => 0,
        }
    }

    fn port_out(&mut self, port: u16, _size: u8, val: u32) {
        let offset = port - self.base_port;
        match offset {
            0 => {
                // THR: transmit byte
                let byte = val as u8;
                self.output_buffer.push(byte);
                if self.write_stdout {
                    let _ = io::stdout().write_all(&[byte]);
                    let _ = io::stdout().flush();
                }
            }
            1 => self.ier = val as u8,
            3 => self.lcr = val as u8,
            4 => self.mcr = val as u8,
            7 => self.scratch = val as u8,
            _ => {}
        }
    }

    fn port_range(&self) -> (u16, u16) {
        (self.base_port, self.base_port + 7)
    }
}
