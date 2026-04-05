/// Intel 8253/8254 Programmable Interval Timer (PIT)
///
/// 3 channels:
/// - Channel 0: System timer (IRQ0) — 18.2 Hz default
/// - Channel 1: DRAM refresh (legacy, usually ignored)
/// - Channel 2: PC speaker
///
/// Ports:
/// - 0x40: Channel 0 data
/// - 0x41: Channel 1 data
/// - 0x42: Channel 2 data
/// - 0x43: Control/mode register
///
/// Base frequency: 1,193,182 Hz

use crate::port_bus::PortDevice;

pub const PIT_FREQUENCY: u32 = 1_193_182;

#[derive(Debug, Clone)]
struct PitChannel {
    /// Reload value (counter reset to this)
    reload: u16,
    /// Current counter value
    counter: u16,
    /// Operating mode (0-5)
    mode: u8,
    /// Access mode: 0=latch, 1=lobyte, 2=hibyte, 3=lo/hi
    access: u8,
    /// BCD mode (usually false)
    bcd: bool,
    /// Output pin state
    output: bool,
    /// Gate input (channel 2 only, controlled by port 0x61)
    gate: bool,
    /// Latch value for reading
    latched: Option<u16>,
    /// Which byte to read/write next (for lo/hi access)
    flip_flop: bool,
    /// Whether reload value has been fully written
    reload_ready: bool,
}

