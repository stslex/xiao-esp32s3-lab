//! Raw GPIO numbers, NOT the D0..D10 labels on the XIAO PCB.
#[derive(Clone, Copy)]
pub struct Pins {
    pub cs: i32,
    pub dc: i32,
    pub reset: i32,
    pub mosi: i32,
    pub clock: i32,
}

// Owner-confirmed wiring: CS D0, DC D1, RST D3, SDA D10, SCL D8,
// VCC 3V3, GND GND. The Sense SD card interface is not initialized.
pub const PINS: Option<Pins> = Some(Pins {
    cs: 1,
    dc: 2,
    reset: 4,
    mosi: 9,
    clock: 7,
});

impl Pins {
    pub fn valid(self) -> bool {
        // Only exposed, output-capable XIAO pins not used by the Sense camera,
        // flash, PSRAM or USB. SD is not initialized by this firmware.
        let pins = [self.cs, self.dc, self.reset, self.mosi, self.clock];
        pins.iter()
            .all(|pin| [1, 2, 3, 4, 5, 6, 7, 8, 9, 43, 44].contains(pin))
            && pins
                .iter()
                .enumerate()
                .all(|(i, pin)| !pins[..i].contains(pin))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_duplicates_and_reserved_gpio() {
        let safe = Pins {
            cs: 1,
            dc: 2,
            reset: 4,
            mosi: 9,
            clock: 7,
        };
        assert!(safe.valid());
        assert!(!Pins { reset: 2, ..safe }.valid());
        assert!(!Pins { mosi: 10, ..safe }.valid());
        assert!(!Pins { clock: 19, ..safe }.valid());
    }
}
