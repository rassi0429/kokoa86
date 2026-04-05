/// VGA text mode emulation
///
/// Memory-mapped at 0xB8000-0xB8FFF (text mode buffer)
/// 80x25 characters, each character is 2 bytes: [char, attribute]
///
/// Attribute byte: [blink(1) | bg(3) | fg(4)]

pub const VGA_TEXT_BASE: u32 = 0xB8000;
pub const VGA_TEXT_SIZE: u32 = 0x1000; // 4KB
pub const VGA_COLS: usize = 80;
pub const VGA_ROWS: usize = 25;

/// VGA text mode state
#[derive(Clone)]
pub struct VgaText {
    /// Text buffer: 80*25*2 = 4000 bytes (char + attr pairs)
    pub buffer: Vec<u8>,
    /// Cursor position
    pub cursor_x: u8,
    pub cursor_y: u8,
    /// Miscellaneous output register
    pub misc_output: u8,
    /// CRT controller registers
    pub crtc_index: u8,
    pub crtc_regs: [u8; 25],
    /// Attribute controller
    pub attr_index: u8,
    pub attr_regs: [u8; 21],
    /// Sequencer
    pub seq_index: u8,
    pub seq_regs: [u8; 5],
    /// Graphics controller
    pub gc_index: u8,
    pub gc_regs: [u8; 9],
    /// DAC (palette)
    pub dac_read_index: u8,
    pub dac_write_index: u8,
    pub dac_component: u8, // 0=R, 1=G, 2=B
    pub dac_palette: [[u8; 3]; 256], // RGB entries
}

impl VgaText {
    pub fn new() -> Self {
        let mut vga = Self {
            buffer: vec![0; VGA_COLS * VGA_ROWS * 2],
            cursor_x: 0,
            cursor_y: 0,
            misc_output: 0x63,
            crtc_index: 0,
            crtc_regs: [0; 25],
            attr_index: 0,
            attr_regs: [0; 21],
            seq_index: 0,
            seq_regs: [0; 5],
            gc_index: 0,
            gc_regs: [0; 9],
            dac_read_index: 0,
            dac_write_index: 0,
            dac_component: 0,
            dac_palette: [[0; 3]; 256],
        };
        // Initialize default 16-color text mode palette
        vga.init_default_palette();
        vga
    }

    fn init_default_palette(&mut self) {
        // Standard CGA 16-color palette (6-bit VGA values)
        let colors: [[u8; 3]; 16] = [
            [0x00, 0x00, 0x00], // 0: Black
            [0x00, 0x00, 0x2A], // 1: Blue
            [0x00, 0x2A, 0x00], // 2: Green
            [0x00, 0x2A, 0x2A], // 3: Cyan
            [0x2A, 0x00, 0x00], // 4: Red
            [0x2A, 0x00, 0x2A], // 5: Magenta
            [0x2A, 0x15, 0x00], // 6: Brown
            [0x2A, 0x2A, 0x2A], // 7: Light Gray
            [0x15, 0x15, 0x15], // 8: Dark Gray
            [0x15, 0x15, 0x3F], // 9: Light Blue
            [0x15, 0x3F, 0x15], // A: Light Green
            [0x15, 0x3F, 0x3F], // B: Light Cyan
            [0x3F, 0x15, 0x15], // C: Light Red
            [0x3F, 0x15, 0x3F], // D: Light Magenta
            [0x3F, 0x3F, 0x15], // E: Yellow
            [0x3F, 0x3F, 0x3F], // F: White
        ];
        for (i, c) in colors.iter().enumerate() {
            self.dac_palette[i] = *c;
        }
    }

    /// Read from VGA text buffer (memory-mapped)
    pub fn mem_read(&self, offset: u32) -> u8 {
        if (offset as usize) < self.buffer.len() {
            self.buffer[offset as usize]
        } else {
            0
        }
    }

    /// Write to VGA text buffer (memory-mapped)
    pub fn mem_write(&mut self, offset: u32, val: u8) {
        if (offset as usize) < self.buffer.len() {
            self.buffer[offset as usize] = val;
        }
    }

