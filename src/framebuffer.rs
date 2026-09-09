//! RGB565 pixels in wire (big-endian) order, independent of ESP-IDF.
pub const WIDTH: usize = 240;
pub const HEIGHT: usize = 320;
pub const BYTE_LEN: usize = WIDTH * HEIGHT * 2;

pub struct Framebuffer(pub Vec<u8>);

impl Framebuffer {
    pub fn new(background: u16) -> Self {
        let mut frame = Self(vec![0; BYTE_LEN]);
        frame.rect(0, 0, WIDTH, HEIGHT, background);
        frame
    }

    pub fn pixel(&mut self, x: usize, y: usize, color: u16) {
        if x < WIDTH && y < HEIGHT {
            let offset = (y * WIDTH + x) * 2;
            self.0[offset..offset + 2].copy_from_slice(&color.to_be_bytes());
        }
    }

    pub fn rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: u16) {
        for row in y..y.saturating_add(height).min(HEIGHT) {
            for col in x..x.saturating_add(width).min(WIDTH) {
                self.pixel(col, row, color);
            }
        }
    }

    pub fn text(&mut self, x: usize, y: usize, text: &str, scale: usize, color: u16) {
        for (index, character) in text.to_ascii_uppercase().chars().enumerate() {
            for (row, bits) in glyph(character).iter().enumerate() {
                for col in 0..5 {
                    if bits & (1 << (4 - col)) != 0 {
                        self.rect(
                            x + (index * 6 + col) * scale,
                            y + row * scale,
                            scale,
                            scale,
                            color,
                        );
                    }
                }
            }
        }
    }

    pub fn diagnostic() -> Self {
        let mut frame = Self::new(rgb(12, 18, 28));
        frame.text(18, 22, "XIAO", 4, rgb(255, 255, 255));
        frame.text(18, 64, "DISPLAY READY", 2, rgb(91, 226, 169));
        for y in 100..236 {
            for x in 16..224 {
                frame.pixel(
                    x,
                    y,
                    rgb(
                        ((x - 16) * 255 / 207) as u8,
                        ((y - 100) * 255 / 135) as u8,
                        130,
                    ),
                );
            }
        }
        for (index, color) in [rgb(255, 0, 0), rgb(0, 255, 0), rgb(0, 0, 255)]
            .iter()
            .enumerate()
        {
            frame.rect(16 + index * 70, 250, 68, 22, *color);
        }
        frame.text(16, 287, "240 X 320 / RGB", 2, rgb(208, 219, 233));
        frame
    }
}

pub const fn rgb(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 & 0xf8) << 8) | ((g as u16 & 0xfc) << 3) | (b as u16 >> 3)
}

// Original 5x7 bitmap glyphs for the small on-device status dashboard.
fn glyph(c: char) -> [u8; 7] {
    match c {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        '/' => [1, 1, 2, 4, 8, 16, 16],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '.' => [0, 0, 0, 0, 0, 4, 4],
        ':' => [0, 4, 4, 0, 4, 4, 0],
        '@' => [14, 17, 23, 21, 23, 16, 14],
        ' ' => [0; 7],
        _ => [14, 17, 1, 2, 4, 0, 4],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_order_and_clipping() {
        let mut frame = Framebuffer::new(0);
        frame.rect(WIDTH - 1, HEIGHT - 1, 100, 100, rgb(255, 0, 0));
        assert_eq!(frame.0.len(), BYTE_LEN);
        assert_eq!(&frame.0[BYTE_LEN - 2..], &[0xf8, 0x00]);
        assert!(frame.0[..BYTE_LEN - 2].iter().all(|b| *b == 0));
        assert_eq!(rgb(0, 255, 0), 0x07e0);
        assert_eq!(rgb(0, 0, 255), 0x001f);
    }
}
