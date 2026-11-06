//! Decodificador yEnc en streaming (línea a línea).

#[derive(Debug)]
pub enum YencError {
    NotYenc,
    Truncated,
}

#[derive(Default)]
pub struct YencDecoder {
    pub data: Vec<u8>,
    pub name: Option<String>,
    pub total_size: Option<u64>,
    pub part: Option<u32>,
    pub begin: Option<u64>,
    pub end: Option<u64>,
    expected_crc: Option<u32>,
    state: u8, // 0: esperando =ybegin, 1: datos, 2: terminado
}

fn kv<'a>(line: &'a [u8], key: &str) -> Option<&'a str> {
    let s = std::str::from_utf8(line).ok()?;
    for tok in s.split_ascii_whitespace() {
        if let Some(v) = tok.strip_prefix(key) {
            if let Some(v) = v.strip_prefix('=') {
                return Some(v);
            }
        }
    }
    None
}

fn parse_crc(v: &str) -> Option<u32> {
    let v = v.trim().trim_start_matches("0x");
    u32::from_str_radix(v, 16).ok()
}

impl YencDecoder {
    pub fn new() -> Self {
        Self { data: Vec::with_capacity(800 * 1024), ..Default::default() }
    }

    pub fn reset(&mut self) {
        self.data.clear();
        self.name = None;
        self.total_size = None;
        self.part = None;
        self.begin = None;
        self.end = None;
        self.expected_crc = None;
        self.state = 0;
    }

    /// Procesa una línea ya sin CRLF y sin dot-stuffing.
    pub fn feed_line(&mut self, line: &[u8]) {
        match self.state {
            0 => {
                if line.starts_with(b"=ybegin ") {
                    self.part = kv(line, "part").and_then(|v| v.parse().ok());
                    self.total_size = kv(line, "size").and_then(|v| v.parse().ok());
                    if let Some(pos) = find(line, b" name=") {
                        let n = String::from_utf8_lossy(&line[pos + 6..]).trim().to_string();
                        if !n.is_empty() {
                            self.name = Some(n);
                        }
                    }
                    self.state = 1;
                }
            }
            1 => {
                if line.starts_with(b"=ypart ") {
                    self.begin = kv(line, "begin").and_then(|v| v.parse().ok());
                    self.end = kv(line, "end").and_then(|v| v.parse().ok());
                } else if line.starts_with(b"=yend") {
                    let pcrc = kv(line, "pcrc32").and_then(parse_crc);
                    let crc = kv(line, "crc32").and_then(parse_crc);
                    self.expected_crc = if self.part.is_some() || self.begin.is_some() { pcrc } else { pcrc.or(crc) };
                    self.state = 2;
                } else {
                    self.decode_line(line);
                }
            }
            _ => {}
        }
    }

    #[inline]
    fn decode_line(&mut self, line: &[u8]) {
        let start = self.data.len();
        self.data.resize(start + line.len(), 0);
        let out = &mut self.data[start..];
        let mut o = 0;
        let mut i = 0;
        let n = line.len();
        while i < n {
            let c = line[i];
            if c == b'=' {
                i += 1;
                if i < n {
                    out[o] = line[i].wrapping_sub(106);
                    o += 1;
                }
            } else {
                out[o] = c.wrapping_sub(42);
                o += 1;
            }
            i += 1;
        }
        self.data.truncate(start + o);
    }

    /// Devuelve Ok(crc_valido).
    pub fn finish(&self) -> Result<bool, YencError> {
        if self.state == 0 {
            return Err(YencError::NotYenc);
        }
        if self.state == 1 && self.data.is_empty() {
            return Err(YencError::Truncated);
        }
        Ok(match self.expected_crc {
            Some(c) => crc32fast::hash(&self.data) == c,
            None => self.state == 2,
        })
    }

    /// Desplazamiento (0-based) en el archivo final.
    pub fn offset(&self) -> u64 {
        self.begin.map(|b| b.saturating_sub(1)).unwrap_or(0)
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for &b in data {
            let e = b.wrapping_add(42);
            if matches!(e, 0 | b'\n' | b'\r' | b'=') {
                out.push(b'=');
                out.push(e.wrapping_add(64));
            } else {
                out.push(e);
            }
        }
        out
    }

    #[test]
    fn roundtrip() {
        let data: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
        let crc = crc32fast::hash(&data);
        let mut d = YencDecoder::new();
        d.feed_line(b"=ybegin part=2 total=10 line=128 size=50000 name=my file.bin");
        d.feed_line(b"=ypart begin=5001 end=10000");
        for chunk in data.chunks(128) {
            d.feed_line(&encode(chunk));
        }
        d.feed_line(format!("=yend size=5000 part=2 pcrc32={:08x}", crc).as_bytes());
        assert!(d.finish().unwrap());
        assert_eq!(d.data, data);
        assert_eq!(d.offset(), 5000);
        assert_eq!(d.name.as_deref(), Some("my file.bin"));
    }
}
