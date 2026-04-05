/// Trait for devices that handle I/O port access
pub trait PortDevice {
    fn port_in(&mut self, port: u16, size: u8) -> u32;
    fn port_out(&mut self, port: u16, size: u8, val: u32);
    fn port_range(&self) -> (u16, u16); // (start, end) inclusive
}

/// I/O port bus with O(1) dispatch via flat lookup table
pub struct PortBus {
    devices: Vec<Box<dyn PortDevice>>,
    /// Maps port number -> index into devices Vec (None = unhandled)
    map: Vec<Option<usize>>,
}

impl PortBus {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            map: vec![None; 65536],
        }
    }

    pub fn register(&mut self, device: Box<dyn PortDevice>) {
        let idx = self.devices.len();
        let (start, end) = device.port_range();
        for port in start..=end {
            self.map[port as usize] = Some(idx);
        }
        self.devices.push(device);
    }

    pub fn port_in(&mut self, port: u16, size: u8) -> u32 {
        if let Some(idx) = self.map[port as usize] {
            self.devices[idx].port_in(port, size)
        } else {
            log::trace!("Unhandled port IN: 0x{:04X}", port);
            0xFF // Default: all bits high (common for absent hardware)
        }
    }

    pub fn port_out(&mut self, port: u16, size: u8, val: u32) {
        if let Some(idx) = self.map[port as usize] {
            self.devices[idx].port_out(port, size, val);
        } else {
            log::trace!("Unhandled port OUT: 0x{:04X} = 0x{:X}", port, val);
        }
    }
}
