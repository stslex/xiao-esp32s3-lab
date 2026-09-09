//! Bounded, line-based USB protocol. Kept independent of ESP-IDF for host tests.
pub const PREFIX: &str = "XIAO1";
pub const MAX_LINE: usize = 256;
pub const MAX_FRAME: usize = 1024 * 1024;
pub const CHUNK_SIZE: usize = 256;

pub struct Credentials {
    pub ssid: String,
    pub password: String,
}

impl Credentials {
    pub fn from_hex(ssid: &str, password: &str) -> Result<Self, &'static str> {
        let ssid = String::from_utf8(decode_hex(ssid)?).map_err(|_| "invalid_utf8")?;
        let password = String::from_utf8(decode_hex(password)?).map_err(|_| "invalid_utf8")?;
        if ssid.is_empty() || ssid.len() > 32 || ssid.contains('\0') {
            return Err("ssid_must_be_1_to_32_bytes");
        }
        if !(8..=63).contains(&password.len()) || password.contains('\0') {
            return Err("password_must_be_8_to_63_bytes");
        }
        Ok(Self { ssid, password })
    }

    pub fn stored(&self) -> String {
        format!(
            "{} {}",
            encode_hex(self.ssid.as_bytes()),
            encode_hex(self.password.as_bytes())
        )
    }

    pub fn from_stored(value: &str) -> Result<Self, &'static str> {
        let (ssid, password) = value.split_once(' ').ok_or("invalid_credentials")?;
        Self::from_hex(ssid, password)
    }
}

pub enum Command {
    Status,
    Capture,
    WifiSet(Credentials),
    WifiForget,
    DisplayTest,
    DisplayGithub,
    GithubSet(String),
    GithubForget,
}

pub fn parse(line: &[u8]) -> Result<(u32, Command), &'static str> {
    if line.len() > MAX_LINE {
        return Err("command_too_long");
    }
    let line = std::str::from_utf8(line).map_err(|_| "invalid_command")?;
    let parts: Vec<_> = line.split_ascii_whitespace().collect();
    if parts.len() < 3 || parts[0] != PREFIX {
        return Err("invalid_command");
    }
    let id = parts[1].parse().map_err(|_| "invalid_request_id")?;
    let command = match parts[2..] {
        ["STATUS"] => Command::Status,
        ["CAPTURE"] => Command::Capture,
        ["WIFI_SET", ssid, password] => Command::WifiSet(Credentials::from_hex(ssid, password)?),
        ["WIFI_FORGET"] => Command::WifiForget,
        ["DISPLAY_TEST"] => Command::DisplayTest,
        ["DISPLAY_GITHUB"] => Command::DisplayGithub,
        ["GITHUB_SET", token] => Command::GithubSet(github_token(token)?),
        ["GITHUB_FORGET"] => Command::GithubForget,
        _ => return Err("invalid_command"),
    };
    Ok((id, command))
}

fn github_token(value: &str) -> Result<String, &'static str> {
    let token = String::from_utf8(decode_hex(value)?).map_err(|_| "invalid_token")?;
    if !token.starts_with("github_pat_")
        || !(50..=100).contains(&token.len())
        || !token
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        return Err("expected_fine_grained_github_token");
    }
    Ok(token)
}

#[derive(Default)]
pub struct LineReader {
    bytes: Vec<u8>,
    overflow: bool,
}

impl LineReader {
    pub fn push(&mut self, byte: u8) -> Option<Vec<u8>> {
        if byte == b'\n' {
            let line = if self.overflow {
                None
            } else {
                Some(std::mem::take(&mut self.bytes))
            };
            self.bytes.clear();
            self.overflow = false;
            return line;
        }
        if !self.overflow {
            if self.bytes.len() == MAX_LINE {
                self.bytes.clear();
                self.overflow = true;
            } else {
                self.bytes.push(byte);
            }
        }
        None
    }
}

pub fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 15) as usize] as char);
    }
    result
}

pub fn decode_hex(value: &str) -> Result<Vec<u8>, &'static str> {
    if value.len() % 2 != 0 || value.len() > MAX_LINE {
        return Err("invalid_hex");
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let hi = (pair[0] as char).to_digit(16).ok_or("invalid_hex")?;
            let lo = (pair[1] as char).to_digit(16).ok_or("invalid_hex")?;
            Ok(((hi << 4) | lo) as u8)
        })
        .collect()
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fine_grained_token_and_display_commands() {
        let token = format!("github_pat_{}", "a".repeat(82));
        let line = format!("XIAO1 9 GITHUB_SET {}", encode_hex(token.as_bytes()));
        assert!(line.len() <= MAX_LINE);
        assert!(
            matches!(parse(line.as_bytes()), Ok((9, Command::GithubSet(value))) if value == token)
        );
        assert!(parse(b"XIAO1 9 GITHUB_SET 0000").is_err());
        assert!(matches!(
            parse(b"XIAO1 9 DISPLAY_TEST"),
            Ok((9, Command::DisplayTest))
        ));
        assert!(matches!(
            parse(b"XIAO1 9 DISPLAY_GITHUB"),
            Ok((9, Command::DisplayGithub))
        ));
    }

    #[test]
    fn known_crc_vectors() {
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn fragmented_input_and_overflow_recover_at_newline() {
        let mut reader = LineReader::default();
        for _ in 0..MAX_LINE + 5 {
            assert!(reader.push(b'x').is_none());
        }
        assert!(reader.push(b'\n').is_none());
        let mut lines = Vec::new();
        for byte in b"XIAO1 7 CAPTURE\r\nXIAO1 8 STATUS\n" {
            if let Some(line) = reader.push(*byte) {
                lines.push(line);
            }
        }
        assert!(matches!(parse(&lines[0]), Ok((7, Command::Capture))));
        assert!(matches!(parse(&lines[1]), Ok((8, Command::Status))));
    }

    #[test]
    fn credentials_support_spaces_and_unicode_without_command_injection() {
        let ssid = "Home сеть";
        let password = "a b\nc def";
        let stored = format!(
            "{} {}",
            encode_hex(ssid.as_bytes()),
            encode_hex(password.as_bytes())
        );
        let credentials = Credentials::from_stored(&stored).unwrap();
        assert_eq!(credentials.ssid, ssid);
        assert_eq!(credentials.password, password);
        assert_eq!(credentials.stored(), stored);
    }

    #[test]
    fn malformed_and_out_of_bounds_commands_are_rejected() {
        for command in [
            "XIAO1 x STATUS",
            "XIAO1 4 STATUS extra",
            "XIAO1 4 WIFI_SET ff 12",
            "XIAO1 4 WIFI_SET 00 3132333435363738",
        ] {
            assert!(parse(command.as_bytes()).is_err());
        }
        assert!(
            Credentials::from_hex(&encode_hex("я".repeat(17).as_bytes()), "3132333435363738")
                .is_err()
        );
        assert!(decode_hex("xyz").is_err());
    }
}