    /// Write a character at cursor position (used by BIOS stub INT 0x10)
    pub fn put_char(&mut self, ch: u8, attr: u8) {
        match ch {
            0x0A => {
                // Line feed
                self.cursor_y += 1;
                if self.cursor_y >= VGA_ROWS as u8 {
                    self.scroll_up();
                    self.cursor_y = (VGA_ROWS - 1) as u8;
                }
            }
            0x0D => {
                // Carriage return
                self.cursor_x = 0;
            }
            0x08 => {
                // Backspace
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                }
            }
            _ => {
                let offset = ((self.cursor_y as usize) * VGA_COLS + self.cursor_x as usize) * 2;
                if offset + 1 < self.buffer.len() {
                    self.buffer[offset] = ch;
                    self.buffer[offset + 1] = attr;
                }
                self.cursor_x += 1;
                if self.cursor_x >= VGA_COLS as u8 {
                    self.cursor_x = 0;
                    self.cursor_y += 1;
                    if self.cursor_y >= VGA_ROWS as u8 {
                        self.scroll_up();
                        self.cursor_y = (VGA_ROWS - 1) as u8;
                    }
                }
            }
        }
    }

    fn scroll_up(&mut self) {
        let row_bytes = VGA_COLS * 2;
        self.buffer.copy_within(row_bytes.., 0);
        // Clear last row
        let start = (VGA_ROWS - 1) * row_bytes;
        for i in 0..row_bytes {
            self.buffer[start + i] = if i % 2 == 0 { 0x20 } else { 0x07 };
        }
    }

    /// Get the text buffer as a grid of (char, fg_rgb, bg_rgb) for rendering
    pub fn render_cells(&self) -> Vec<(char, [u8; 3], [u8; 3])> {
        let mut cells = Vec::with_capacity(VGA_COLS * VGA_ROWS);
        for row in 0..VGA_ROWS {
            for col in 0..VGA_COLS {
                let offset = (row * VGA_COLS + col) * 2;
                let ch = self.buffer[offset];
                let attr = self.buffer[offset + 1];
                let fg_idx = (attr & 0x0F) as usize;
                let bg_idx = ((attr >> 4) & 0x07) as usize;
                let fg = self.palette_to_rgb(fg_idx);
                let bg = self.palette_to_rgb(bg_idx);
                let c = if ch >= 0x20 && ch < 0x7F {
                    ch as char
                } else if ch == 0 {
                    ' '
                } else {
                    '\u{25A1}' // placeholder for non-printable
                };
                cells.push((c, fg, bg));
            }
        }
        cells
    }

    /// Convert a palette index to 8-bit RGB
    fn palette_to_rgb(&self, idx: usize) -> [u8; 3] {
        let [r, g, b] = self.dac_palette[idx];
        // VGA DAC uses 6-bit values (0-63), scale to 8-bit
        [(r << 2) | (r >> 4), (g << 2) | (g >> 4), (b << 2) | (b >> 4)]
    }

    /// Handle VGA I/O port reads
    pub fn port_in(&mut self, port: u16) -> u8 {
        match port {
            0x3C0 => self.attr_index,
            0x3C1 => {
                let idx = (self.attr_index & 0x1F) as usize;
                if idx < self.attr_regs.len() { self.attr_regs[idx] } else { 0 }
            }
            0x3C4 => self.seq_index,
            0x3C5 => {
                let idx = self.seq_index as usize;
                if idx < self.seq_regs.len() { self.seq_regs[idx] } else { 0 }
            }
            0x3C7 => 0x03, // DAC state: write mode
            0x3C8 => self.dac_write_index,
            0x3C9 => {
                let idx = self.dac_read_index as usize;
                let comp = self.dac_component as usize;
                let val = self.dac_palette[idx][comp];
                self.dac_component += 1;
                if self.dac_component >= 3 {
                    self.dac_component = 0;
                    self.dac_read_index = self.dac_read_index.wrapping_add(1);
                }
                val
            }
            0x3CC => self.misc_output,
            0x3CE => self.gc_index,
            0x3CF => {
                let idx = self.gc_index as usize;
                if idx < self.gc_regs.len() { self.gc_regs[idx] } else { 0 }
            }
            0x3D4 => self.crtc_index,
            0x3D5 => {
                let idx = self.crtc_index as usize;
                if idx < self.crtc_regs.len() { self.crtc_regs[idx] } else { 0 }
            }
            0x3DA => {
                // Input Status Register 1: toggle vertical retrace bit
                0x00 // simplified: always report not in retrace
            }
            _ => 0,
        }
    }

    /// Handle VGA I/O port writes
    pub fn port_out(&mut self, port: u16, val: u8) {
        match port {
            0x3C0 => self.attr_index = val,
            0x3C2 => self.misc_output = val,
            0x3C4 => self.seq_index = val,
            0x3C5 => {
                let idx = self.seq_index as usize;
                if idx < self.seq_regs.len() { self.seq_regs[idx] = val; }
            }
            0x3C7 => {
                self.dac_read_index = val;
                self.dac_component = 0;
            }
            0x3C8 => {
                self.dac_write_index = val;
                self.dac_component = 0;
            }
            0x3C9 => {
                let idx = self.dac_write_index as usize;
                let comp = self.dac_component as usize;
                if idx < 256 && comp < 3 {
                    self.dac_palette[idx][comp] = val;
                }
                self.dac_component += 1;
                if self.dac_component >= 3 {
                    self.dac_component = 0;
                    self.dac_write_index = self.dac_write_index.wrapping_add(1);
                }
            }
            0x3CE => self.gc_index = val,
            0x3CF => {
                let idx = self.gc_index as usize;
                if idx < self.gc_regs.len() { self.gc_regs[idx] = val; }
            }
            0x3D4 => self.crtc_index = val,
            0x3D5 => {
                let idx = self.crtc_index as usize;
                if idx < self.crtc_regs.len() {
                    self.crtc_regs[idx] = val;
                }
                // Track cursor position from CRTC registers
                if self.crtc_index == 0x0E || self.crtc_index == 0x0F {
                    let cursor_pos =
                        ((self.crtc_regs[0x0E] as u16) << 8) | self.crtc_regs[0x0F] as u16;
                    self.cursor_x = (cursor_pos % VGA_COLS as u16) as u8;
                    self.cursor_y = (cursor_pos / VGA_COLS as u16) as u8;
                }
            }
            _ => {}
        }
    }
}

