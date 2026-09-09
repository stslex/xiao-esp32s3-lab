//! Source pixels remain uncropped; orientation and framing are applied when drawing.
use crate::framebuffer::{BYTE_LEN, HEIGHT, WIDTH};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub pixels: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub legacy_padding: bool,
}
impl Image {
    pub fn new(pixels: Vec<u8>, width: usize, height: usize) -> Result<Self, &'static str> {
        if width == 0
            || height == 0
            || width > 640
            || height > 640
            || pixels.len() != width * height * 2
        {
            return Err("invalid_source_image_dimensions");
        }
        Ok(Self {
            pixels,
            width,
            height,
            legacy_padding: false,
        })
    }
    pub fn portrait(pixels: Vec<u8>) -> Result<Self, &'static str> {
        Self::new(pixels, WIDTH, HEIGHT)
    }

    fn bounds(&self) -> (usize, usize, usize, usize) {
        if !self.legacy_padding {
            return (0, 0, self.width, self.height);
        }
        // Older uploads already contained symmetric black letterboxing. Ignore
        // those margins for reframing, retaining the original saved bytes.
        let black_row = |y: usize| {
            self.pixels[y * self.width * 2..(y + 1) * self.width * 2]
                .iter()
                .all(|b| *b == 0)
        };
        let top = (0..self.height).take_while(|y| black_row(*y)).count();
        let bottom = (0..self.height).rev().take_while(|y| black_row(*y)).count();
        if top > 0 && bottom > 0 && top.abs_diff(bottom) <= 1 && top + bottom < self.height {
            return (0, top, self.width, self.height - top - bottom);
        }
        let black_col = |x: usize| {
            (0..self.height).all(|y| {
                self.pixels[(y * self.width + x) * 2..(y * self.width + x) * 2 + 2] == [0, 0]
            })
        };
        let left = (0..self.width).take_while(|x| black_col(*x)).count();
        let right = (0..self.width).rev().take_while(|x| black_col(*x)).count();
        if left > 0 && right > 0 && left.abs_diff(right) <= 1 && left + right < self.width {
            return (left, 0, self.width - left - right, self.height);
        }
        (0, 0, self.width, self.height)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub rotation: u16,
    pub cover: bool,
    pub focus_x: usize,
    pub focus_y: usize,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            rotation: 0,
            cover: false,
            focus_x: 50,
            focus_y: 50,
        }
    }
}
impl Layout {
    pub fn dimensions(self) -> (usize, usize) {
        if self.rotation == 0 {
            (240, 320)
        } else {
            (320, 240)
        }
    }
    pub fn json(self) -> Value {
        json!({"rotation":self.rotation,"fit":if self.cover {"cover"} else {"contain"},"focus_x":self.focus_x,"focus_y":self.focus_y})
    }
    pub fn patch(self, value: &Value) -> Result<Self, &'static str> {
        let fields = value
            .as_object()
            .filter(|v| !v.is_empty())
            .ok_or("expected_display_layout_object")?;
        let mut next = self;
        for (key, value) in fields {
            match key.as_str() {
                "rotation" => {
                    next.rotation = match value.as_u64() {
                        Some(0) => 0,
                        Some(90) => 90,
                        Some(270) => 270,
                        _ => return Err("unsupported_display_rotation"),
                    }
                }
                "fit" => {
                    next.cover = match value.as_str() {
                        Some("contain") => false,
                        Some("cover") => true,
                        _ => return Err("unsupported_image_fit"),
                    }
                }
                "focus_x" | "focus_y" => {
                    let n = value
                        .as_u64()
                        .filter(|n| *n <= 100)
                        .ok_or("crop_position_must_be_0_to_100")?
                        as usize;
                    if key == "focus_x" {
                        next.focus_x = n;
                    } else {
                        next.focus_y = n;
                    }
                }
                _ => return Err("unknown_display_layout_field"),
            }
        }
        Ok(next)
    }

    pub fn render(self, image: &Image, photo: bool) -> Vec<u8> {
        let (tw, th) = self.dimensions();
        let (mut sx, mut sy, mut sw, mut sh) = if photo {
            image.bounds()
        } else {
            (0, 0, image.width, image.height)
        };
        let (mut dw, mut dh) = (tw, th);
        if self.cover && photo {
            if sw * th > sh * tw {
                let crop = (sh * tw / th).max(1);
                sx += (sw - crop) * self.focus_x / 100;
                sw = crop;
            } else {
                let crop = (sw * th / tw).max(1);
                sy += (sh - crop) * self.focus_y / 100;
                sh = crop;
            }
        } else if sw * th > sh * tw {
            dh = (sh * tw / sw).max(1);
        } else {
            dw = (sw * th / sh).max(1);
        }
        let (dx, dy) = ((tw - dw) / 2, (th - dh) / 2);
        let mut output = vec![0; BYTE_LEN];
        for y in 0..dh {
            let row = (sy + y * sh / dh) * image.width;
            for x in 0..dw {
                let src = (row + sx + x * sw / dw) * 2;
                let dst = ((dy + y) * tw + dx + x) * 2;
                output[dst..dst + 2].copy_from_slice(&image.pixels[src..src + 2]);
            }
        }
        output
    }

    pub fn physical(self, logical: &[u8]) -> Vec<u8> {
        if self.rotation == 0 {
            return logical.to_vec();
        }
        let mut output = vec![0; BYTE_LEN];
        for y in 0..240 {
            for x in 0..320 {
                let (px, py) = if self.rotation == 90 {
                    (239 - y, x)
                } else {
                    (y, 319 - x)
                };
                let src = (y * 320 + x) * 2;
                let dst = (py * 240 + px) * 2;
                output[dst..dst + 2].copy_from_slice(&logical[src..src + 2]);
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pixel(b: &[u8], w: usize, x: usize, y: usize) -> u16 {
        u16::from_be_bytes(b[(y * w + x) * 2..(y * w + x) * 2 + 2].try_into().unwrap())
    }
    #[test]
    fn fit_crop_focus_and_rotation_preserve_known_pixels() {
        let mut pixels = vec![];
        for _ in 0..2 {
            for c in [0xf800u16, 0x07e0, 0x001f, 0xffff] {
                pixels.extend_from_slice(&c.to_be_bytes());
            }
        }
        let source = Image::new(pixels.clone(), 4, 2).unwrap();
        let fit = Layout::default().render(&source, true);
        assert_eq!(pixel(&fit, 240, 0, 0), 0);
        assert_eq!(pixel(&fit, 240, 0, 100), 0xf800);
        let left = Layout {
            cover: true,
            focus_x: 0,
            ..Layout::default()
        }
        .render(&source, true);
        let right = Layout {
            cover: true,
            focus_x: 100,
            ..Layout::default()
        }
        .render(&source, true);
        assert_eq!(pixel(&left, 240, 239, 319), 0xf800);
        assert_eq!(pixel(&right, 240, 0, 0), 0xffff);
        assert_eq!(source.pixels, pixels);
        let mut corners = vec![0; BYTE_LEN];
        for (x, y, c) in [(0, 0, 1u16), (319, 0, 2), (0, 239, 3), (319, 239, 4)] {
            corners[(y * 320 + x) * 2..(y * 320 + x) * 2 + 2].copy_from_slice(&c.to_be_bytes());
        }
        let cw = Layout {
            rotation: 90,
            ..Layout::default()
        }
        .physical(&corners);
        assert_eq!(
            [
                pixel(&cw, 240, 0, 0),
                pixel(&cw, 240, 239, 0),
                pixel(&cw, 240, 0, 319),
                pixel(&cw, 240, 239, 319)
            ],
            [3, 1, 4, 2]
        );
        let ccw = Layout {
            rotation: 270,
            ..Layout::default()
        }
        .physical(&corners);
        assert_eq!(
            [
                pixel(&ccw, 240, 0, 0),
                pixel(&ccw, 240, 239, 0),
                pixel(&ccw, 240, 0, 319),
                pixel(&ccw, 240, 239, 319)
            ],
            [2, 4, 1, 3]
        );
        assert!(Layout::default().patch(&json!({"rotation":180})).is_err());
        assert!(Layout::default().patch(&json!({"focus_y":101})).is_err());
    }
}
