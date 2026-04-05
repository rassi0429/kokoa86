//! Serial (COM1) device tests

use kokoa86_dev::port_bus::PortDevice;
use kokoa86_dev::Serial8250;

#[test]
fn test_serial_transmit() {
    let mut serial = Serial8250::new_capture(0x3F8);

    // Write "Hi" to THR (port 0x3F8)
    serial.port_out(0x3F8, 1, b'H' as u32);
    serial.port_out(0x3F8, 1, b'i' as u32);

    assert_eq!(serial.output(), b"Hi");
}

#[test]
fn test_serial_lsr_always_ready() {
    let mut serial = Serial8250::new_capture(0x3F8);

    // LSR (port 0x3FD) should report transmitter empty
    let lsr = serial.port_in(0x3FD, 1);
    assert_eq!(lsr & 0x60, 0x60); // THRE + TEMT
}

#[test]
fn test_serial_scratch_register() {
    let mut serial = Serial8250::new_capture(0x3F8);

    serial.port_out(0x3FF, 1, 0x42);
    assert_eq!(serial.port_in(0x3FF, 1), 0x42);
}

#[test]
fn test_serial_port_range() {
    let serial = Serial8250::new_capture(0x3F8);
    assert_eq!(serial.port_range(), (0x3F8, 0x3FF));
}
