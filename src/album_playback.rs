//! Playback time advances only while the album is visible and not paused.
#[derive(Debug)]
pub struct Playback {
    pub current: Option<usize>,
    pub pending: Option<usize>,
    pub paused: bool,
    pub revision: u64,
    pub remaining_ms: u64,
    interval_ms: u64,
    clock_ms: u64,
}
impl Playback {
    pub fn new(interval_seconds: u64, clock_ms: u64) -> Self {
        Self {
            current: None,
            pending: None,
            paused: false,
            revision: 0,
            remaining_ms: interval_seconds * 1000,
            interval_ms: interval_seconds * 1000,
            clock_ms,
        }
    }
    pub fn tick(&mut self, clock_ms: u64) {
        if !self.paused && self.current.is_some() {
            self.remaining_ms = self
                .remaining_ms
                .saturating_sub(clock_ms.saturating_sub(self.clock_ms));
        }
        self.clock_ms = clock_ms;
    }
    pub fn pause(&mut self, paused: bool, clock_ms: u64) {
        self.tick(clock_ms);
        if self.paused != paused || (paused && self.pending.is_some()) {
            self.paused = paused;
            self.revision += 1;
            // Pause freezes the displayed photo, including an in-flight next request.
            if paused {
                self.pending = None;
            }
        }
    }
    pub fn step(&mut self, direction: i32, count: usize, clock_ms: u64) {
        self.tick(clock_ms);
        if count == 0 {
            return;
        }
        let base = self.pending.or(self.current).unwrap_or(0);
        self.pending = Some(if direction < 0 {
            (base + count - 1) % count
        } else {
            (base + 1) % count
        });
        self.revision += 1;
    }
    pub fn target(&mut self, clock_ms: u64, count: usize) -> Option<usize> {
        self.tick(clock_ms);
        if count == 0 {
            return None;
        }
        if let Some(index) = self.pending {
            return Some(index % count);
        }
        match self.current {
            None if !self.paused => Some(0),
            Some(index) if !self.paused && self.remaining_ms == 0 => Some((index + 1) % count),
            _ => None,
        }
    }
    pub fn displayed(&mut self, index: usize, clock_ms: u64) {
        self.current = Some(index);
        self.pending = None;
        self.remaining_ms = self.interval_ms;
        self.clock_ms = clock_ms;
    }
    pub fn interval(&mut self, seconds: u64, clock_ms: u64) {
        self.tick(clock_ms);
        self.interval_ms = seconds * 1000;
        self.remaining_ms = self.interval_ms;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paused_and_hidden_time_does_not_skip_current_photo() {
        let mut p = Playback::new(60, 0);
        assert_eq!(p.target(0, 4), Some(0));
        p.displayed(0, 0);
        p.tick(20_000);
        p.pause(true, 20_000);
        assert_eq!(p.target(920_000, 4), None);
        assert_eq!(p.remaining_ms, 40_000);
        p.pause(false, 920_000);
        // A hidden display supplies the same active clock, however long it stays hidden.
        assert_eq!(p.target(920_000, 4), None);
        assert_eq!(p.current, Some(0));
        assert_eq!(p.target(959_999, 4), None);
        assert_eq!(p.target(960_000, 4), Some(1));
    }
    #[test]
    fn previous_wraps_and_rapid_commands_supersede_pending_downloads() {
        let mut p = Playback::new(15, 0);
        p.displayed(0, 0);
        p.pause(true, 1000);
        let before = p.revision;
        p.step(-1, 4, 2000);
        assert_eq!(p.target(2000, 4), Some(3));
        p.step(-1, 4, 2001);
        assert_eq!(p.target(2001, 4), Some(2));
        p.step(1, 4, 2002);
        assert_eq!(p.target(2002, 4), Some(3));
        assert!(p.revision > before);
        p.displayed(3, 3000);
        assert_eq!(p.target(90_000, 4), None);
        assert!(p.paused);
        p.step(1, 4, 90_001);
        assert_eq!(p.target(90_001, 4), Some(0));
    }
    #[test]
    fn pause_cancels_pending_step_and_interval_change_keeps_position() {
        let mut p = Playback::new(60, 0);
        p.displayed(2, 0);
        p.step(1, 5, 5000);
        let ticket = p.revision;
        p.pause(true, 6000);
        assert_ne!(p.revision, ticket);
        assert_eq!(p.target(100_000, 5), None);
        p.interval(15, 100_000);
        assert_eq!(p.current, Some(2));
        assert!(p.paused);
        p.pause(false, 100_000);
        assert_eq!(p.target(114_999, 5), None);
        assert_eq!(p.target(115_000, 5), Some(3));
    }
}
