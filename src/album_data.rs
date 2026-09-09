//! Bounded parsing of the public Google Photos shared-album page (not a stable API).
use serde_json::Value;
pub const MAX_DATA: usize = 256 * 1024;
const MARKER: &[u8] = b"AF_initDataCallback({key: 'ds:1'";

pub fn album_url(url: &str) -> bool {
    (url.starts_with("https://photos.app.goo.gl/")
        || url.starts_with("https://photos.google.com/share/"))
        && url.len() <= 1024
        && url
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-._~:/?=&%".contains(&c))
}
pub fn image_url(url: &str) -> bool {
    ["lh3", "lh4", "lh5", "lh6"]
        .iter()
        .any(|host| url.starts_with(&format!("https://{host}.googleusercontent.com/")))
        && url.len() <= 2048
        && url
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-._~:/?=&%".contains(&c))
}

#[derive(Default)]
pub struct Document {
    buffer: Vec<u8>,
    started: bool,
}
impl Document {
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Option<Value>, &'static str> {
        self.buffer.extend_from_slice(bytes);
        if !self.started {
            if let Some(at) = self.buffer.windows(MARKER.len()).position(|w| w == MARKER) {
                self.buffer.drain(..at);
                self.started = true;
            } else {
                let keep = self.buffer.len().saturating_sub(MARKER.len());
                self.buffer.drain(..keep);
                return Ok(None);
            }
        }
        if self.buffer.len() > MAX_DATA {
            return Err("album_data_too_large");
        }
        if let Some(end) = self.buffer.windows(14).position(|w| w == b", sideChannel:") {
            let start = self
                .buffer
                .windows(5)
                .position(|w| w == b"data:")
                .ok_or("album_page_format_changed")?
                + 5;
            let value = serde_json::from_slice(&self.buffer[start..end])
                .map_err(|_| "album_page_format_changed")?;
            return Ok(Some(value));
        }
        Ok(None)
    }
}

pub fn media(value: &Value) -> Result<(Vec<String>, bool), &'static str> {
    let items = value
        .get(1)
        .and_then(Value::as_array)
        .ok_or("album_page_format_changed")?;
    let mut urls = Vec::new();
    for item in items.iter().take(200) {
        if let Some(url) = item.get(1).and_then(|a| a.get(0)).and_then(Value::as_str) {
            if image_url(url) && !urls.iter().any(|s| s == url) {
                urls.push(url.to_owned());
            }
        }
    }
    if urls.is_empty() {
        return Err("album_empty_or_not_public");
    }
    let partial = items.len() > 200
        || value
            .get(2)
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty());
    Ok((urls, partial))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_across_every_chunk_boundary_and_reject_external_hosts() {
        let html = b"<script>prefix AF_initDataCallback({key: 'ds:1', hash:'2', data:[null,[[\"id\",[\"https://lh3.googleusercontent.com/pw/photo\",200,300]],[\"bad\",[\"http://192.168.1.1/private\",1,1]]],\"\"], sideChannel: {}});</script>";
        for split in 1..html.len() {
            let mut d = Document::default();
            let value = d
                .feed(&html[..split])
                .unwrap()
                .or_else(|| d.feed(&html[split..]).unwrap())
                .unwrap();
            let (urls, partial) = media(&value).unwrap();
            assert_eq!(urls, ["https://lh3.googleusercontent.com/pw/photo"]);
            assert!(!partial);
        }
        assert!(!album_url("https://photos.app.goo.gl.evil.org/a"));
        assert!(!album_url("https://photos.app.goo.gl/a\r\nHost: evil"));
        assert!(!image_url("https://lh3.googleusercontent.com@evil.org/x"));
    }
}
