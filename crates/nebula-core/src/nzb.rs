use regex::Regex;
use std::collections::HashSet;
use std::io::Read;
use std::sync::LazyLock;

#[derive(Debug, Clone)]
pub struct NzbSegment {
    pub number: u32,
    pub bytes: u64,
    pub message_id: String,
}

#[derive(Debug, Clone)]
pub struct NzbFile {
    pub filename: String,
    pub date: u64,
    pub groups: Vec<String>,
    pub segments: Vec<NzbSegment>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Nzb {
    pub title: Option<String>,
    pub password: Option<String>,
    pub category: Option<String>,
    pub files: Vec<NzbFile>,
}

static RE_VOL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.vol\d+[+-](\d+)\.par2$").unwrap());
static RE_QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([^"]+)""#).unwrap());
static RE_FNAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([^\s\[\]\(\)<>]+\.[A-Za-z0-9]{2,5}(?:\.\d{3})?)(?:\s|$|\))").unwrap());

impl NzbFile {
    pub fn is_par2(&self) -> bool {
        self.filename.to_ascii_lowercase().ends_with(".par2")
    }
    /// Número de bloques de recuperación si es un volumen PAR2 (name.vol07+08.par2).
    pub fn vol_blocks(&self) -> Option<u32> {
        RE_VOL.captures(&self.filename).and_then(|c| c[1].parse().ok())
    }
    /// ¿Es un volumen de recuperación? Además de los `name.volNN+MM.par2`, cuenta como volumen
    /// cualquier `.par2` grande: los NZB ofuscados nombran los volúmenes como `hash.par2`, y el
    /// índice (solo sumas de control) casi nunca pasa de unos cientos de KB.
    pub fn is_recovery_vol(&self) -> bool {
        self.is_par2() && (self.vol_blocks().is_some() || self.bytes > PAR2_INDEX_MAX)
    }
}

/// Tamaño máximo que se espera de un PAR2 índice; por encima se trata como volumen.
pub const PAR2_INDEX_MAX: u64 = 2 * 1024 * 1024;

pub fn vol_blocks_of(name: &str) -> Option<u32> {
    RE_VOL.captures(name).and_then(|c| c[1].parse().ok())
}

/// Obtiene el nombre de archivo a partir del "subject" de Usenet.
pub fn filename_from_subject(subject: &str) -> String {
    if let Some(c) = RE_QUOTED.captures(subject) {
        let s = c[1].trim();
        if !s.is_empty() {
            return crate::util::sanitize_filename(s);
        }
    }
    // Quitar el sufijo "yEnc (1/50)" y buscar algo con pinta de nombre de archivo
    let cut = subject.find("yEnc").map(|i| &subject[..i]).unwrap_or(subject);
    if let Some(m) = RE_FNAME.captures_iter(cut).last() {
        return crate::util::sanitize_filename(&m[1]);
    }
    crate::util::sanitize_filename(cut.trim())
}

fn decode_text(data: &[u8]) -> String {
    let data = data.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(data);
    match std::str::from_utf8(data) {
        Ok(s) => s.to_string(),
        // Latin-1 como alternativa
        Err(_) => data.iter().map(|&b| b as char).collect(),
    }
}

pub fn maybe_gunzip(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() > 2 && data[0] == 0x1f && data[1] == 0x8b {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(data)
            .read_to_end(&mut out)
            .map_err(|e| t!("nzb.gzCorrupt", error = e))?;
        Ok(out)
    } else {
        Ok(data.to_vec())
    }
}

pub fn looks_like_nzb(data: &[u8]) -> bool {
    let head = &data[..data.len().min(4096)];
    let s = String::from_utf8_lossy(head).to_ascii_lowercase();
    s.contains("<nzb")
}

pub fn parse(raw: &[u8]) -> Result<Nzb, String> {
    let data = maybe_gunzip(raw)?;
    let text = decode_text(&data);
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(&text, opts)
        .map_err(|e| t!("nzb.invalidXml", error = e))?;
    let root = doc.root_element();
    if !root.tag_name().name().eq_ignore_ascii_case("nzb") {
        return Err(t!("nzb.notNzb"));
    }
    let mut nzb = Nzb::default();
    for node in root.children().filter(|n| n.is_element()) {
        match node.tag_name().name() {
            "head" => {
                for meta in node.children().filter(|n| n.tag_name().name() == "meta") {
                    let kind = meta.attribute("type").unwrap_or("").to_ascii_lowercase();
                    let value = meta.text().unwrap_or("").trim().to_string();
                    if value.is_empty() {
                        continue;
                    }
                    match kind.as_str() {
                        "title" | "name" => nzb.title = Some(value),
                        "password" => nzb.password = Some(value),
                        "category" => nzb.category = Some(value),
                        _ => {}
                    }
                }
            }
            "file" => {
                let subject = node.attribute("subject").unwrap_or("").to_string();
                let date = node.attribute("date").and_then(|d| d.parse().ok()).unwrap_or(0);
                let mut groups = Vec::new();
                let mut segments = Vec::new();
                for child in node.children().filter(|n| n.is_element()) {
                    match child.tag_name().name() {
                        "groups" => {
                            for g in child.children().filter(|n| n.is_element()) {
                                if let Some(t) = g.text() {
                                    groups.push(t.trim().to_string());
                                }
                            }
                        }
                        "segments" => {
                            for s in child.children().filter(|n| n.is_element()) {
                                let id = s.text().unwrap_or("").trim();
                                if id.is_empty() {
                                    continue;
                                }
                                let id = id.trim_start_matches('<').trim_end_matches('>').to_string();
                                segments.push(NzbSegment {
                                    number: s.attribute("number").and_then(|n| n.parse().ok()).unwrap_or(0),
                                    bytes: s.attribute("bytes").and_then(|n| n.parse().ok()).unwrap_or(0),
                                    message_id: id,
                                });
                            }
                        }
                        _ => {}
                    }
                }
                if segments.is_empty() {
                    continue;
                }
                segments.sort_by_key(|s| s.number);
                segments.dedup_by_key(|s| s.number);
                let bytes = segments.iter().map(|s| s.bytes).sum();
                nzb.files.push(NzbFile {
                    filename: filename_from_subject(&subject),
                    date,
                    groups,
                    segments,
                    bytes,
                });
            }
            _ => {}
        }
    }
    if nzb.files.is_empty() {
        return Err(t!("nzb.empty"));
    }
    // Nombres únicos
    let mut seen = HashSet::new();
    for f in &mut nzb.files {
        let mut name = f.filename.clone();
        let mut n = 2;
        while !seen.insert(name.to_ascii_lowercase()) {
            name = format!("{} ({})", f.filename, n);
            n += 1;
        }
        f.filename = name;
    }
    // Orden: par2 principal primero, después datos por nombre, volúmenes par2 al final
    nzb.files.sort_by(|a, b| {
        let class = |f: &NzbFile| {
            if f.is_recovery_vol() {
                2
            } else if f.is_par2() {
                0
            } else {
                1
            }
        };
        class(a).cmp(&class(b)).then_with(|| a.filename.to_ascii_lowercase().cmp(&b.filename.to_ascii_lowercase()))
    });
    Ok(nzb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_names() {
        assert_eq!(filename_from_subject(r#"[1/5] - "movie.part01.rar" yEnc (1/100)"#), "movie.part01.rar");
        assert_eq!(filename_from_subject("Some.Release.Name.mkv yEnc (1/20)"), "Some.Release.Name.mkv");
        assert_eq!(filename_from_subject("abc [01/10] - file.r00 yEnc (3/40)"), "file.r00");
    }

    #[test]
    fn parse_basic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE nzb PUBLIC "-//newzBin//DTD NZB 1.1//EN" "http://www.newzbin.com/DTD/nzb/nzb-1.1.dtd">
<nzb xmlns="http://www.newzbin.com/DTD/2003/nzb">
 <head><meta type="title">Mi Descarga</meta><meta type="password">secreto</meta></head>
 <file poster="a@b" date="1700000000" subject="&quot;test.rar&quot; yEnc (1/2)">
  <groups><group>alt.binaries.test</group></groups>
  <segments>
   <segment bytes="100" number="2">b@x</segment>
   <segment bytes="200" number="1">&lt;a@x&gt;</segment>
  </segments>
 </file>
 <file poster="a@b" date="1700000000" subject="&quot;test.par2&quot; yEnc (1/1)">
  <groups><group>alt.binaries.test</group></groups>
  <segments><segment bytes="50" number="1">c@x</segment></segments>
 </file>
</nzb>"#;
        let n = parse(xml.as_bytes()).unwrap();
        assert_eq!(n.title.as_deref(), Some("Mi Descarga"));
        assert_eq!(n.password.as_deref(), Some("secreto"));
        assert_eq!(n.files.len(), 2);
        assert_eq!(n.files[0].filename, "test.par2");
        assert_eq!(n.files[1].segments[0].message_id, "a@x");
        assert_eq!(n.files[1].bytes, 300);
    }

    #[test]
    fn obfuscated_par2_volumes_go_last() {
        let file = |name: &str, bytes: u64| NzbFile {
            filename: name.into(),
            date: 0,
            groups: vec![],
            segments: vec![],
            bytes,
        };
        let idx = file("a6a50c.par2", 5_000);
        let vol = file("af0627.par2", 118_000_000);
        let named = file("x.vol07+08.par2", 4_000);
        let data = file("movie.mkv", 3_000_000_000);
        assert!(!idx.is_recovery_vol());
        assert!(vol.is_recovery_vol());
        assert!(named.is_recovery_vol());
        assert!(!data.is_recovery_vol());
    }
}
