/// Intel 8259 Programmable Interrupt Controller (PIC)
///
/// Standard PC has two cascaded PICs:
/// - Master PIC: ports 0x20-0x21, IRQ 0-7
/// - Slave PIC:  ports 0xA0-0xA1, IRQ 8-15
///
/// Slave is connected to Master's IRQ2.

use crate::port_bus::PortDevice;

#[derive(Debug, Clone)]
pub struct Pic8259 {
    base_port: u16,
    /// Interrupt vector offset (set via ICW2)
    pub vector_offset: u8,
    /// Interrupt Mask Register (IMR) — 1 = masked
    imr: u8,
    /// Interrupt Request Register (IRR) — pending interrupts
    irr: u8,
    /// In-Service Register (ISR) — currently being serviced
    isr: u8,
    /// ICW state machine
    icw_step: u8,
    icw4_needed: bool,
    /// Is this the master or slave?
    is_master: bool,
    /// Auto EOI mode
    auto_eoi: bool,
    /// Read IRR (false) or ISR (true) on port read
    read_isr: bool,
}

impl Pic8259 {
    pub fn new(base_port: u16, is_master: bool) -> Self {
        Self {
            base_port,
            vector_offset: if is_master { 0x08 } else { 0x70 },
            imr: 0xFF, // all masked initially
            irr: 0,
            isr: 0,
            icw_step: 0,
            icw4_needed: false,
            is_master,
            auto_eoi: false,
            read_isr: false,
        }
    }

    /// Raise an IRQ line
    pub fn raise_irq(&mut self, irq: u8) {
        self.irr |= 1 << irq;
    }

    /// Lower an IRQ line
    pub fn lower_irq(&mut self, irq: u8) {
        self.irr &= !(1 << irq);
    }

    /// Get the highest priority interrupt ready to be serviced.
    /// Returns Some(vector_number) or None.
    pub fn get_interrupt(&mut self) -> Option<u8> {
        let pending = self.irr & !self.imr;
        if pending == 0 {
            return None;
        }
        // Find lowest bit (highest priority)
        let irq = pending.trailing_zeros() as u8;
        // Check if higher-priority interrupt is in service
        let in_service_mask = (1u8 << irq) - 1;
        if self.isr & in_service_mask != 0 {
            return None; // Higher priority interrupt in service
        }
        // Acknowledge
        self.irr &= !(1 << irq);
        self.isr |= 1 << irq;
        if self.auto_eoi {
            self.isr &= !(1 << irq);
        }
        Some(self.vector_offset + irq)
    }

    /// Check if any interrupt is pending (without acknowledging)
    pub fn has_interrupt(&self) -> bool {
        (self.irr & !self.imr) != 0
    }
}

impl PortDevice for Pic8259 {
    fn port_in(&mut self, port: u16, _size: u8) -> u32 {
        let offset = port - self.base_port;
        match offset {
            0 => {
                if self.read_isr {
                    self.isr as u32
                } else {
                    self.irr as u32
                }
            }
            1 => self.imr as u32,
            _ => 0,
        }
    }

    fn port_out(&mut self, port: u16, _size: u8, val: u32) {
        let offset = port - self.base_port;
        let val = val as u8;

        match offset {
            0 => {
                if val & 0x10 != 0 {
                    // ICW1: initialization command
                    self.icw_step = 1;
                    self.icw4_needed = val & 0x01 != 0;
                    self.imr = 0;
                    self.isr = 0;
                    self.irr = 0;
                    self.auto_eoi = false;
                } else if val & 0x08 != 0 {
                    // OCW3
                    if val & 0x02 != 0 {
                        self.read_isr = val & 0x01 != 0;
                    }
                } else {
                    // OCW2: EOI commands
                    let cmd = (val >> 5) & 0x07;
                    match cmd {
                        1 => {
                            // Non-specific EOI: clear highest priority ISR bit
                            if self.isr != 0 {
                                let bit = self.isr.trailing_zeros() as u8;
                                self.isr &= !(1 << bit);
                            }
                        }
                        3 => {
                            // Specific EOI
                            let irq = val & 0x07;
                            self.isr &= !(1 << irq);
                        }
                        _ => {}
                    }
                }
            }
            1 => {
                match self.icw_step {
                    1 => {
                        // ICW2: vector offset
                        self.vector_offset = val & 0xF8;
                        self.icw_step = if self.is_master { 2 } else { 2 };
                    }
                    2 => {
                        // ICW3: cascade configuration (ignore details)
                        self.icw_step = if self.icw4_needed { 3 } else { 0 };
                    }
                    3 => {
                        // ICW4: mode
                        self.auto_eoi = val & 0x02 != 0;
                        self.icw_step = 0;
                    }
                    _ => {
                        // OCW1: set IMR
                        self.imr = val;
                    }
                }
            }
            _ => {}
        }
    }

    fn port_range(&self) -> (u16, u16) {
        (self.base_port, self.base_port + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pic_init_sequence() {
        let mut pic = Pic8259::new(0x20, true);

        // ICW1
        pic.port_out(0x20, 1, 0x11); // edge, cascade, ICW4 needed
        // ICW2: vector offset 0x20
        pic.port_out(0x21, 1, 0x20);
        // ICW3: slave on IRQ2
        pic.port_out(0x21, 1, 0x04);
        // ICW4: 8086 mode
        pic.port_out(0x21, 1, 0x01);

        assert_eq!(pic.vector_offset, 0x20);
        assert_eq!(pic.imr, 0x00); // cleared during init
    }

    #[test]
    fn test_pic_mask_and_irq() {
        let mut pic = Pic8259::new(0x20, true);
        // Init
        pic.port_out(0x20, 1, 0x11);
        pic.port_out(0x21, 1, 0x20);
        pic.port_out(0x21, 1, 0x04);
        pic.port_out(0x21, 1, 0x01);

        // Mask all except IRQ0
        pic.port_out(0x21, 1, 0xFE);

        // Raise IRQ0
        pic.raise_irq(0);
        assert!(pic.has_interrupt());

        let vec = pic.get_interrupt();
        assert_eq!(vec, Some(0x20)); // vector_offset + 0
    }

    #[test]
    fn test_pic_masked_irq_ignored() {
        let mut pic = Pic8259::new(0x20, true);
        pic.port_out(0x20, 1, 0x11);
        pic.port_out(0x21, 1, 0x20);
        pic.port_out(0x21, 1, 0x04);
        pic.port_out(0x21, 1, 0x01);

        // Mask all
        pic.port_out(0x21, 1, 0xFF);

        pic.raise_irq(0);
        assert!(!pic.has_interrupt());
        assert_eq!(pic.get_interrupt(), None);
    }

    #[test]
    fn test_pic_eoi() {
        let mut pic = Pic8259::new(0x20, true);
        pic.port_out(0x20, 1, 0x11);
        pic.port_out(0x21, 1, 0x20);
        pic.port_out(0x21, 1, 0x04);
        pic.port_out(0x21, 1, 0x01);
        pic.port_out(0x21, 1, 0x00); // unmask all

        pic.raise_irq(0);
        let _ = pic.get_interrupt();
        assert_eq!(pic.isr, 0x01); // IRQ0 in service

        // Non-specific EOI
        pic.port_out(0x20, 1, 0x20);
        assert_eq!(pic.isr, 0x00);
    }
}
