//! Two flash slots: commit a CRC-protected header only after the new image is written.
use crate::{framebuffer::BYTE_LEN, image_layout::Image, protocol::crc32};

pub const SLOT_SIZE: usize = 0x26000;
const HEADER: usize = 32;
const MAGIC: &[u8; 8] = b"XIAOPH02";

fn header_crc(h: &[u8; HEADER]) -> u32 {
    if &h[..8] == b"XIAOPH01" {
        return crc32(&h[..20]);
    }
    let mut bytes = h[..20].to_vec();
    bytes.extend_from_slice(&h[24..]);
    crc32(&bytes)
}

pub trait Storage {
    fn read(&mut self, offset: usize, data: &mut [u8]) -> Result<(), &'static str>;
    fn write(&mut self, offset: usize, data: &[u8]) -> Result<(), &'static str>;
    fn erase(&mut self, offset: usize, length: usize) -> Result<(), &'static str>;
}

pub struct PhotoStore<S> {
    storage: S,
    current: Option<(usize, u32)>,
}

impl<S: Storage> PhotoStore<S> {
    pub fn new(storage: S) -> Self {
        Self {
            storage,
            current: None,
        }
    }

    fn read_slot(&mut self, slot: usize) -> Result<Option<(u32, Image)>, &'static str> {
        let mut h = [0u8; HEADER];
        self.storage.read(slot * SLOT_SIZE, &mut h)?;
        let legacy = &h[..8] == b"XIAOPH01";
        let length = u32::from_le_bytes(h[12..16].try_into().unwrap()) as usize;
        let (width, height) = if legacy {
            (240, 320)
        } else {
            (
                u16::from_le_bytes(h[24..26].try_into().unwrap()) as usize,
                u16::from_le_bytes(h[26..28].try_into().unwrap()) as usize,
            )
        };
        if (!legacy && &h[..8] != MAGIC)
            || length > BYTE_LEN
            || width == 0
            || height == 0
            || width > 640
            || height > 640
            || length != width * height * 2
            || header_crc(&h) != u32::from_le_bytes(h[20..24].try_into().unwrap())
        {
            return Ok(None);
        }
        let mut frame = vec![0u8; length];
        self.storage.read(slot * SLOT_SIZE + HEADER, &mut frame)?;
        if crc32(&frame) != u32::from_le_bytes(h[16..20].try_into().unwrap()) {
            return Ok(None);
        }
        Ok(Some((
            u32::from_le_bytes(h[8..12].try_into().unwrap()),
            Image {
                pixels: frame,
                width,
                height,
                legacy_padding: legacy || h[28] == 1,
            },
        )))
    }

    pub fn load(&mut self) -> Result<Option<Image>, &'static str> {
        let mut found = None;
        self.current = None;
        for slot in 0..2 {
            if let Some((generation, frame)) = self.read_slot(slot)? {
                if self
                    .current
                    .is_none_or(|(_, previous)| generation.wrapping_sub(previous) as i32 > 0)
                {
                    self.current = Some((slot, generation));
                    found = Some(frame);
                }
            }
        }
        Ok(found)
    }

    pub fn save(&mut self, image: &Image) -> Result<(), &'static str> {
        let frame = &image.pixels;
        if frame.len() > BYTE_LEN
            || frame.is_empty()
            || frame.len() != image.width * image.height * 2
            || image.width > 640
            || image.height > 640
        {
            return Err("invalid_photo_size");
        }
        let (slot, generation) = self
            .current
            .map(|(s, g)| (1 - s, g.wrapping_add(1)))
            .unwrap_or((0, 1));
        let base = slot * SLOT_SIZE;
        self.storage.erase(base, SLOT_SIZE)?;
        self.storage.write(base + HEADER, frame)?;
        let mut h = [0u8; HEADER];
        h[..8].copy_from_slice(MAGIC);
        h[8..12].copy_from_slice(&generation.to_le_bytes());
        h[12..16].copy_from_slice(&(frame.len() as u32).to_le_bytes());
        h[16..20].copy_from_slice(&crc32(frame).to_le_bytes());
        h[24..26].copy_from_slice(&(image.width as u16).to_le_bytes());
        h[26..28].copy_from_slice(&(image.height as u16).to_le_bytes());
        h[28] = image.legacy_padding as u8;
        let header_crc = header_crc(&h);
        h[20..24].copy_from_slice(&header_crc.to_le_bytes());
        self.storage.write(base, &h)?;
        if self
            .read_slot(slot)?
            .is_none_or(|(_, saved)| saved != *image)
        {
            return Err("photo_flash_verify_failed");
        }
        self.current = Some((slot, generation));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Memory {
        bytes: Vec<u8>,
        fail: Option<usize>,
    }
    impl Storage for Memory {
        fn read(&mut self, o: usize, b: &mut [u8]) -> Result<(), &'static str> {
            b.copy_from_slice(&self.bytes[o..o + b.len()]);
            Ok(())
        }
        fn write(&mut self, o: usize, b: &[u8]) -> Result<(), &'static str> {
            let n = self.fail.map(|n| n.min(b.len())).unwrap_or(b.len());
            self.bytes[o..o + n].copy_from_slice(&b[..n]);
            if self.fail.is_some() {
                Err("power_loss")
            } else {
                Ok(())
            }
        }
        fn erase(&mut self, o: usize, n: usize) -> Result<(), &'static str> {
            self.bytes[o..o + n].fill(255);
            Ok(())
        }
    }
    #[test]
    fn interrupted_image_write_preserves_previous_photo() {
        let mut s = PhotoStore::new(Memory {
            bytes: vec![255; SLOT_SIZE * 2],
            fail: None,
        });
        assert!(s.load().unwrap().is_none());
        let first = Image::portrait(vec![17; BYTE_LEN]).unwrap();
        s.save(&first).unwrap();
        s.storage.fail = Some(7000);
        let second = Image::new(vec![99; 320 * 180 * 2], 320, 180).unwrap();
        assert!(s.save(&second).is_err());
        let mut reboot = PhotoStore::new(s.storage);
        assert_eq!(reboot.load().unwrap().unwrap(), first);
        reboot.storage.fail = None;
        reboot.save(&second).unwrap();
        assert_eq!(reboot.load().unwrap().unwrap(), second);
        reboot.storage.bytes[SLOT_SIZE + HEADER + 5] ^= 1;
        assert_eq!(reboot.load().unwrap().unwrap(), first);
    }
    #[test]
    fn legacy_photo_remains_readable_and_dimensions_are_protected() {
        let mut storage = Memory {
            bytes: vec![255; SLOT_SIZE * 2],
            fail: None,
        };
        let pixels = vec![42; BYTE_LEN];
        let mut h = [0u8; HEADER];
        h[..8].copy_from_slice(b"XIAOPH01");
        h[8..12].copy_from_slice(&1u32.to_le_bytes());
        h[12..16].copy_from_slice(&(BYTE_LEN as u32).to_le_bytes());
        h[16..20].copy_from_slice(&crc32(&pixels).to_le_bytes());
        let crc = header_crc(&h);
        h[20..24].copy_from_slice(&crc.to_le_bytes());
        storage.write(0, &h).unwrap();
        storage.write(HEADER, &pixels).unwrap();
        let mut store = PhotoStore::new(storage);
        let old = store.load().unwrap().unwrap();
        assert_eq!(old.pixels, pixels);
        assert!(old.legacy_padding);
        let new = Image::new(vec![13; 320 * 180 * 2], 320, 180).unwrap();
        store.save(&new).unwrap();
        assert_eq!(store.load().unwrap().unwrap(), new);
        store.storage.bytes[SLOT_SIZE + 24] ^= 1;
        assert_eq!(store.load().unwrap().unwrap(), old);
    }
}
