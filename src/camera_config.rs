//! Camera controls and JPEG header validation, independent of ESP-IDF.
use serde_json::{json, Value};

pub const MAX_JPEG: usize = 1024 * 1024;
pub const RESOLUTIONS: [(&str, usize, usize); 7] = [
    ("QVGA", 320, 240),
    ("VGA", 640, 480),
    ("SVGA", 800, 600),
    ("XGA", 1024, 768),
    ("SXGA", 1280, 1024),
    ("UXGA", 1600, 1200),
    ("QXGA", 2048, 1536),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub resolution: usize,
    pub quality: i32,
    pub brightness: i32,
    pub contrast: i32,
    pub saturation: i32,
    pub hmirror: bool,
    pub vflip: bool,
}

impl Settings {
    pub const DEFAULT: Self = Self {
        resolution: 2,
        quality: 12,
        brightness: 0,
        contrast: 0,
        saturation: 0,
        hmirror: false,
        vflip: true,
    };

    pub fn json(self) -> Value {
        let (name, width, height) = RESOLUTIONS[self.resolution];
        json!({"framesize": name, "width": width, "height": height, "quality": self.quality,
            "brightness": self.brightness, "contrast": self.contrast, "saturation": self.saturation,
            "hmirror": self.hmirror, "vflip": self.vflip})
    }

    pub fn patch(self, value: &Value) -> Result<Self, &'static str> {
        let fields = value.as_object().ok_or("expected_camera_settings_object")?;
        if fields.is_empty() {
            return Err("empty_camera_settings");
        }
        let mut next = self;
        for (key, value) in fields {
            match key.as_str() {
                "framesize" => {
                    next.resolution = RESOLUTIONS
                        .iter()
                        .position(|(name, _, _)| Some(*name) == value.as_str())
                        .ok_or("unsupported_camera_resolution")?;
                }
                "quality" => next.quality = integer(value, 10, 40)?,
                "brightness" => next.brightness = integer(value, -2, 2)?,
                "contrast" => next.contrast = integer(value, -2, 2)?,
                "saturation" => next.saturation = integer(value, -2, 2)?,
                "hmirror" => next.hmirror = value.as_bool().ok_or("expected_camera_boolean")?,
                "vflip" => next.vflip = value.as_bool().ok_or("expected_camera_boolean")?,
                // Read-only dimensions must never be accepted as a settings write.
                _ => return Err("unknown_camera_setting"),
            }
        }
        Ok(next)
    }

    pub fn stored(self) -> String {
        json!({"framesize": RESOLUTIONS[self.resolution].0, "quality": self.quality,
            "brightness": self.brightness, "contrast": self.contrast, "saturation": self.saturation,
            "hmirror": self.hmirror, "vflip": self.vflip})
        .to_string()
    }
}

fn integer(value: &Value, min: i64, max: i64) -> Result<i32, &'static str> {
    value
        .as_i64()
        .filter(|value| (min..=max).contains(value))
        .map(|value| value as i32)
        .ok_or("camera_setting_out_of_range")
}

pub fn jpeg_dimensions(bytes: &[u8]) -> Option<(usize, usize)> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut offset = 2;
    while offset + 4 <= bytes.len() {
        if bytes[offset] != 0xff {
            return None;
        }
        while bytes.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *bytes.get(offset)?;
        offset += 1;
        if marker == 0xda || marker == 0xd9 {
            return None;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let length = u16::from_be_bytes([*bytes.get(offset)?, *bytes.get(offset + 1)?]) as usize;
        if length < 2 || offset + length > bytes.len() {
            return None;
        }
        if matches!(marker, 0xc0 | 0xc1 | 0xc2) {
            if length < 8 {
                return None;
            }
            let height = u16::from_be_bytes([bytes[offset + 3], bytes[offset + 4]]) as usize;
            let width = u16::from_be_bytes([bytes[offset + 5], bytes[offset + 6]]) as usize;
            return (width > 0 && height > 0).then_some((width, height));
        }
        offset += length;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_invalid_batch_without_changing_previous_settings() {
        let initial = Settings::DEFAULT;
        for value in [
            json!({"framesize":"UNKNOWN"}),
            json!({"quality":8}),
            json!({"quality":9}),
            json!({"brightness":3}),
            json!({"hmirror":1}),
            json!({"quality":10.5}),
            json!({"width":1}),
            json!({"quality":10,"unexpected":true}),
        ] {
            assert!(initial.patch(&value).is_err());
        }
        assert_eq!(initial.quality, 12);
        let next = initial
            .patch(&json!({"framesize":"QXGA","quality":10,"hmirror":true}))
            .unwrap();
        assert_eq!(RESOLUTIONS[next.resolution], ("QXGA", 2048, 1536));
        assert!(next.hmirror);
        assert_eq!(next.quality, 10);
    }
    #[test]
    fn jpeg_parser_checks_segments_and_actual_dimensions() {
        let jpeg = [
            0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0, 0xff, 0xc0, 0, 8, 8, 6, 0, 8, 0, 0,
        ];
        assert_eq!(jpeg_dimensions(&jpeg), Some((2048, 1536)));
        for n in 0..jpeg.len() {
            assert_eq!(jpeg_dimensions(&jpeg[..n]), None);
        }
        let mut bad = jpeg;
        bad[11] = 0;
        assert_eq!(jpeg_dimensions(&bad), None);
    }
    #[test]
    fn stored_settings_round_trip() {
        let settings = Settings::DEFAULT
            .patch(&json!({"brightness":-2,"framesize":"UXGA"}))
            .unwrap();
        let saved: Value = serde_json::from_str(&settings.stored()).unwrap();
        assert!(Settings::DEFAULT.patch(&saved).unwrap() == settings);
    }
}