impl Default for VgaText {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_put_char() {
        let mut vga = VgaText::new();
        vga.put_char(b'A', 0x07);
        assert_eq!(vga.buffer[0], b'A');
        assert_eq!(vga.buffer[1], 0x07);
        assert_eq!(vga.cursor_x, 1);
        assert_eq!(vga.cursor_y, 0);
    }

    #[test]
    fn test_newline() {
        let mut vga = VgaText::new();
        vga.put_char(b'A', 0x07);
        vga.put_char(0x0D, 0x07); // CR
        vga.put_char(0x0A, 0x07); // LF
        assert_eq!(vga.cursor_x, 0);
        assert_eq!(vga.cursor_y, 1);
    }

    #[test]
    fn test_scroll() {
        let mut vga = VgaText::new();
        // Fill screen and force scroll
        for _ in 0..VGA_ROWS {
            vga.put_char(b'X', 0x07);
            vga.put_char(0x0A, 0x07);
        }
        assert_eq!(vga.cursor_y, (VGA_ROWS - 1) as u8);
    }

    #[test]
    fn test_mem_mapped_write() {
        let mut vga = VgaText::new();
        vga.mem_write(0, b'H');
        vga.mem_write(1, 0x0F); // white on black
        vga.mem_write(2, b'i');
        vga.mem_write(3, 0x0F);

        let cells = vga.render_cells();
        assert_eq!(cells[0].0, 'H');
        assert_eq!(cells[1].0, 'i');
    }

    #[test]
    fn test_render_default_palette() {
        let vga = VgaText::new();
        // White text (0x0F) should give bright white RGB
        let rgb = vga.palette_to_rgb(0x0F);
        assert_eq!(rgb, [0xFF, 0xFF, 0xFF]);
        // Black (0x00) should give black
        let rgb = vga.palette_to_rgb(0x00);
        assert_eq!(rgb, [0x00, 0x00, 0x00]);
    }
}
