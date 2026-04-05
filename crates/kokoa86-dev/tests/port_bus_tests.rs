//! Port bus tests

use kokoa86_dev::port_bus::{PortBus, PortDevice};

struct TestDevice {
    base: u16,
    value: u32,
}

impl PortDevice for TestDevice {
    fn port_in(&mut self, _port: u16, _size: u8) -> u32 {
        self.value
    }
    fn port_out(&mut self, _port: u16, _size: u8, val: u32) {
        self.value = val;
    }
    fn port_range(&self) -> (u16, u16) {
        (self.base, self.base + 3)
    }
}

#[test]
fn test_port_bus_register_and_dispatch() {
    let mut bus = PortBus::new();
    bus.register(Box::new(TestDevice { base: 0x100, value: 0x42 }));

    assert_eq!(bus.port_in(0x100, 1), 0x42);
    assert_eq!(bus.port_in(0x103, 1), 0x42); // within range

    // Unregistered port returns 0xFF
    assert_eq!(bus.port_in(0x200, 1), 0xFF);
}

#[test]
fn test_port_bus_write_then_read() {
    let mut bus = PortBus::new();
    bus.register(Box::new(TestDevice { base: 0x300, value: 0 }));

    bus.port_out(0x300, 1, 0xAB);
    assert_eq!(bus.port_in(0x300, 1), 0xAB);
}

#[test]
fn test_port_bus_multiple_devices() {
    let mut bus = PortBus::new();
    bus.register(Box::new(TestDevice { base: 0x100, value: 0x11 }));
    bus.register(Box::new(TestDevice { base: 0x200, value: 0x22 }));

    assert_eq!(bus.port_in(0x100, 1), 0x11);
    assert_eq!(bus.port_in(0x200, 1), 0x22);
}