impl Default for PitChannel {
    fn default() -> Self {
        Self {
            reload: 0,
            counter: 0,
            mode: 0,
            access: 3, // lo/hi default
            bcd: false,
            output: false,
            gate: true, // gate high by default (except ch2)
            latched: None,
            flip_flop: false,
            reload_ready: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Pit8253 {
    channels: [PitChannel; 3],
    /// Accumulated ticks for timing
    ticks: u64,
    /// Whether channel 0 fired (IRQ0 pending)
    pub irq0_pending: bool,
}

impl Pit8253 {
    pub fn new() -> Self {
        Self {
            channels: [
                PitChannel::default(),
                PitChannel::default(),
                PitChannel { gate: false, ..PitChannel::default() },
            ],
            ticks: 0,
            irq0_pending: false,
        }
    }

    /// Tick the PIT by a number of base clock cycles.
    /// In practice, call this periodically (e.g., every N instructions).
    pub fn tick(&mut self, cycles: u32) {
        self.ticks += cycles as u64;

        for ch_idx in 0..3 {
            let ch = &mut self.channels[ch_idx];
            if !ch.gate || !ch.reload_ready {
                continue;
            }

            let reload = if ch.reload == 0 { 0x10000u32 } else { ch.reload as u32 };

            match ch.mode {
                0 => {
                    // Mode 0: Interrupt on terminal count
                    if ch.counter == 0 {
                        ch.output = true;
                        if ch_idx == 0 {
                            self.irq0_pending = true;
                        }
                    } else {
                        let sub = cycles.min(ch.counter as u32);
                        ch.counter -= sub as u16;
                    }
                }
                2 => {
                    // Mode 2: Rate generator (most common for system timer)
                    let mut remaining = cycles;
                    while remaining > 0 {
                        let sub = remaining.min(ch.counter as u32);
                        ch.counter -= sub as u16;
                        remaining -= sub;
                        if ch.counter == 0 {
                            ch.counter = reload as u16;
                            ch.output = true;
                            if ch_idx == 0 {
                                self.irq0_pending = true;
                            }
                        }
                    }
                }
                3 => {
                    // Mode 3: Square wave generator
                    let mut remaining = cycles;
                    while remaining > 0 {
                        let sub = remaining.min(ch.counter as u32);
                        ch.counter -= sub as u16;
                        remaining -= sub;
                        if ch.counter == 0 {
                            ch.counter = reload as u16;
                            ch.output = !ch.output;
                            if ch_idx == 0 {
                                self.irq0_pending = true;
                            }
                        }
                    }
                }
                _ => {
                    // Modes 1, 4, 5 — simplified: just decrement
                    if ch.counter > 0 {
                        let sub = cycles.min(ch.counter as u32);
                        ch.counter -= sub as u16;
                    }
                }
            }
        }
    }

    /// Check and clear IRQ0 pending flag
    pub fn check_irq0(&mut self) -> bool {
        let pending = self.irq0_pending;
        self.irq0_pending = false;
        pending
    }
}

impl PortDevice for Pit8253 {
    fn port_in(&mut self, port: u16, _size: u8) -> u32 {
        let ch_idx = (port - 0x40) as usize;
        if ch_idx >= 3 {
            return 0;
        }

        let ch = &mut self.channels[ch_idx];
        let val = ch.latched.unwrap_or(ch.counter);

        let byte = if ch.flip_flop || ch.access == 2 {
            ch.flip_flop = false;
            ch.latched = None;
            (val >> 8) as u8
        } else {
            if ch.access == 3 {
                ch.flip_flop = true;
            }
            val as u8
        };

        byte as u32
    }

    fn port_out(&mut self, port: u16, _size: u8, val: u32) {
        let val = val as u8;

        if port == 0x43 {
            // Control word
            let ch_idx = ((val >> 6) & 0x03) as usize;
            if ch_idx == 3 {
                // Read-back command (not implemented)
                return;
            }
            let access = (val >> 4) & 0x03;
            let mode = (val >> 1) & 0x07;
            let bcd = val & 0x01 != 0;

            if access == 0 {
                // Counter latch command
                self.channels[ch_idx].latched = Some(self.channels[ch_idx].counter);
                return;
            }

            let ch = &mut self.channels[ch_idx];
            ch.access = access;
            ch.mode = mode;
            ch.bcd = bcd;
            ch.flip_flop = false;
            ch.output = false;
            ch.reload_ready = false;
            return;
        }

        let ch_idx = (port - 0x40) as usize;
        if ch_idx >= 3 {
            return;
        }

        let ch = &mut self.channels[ch_idx];
        match ch.access {
            1 => {
                // Lobyte only
                ch.reload = (ch.reload & 0xFF00) | val as u16;
                ch.counter = ch.reload;
                ch.reload_ready = true;
            }
            2 => {
                // Hibyte only
                ch.reload = (ch.reload & 0x00FF) | ((val as u16) << 8);
                ch.counter = ch.reload;
                ch.reload_ready = true;
            }
            3 => {
                // Lo/Hi
                if !ch.flip_flop {
                    ch.reload = (ch.reload & 0xFF00) | val as u16;
                    ch.flip_flop = true;
                } else {
                    ch.reload = (ch.reload & 0x00FF) | ((val as u16) << 8);
                    ch.counter = ch.reload;
                    ch.flip_flop = false;
                    ch.reload_ready = true;
                }
            }
            _ => {}
        }
    }

    fn port_range(&self) -> (u16, u16) {
        (0x40, 0x43)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pit_init_and_read() {
        let mut pit = Pit8253::new();

        // Program channel 0: mode 2, lo/hi, reload = 0x1000
        pit.port_out(0x43, 1, 0x34); // ch0, lo/hi, mode 2
        pit.port_out(0x40, 1, 0x00); // low byte
        pit.port_out(0x40, 1, 0x10); // high byte -> reload = 0x1000

        assert_eq!(pit.channels[0].reload, 0x1000);
        assert_eq!(pit.channels[0].counter, 0x1000);
    }

    #[test]
    fn test_pit_mode2_irq() {
        let mut pit = Pit8253::new();

        // Channel 0: mode 2, reload = 100
        pit.port_out(0x43, 1, 0x34);
        pit.port_out(0x40, 1, 100);
        pit.port_out(0x40, 1, 0);

        // Tick 99 — not yet
        pit.tick(99);
        assert!(!pit.irq0_pending);

        // Tick 1 more — should fire
        pit.tick(1);
        assert!(pit.check_irq0());
    }

    #[test]
    fn test_pit_counter_latch() {
        let mut pit = Pit8253::new();

        pit.port_out(0x43, 1, 0x34);
        pit.port_out(0x40, 1, 0x00);
        pit.port_out(0x40, 1, 0x10); // reload = 0x1000

        pit.tick(0x100); // counter = 0x0F00

        // Latch counter
        pit.port_out(0x43, 1, 0x00); // latch channel 0

        let lo = pit.port_in(0x40, 1);
        let hi = pit.port_in(0x40, 1);
        let latched = (hi << 8) | lo;
        assert_eq!(latched, 0x0F00);
    }
}
