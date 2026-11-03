//! Cliente de la API Newznab (búsqueda en indexadores).
use crate::config::IndexerConfig;
use serde::Serialize;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(concat!("Nebula/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::limited(10))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .expect("cliente HTTP")
});

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchItem {
    pub title: String,
    /// Enlace de descarga del .nzb
    pub link: String,
    /// Página de detalles en el indexador
    pub details: Option<String>,
    pub size: u64,
    /// Fecha de publicación en Usenet (unix)
    pub date: u64,
    pub grabs: Option<u32>,
    pub files: Option<u32>,
    pub category: String,
    pub category_ids: Vec<u32>,
    /// 0 = sin contraseña, 1 = RAR con contraseña, 2 = contiene archivos protegidos
    pub password: Option<u8>,
    pub indexer_id: String,
    pub indexer_name: String,
    /// Fecha en que el indexador o el feed lo publicó (pubDate, unix). Solo la usa el RSS.
    #[serde(skip)]
    pub published: u64,
    /// Identificador estable del elemento en el feed (guid o enlace). Solo la usa el RSS.
    #[serde(skip)]
    pub guid: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub items: Vec<SearchItem>,
    pub total: Option<u64>,
    pub offset: u32,
    pub ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexerTest {
    pub ok: bool,
    pub message: String,
    pub latency_ms: u64,
}

fn api_url(ix: &IndexerConfig) -> Result<String, String> {
    let base = ix.url.trim().trim_end_matches('/');
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err(t!("indexer.scheme"));
    }
    let path = if ix.api_path.trim().is_empty() { "/api" } else { ix.api_path.trim() };
    let path = format!("/{}", path.trim_matches('/'));
    Ok(if base.ends_with(&path) { base.to_string() } else { format!("{}{}", base, path) })
}

/// Traduce los códigos de error de Newznab.
pub fn newznab_error(code: &str, description: &str) -> String {
    match code {
        "100" => t!("indexer.badKey"),
        "101" => t!("indexer.suspended"),
        "102" => t!("indexer.noPermission"),
        "200" | "201" => t!("indexer.badParams", detail = description),
        "202" | "203" => t!("indexer.unsupported"),
        "300" => t!("indexer.nzbGone"),
        "429" | "500" => t!("indexer.rateLimited"),
        "501" => t!("indexer.dailyLimit"),
        "910" => t!("indexer.apiDisabled"),
        _ if !description.is_empty() => t!("indexer.replied", detail = description),
        _ => t!("indexer.errorCode", code = code),
    }
}

async fn get(ix: &IndexerConfig, params: &[(&str, String)]) -> Result<String, String> {
    let url = api_url(ix)?;
    let mut query: Vec<(&str, String)> = params.to_vec();
    if !ix.api_key.trim().is_empty() {
        query.push(("apikey", ix.api_key.trim().to_string()));
    }
    let resp = CLIENT.get(&url).query(&query).send().await.map_err(|e| {
        if e.is_timeout() {
            t!("indexer.timeout")
        } else if e.is_connect() {
            t!("indexer.connect")
        } else {
            t!("err.connection", error = e.without_url())
        }
    })?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| t!("indexer.incomplete", error = e.without_url()))?;
    if let Some(err) = parse_error(&body) {
        return Err(err);
    }
    if !status.is_success() {
        return Err(match status.as_u16() {
            401 | 403 => t!("indexer.denied"),
            404 => t!("indexer.notFound"),
            429 => t!("indexer.rateLimited"),
            c => t!("indexer.httpError", status = c),
        });
    }
    let head = body.trim_start().get(..200).unwrap_or(body.trim_start()).to_ascii_lowercase();
    if head.starts_with("<!doctype html") || head.starts_with("<html") {
        return Err(t!("indexer.notNewznab"));
    }
    Ok(body)
}

fn parse_error(body: &str) -> Option<String> {
    let t = body.trim_start();
    let t = t.strip_prefix('\u{feff}').unwrap_or(t);
    // <?xml ...?><error code="100" description="..."/>
    let doc = roxmltree::Document::parse(t).ok()?;
    let root = doc.root_element();
    if root.tag_name().name() == "error" {
        return Some(newznab_error(root.attribute("code").unwrap_or(""), root.attribute("description").unwrap_or("")));
    }
    None
}

pub async fn search(ix: &IndexerConfig, query: &str, category: Option<&str>, offset: u32, limit: u32) -> Result<SearchResult, String> {
    let start = Instant::now();
    let mut params: Vec<(&str, String)> = vec![
        ("t", "search".into()),
        ("extended", "1".into()),
        ("limit", limit.clamp(10, 500).to_string()),
        ("offset", offset.to_string()),
    ];
    if !query.trim().is_empty() {
        params.push(("q", query.trim().to_string()));
    }
    if let Some(c) = category.filter(|c| !c.trim().is_empty()) {
        params.push(("cat", c.trim().to_string()));
    }
    let body = get(ix, &params).await?;
    let (items, total) = parse_rss(&body, &ix.id, &ix.label())?;
    Ok(SearchResult { items, total, offset, ms: start.elapsed().as_millis() as u64 })
}

/// Búsqueda con los parámetros Newznab que pide un cliente de la API (Sonarr, Radarr…):
/// `func` es la función ("search", "tvsearch", "movie"…) y `params` el resto, ya filtrados.
/// Si el indexador no admite la función, se repite como búsqueda de texto.
pub async fn query(ix: &IndexerConfig, func: &str, params: &[(&str, String)], offset: u32, limit: u32) -> Result<SearchResult, String> {
    let start = Instant::now();
    let base: Vec<(&str, String)> = vec![
        ("extended", "1".into()),
        ("limit", limit.clamp(1, 500).to_string()),
        ("offset", offset.to_string()),
    ];
    let mut first = base.clone();
    first.push(("t", func.to_string()));
    first.extend(params.iter().cloned());
    let body = match get(ix, &first).await {
        Err(e) if func != "search" && e == t!("indexer.unsupported") && params.iter().any(|(k, v)| *k == "q" && !v.is_empty()) => {
            let mut retry = base;
            retry.push(("t", "search".into()));
            retry.extend(params.iter().filter(|(k, _)| matches!(*k, "q" | "cat" | "maxage" | "minsize" | "maxsize")).cloned());
            get(ix, &retry).await?
        }
        r => r?,
    };
    let (items, total) = parse_rss(&body, &ix.id, &ix.label())?;
    Ok(SearchResult { items, total, offset, ms: start.elapsed().as_millis() as u64 })
}

pub async fn test(ix: &IndexerConfig) -> IndexerTest {
    let start = Instant::now();
    let caps = match get(ix, &[("t", "caps".into())]).await {
        Ok(b) => b,
        Err(e) => return IndexerTest { ok: false, message: e, latency_ms: start.elapsed().as_millis() as u64 },
    };
    let latency = start.elapsed().as_millis() as u64;
    let (title, max) = parse_caps(&caps);
    // La consulta de capacidades no siempre exige API key: se valida con una búsqueda mínima.
    if let Err(e) = get(ix, &[("t", "search".into()), ("limit", "1".into())]).await {
        return IndexerTest { ok: false, message: e, latency_ms: latency };
    }
    let name = title.unwrap_or_else(|| ix.label());
    IndexerTest {
        ok: true,
        message: match max {
            Some(m) => t!("indexer.connectedMax", name = name, max = m),
            None => t!("indexer.connected", name = name),
        },
        latency_ms: latency,
    }
}

fn parse_caps(body: &str) -> (Option<String>, Option<u32>) {
    let Ok(doc) = roxmltree::Document::parse(body.trim_start()) else { return (None, None) };
    let mut title = None;
    let mut max = None;
    for n in doc.descendants() {
        match n.tag_name().name() {
            "server" => title = n.attribute("title").map(|s| s.to_string()).filter(|s| !s.is_empty()),
            "limits" => max = n.attribute("max").and_then(|m| m.parse().ok()),
            _ => {}
        }
    }
    (title, max)
}

pub fn parse_rss(body: &str, indexer_id: &str, indexer_name: &str) -> Result<(Vec<SearchItem>, Option<u64>), String> {
    let text = body.trim_start().trim_start_matches('\u{feff}');
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(text, opts).map_err(|_| t!("indexer.invalidResponse"))?;
    let mut total = None;
    let mut items = Vec::new();
    for n in doc.descendants() {
        if n.tag_name().name() == "response" {
            total = n.attribute("total").and_then(|t| t.parse().ok());
        }
        if n.tag_name().name() != "item" {
            continue;
        }
        let mut it = SearchItem {
            title: String::new(),
            link: String::new(),
            details: None,
            size: 0,
            date: 0,
            grabs: None,
            files: None,
            category: String::new(),
            category_ids: Vec::new(),
            password: None,
            indexer_id: indexer_id.to_string(),
            indexer_name: indexer_name.to_string(),
            published: 0,
            guid: String::new(),
        };
        let mut pub_date = 0;
        let mut enclosure = None;
        let mut guid_link = None;
        for c in n.children().filter(|c| c.is_element()) {
            let text = c.text().unwrap_or("").trim();
            match c.tag_name().name() {
                "title" => it.title = text.to_string(),
                "link" => it.link = text.to_string(),
                "comments" => it.details = Some(text.split('#').next().unwrap_or(text).to_string()),
                "guid" => {
                    it.guid = text.to_string();
                    if c.attribute("isPermaLink") == Some("true") {
                        guid_link = Some(text.to_string());
                    }
                }
                "pubDate" => pub_date = parse_rfc2822(text).unwrap_or(0),
                "category" => it.category = text.to_string(),
                "size" => it.size = text.parse().unwrap_or(0),
                "enclosure" => {
                    enclosure = c.attribute("url").map(|u| u.to_string());
                    if it.size == 0 {
                        it.size = c.attribute("length").and_then(|l| l.parse().ok()).unwrap_or(0);
                    }
                }
                "attr" => {
                    let value = c.attribute("value").unwrap_or("");
                    match c.attribute("name").unwrap_or("") {
                        "size" => it.size = value.parse().unwrap_or(it.size),
                        "grabs" => it.grabs = value.parse().ok(),
                        "files" => it.files = value.parse().ok(),
                        "category" => {
                            if let Ok(v) = value.parse() {
                                it.category_ids.push(v);
                            }
                        }
                        "password" => it.password = value.parse().ok(),
                        "usenetdate" => it.date = parse_rfc2822(value).unwrap_or(0),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        if it.link.is_empty() {
            it.link = enclosure.unwrap_or_default();
        }
        if it.details.is_none() {
            it.details = guid_link;
        }
        if it.date == 0 {
            it.date = pub_date;
        }
        it.published = if pub_date > 0 { pub_date } else { it.date };
        if it.guid.is_empty() {
            it.guid = it.link.clone();
        }
        if !it.title.is_empty() && !it.link.is_empty() {
            items.push(it);
        }
    }
    Ok((items, total))
}

/// Lee un feed RSS suelto (no hace falta que sea de un indexador Newznab).
pub async fn fetch_feed(url: &str, name: &str) -> Result<Vec<SearchItem>, String> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(t!("indexer.scheme"));
    }
    let resp = CLIENT.get(url).send().await.map_err(|e| {
        if e.is_timeout() {
            t!("indexer.timeout")
        } else if e.is_connect() {
            t!("indexer.connect")
        } else {
            t!("err.connection", error = e.without_url())
        }
    })?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| t!("indexer.incomplete", error = e.without_url()))?;
    if let Some(err) = parse_error(&body) {
        return Err(err);
    }
    if !status.is_success() {
        return Err(match status.as_u16() {
            401 | 403 => t!("indexer.denied"),
            404 => t!("indexer.notFound"),
            429 => t!("indexer.rateLimited"),
            c => t!("indexer.httpError", status = c),
        });
    }
    Ok(parse_rss(&body, "", name)?.0)
}

/// Convierte fechas RFC 2822 ("Tue, 29 Sep 2026 10:00:00 +0200") a tiempo unix.
pub fn parse_rfc2822(s: &str) -> Option<u64> {
    let s = s.trim();
    let s = match s.find(',') {
        Some(i) => &s[i + 1..],
        None => s,
    };
    let p: Vec<&str> = s.split_whitespace().collect();
    if p.len() < 4 {
        return None;
    }
    let day: i64 = p[0].parse().ok()?;
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let mon = p[1].to_ascii_lowercase();
    let month = MONTHS.iter().position(|m| mon.starts_with(m))? as i64 + 1;
    let year: i64 = p[2].parse().ok()?;
    let t: Vec<i64> = p[3].split(':').map(|x| x.parse().unwrap_or(0)).collect();
    let (h, mi, se) = (t.first().copied()?, t.get(1).copied().unwrap_or(0), t.get(2).copied().unwrap_or(0));
    let offset = p.get(4).map(|z| {
        let z = z.trim();
        if (z.starts_with('+') || z.starts_with('-')) && z.len() >= 5 {
            let sign = if z.starts_with('-') { -1 } else { 1 };
            let hh: i64 = z[1..3].parse().unwrap_or(0);
            let mm: i64 = z[3..5].parse().unwrap_or(0);
            sign * (hh * 3600 + mm * 60)
        } else {
            0
        }
    });
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let ts = days * 86400 + h * 3600 + mi * 60 + se - offset.unwrap_or(0);
    (ts > 0).then_some(ts as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contra un indexador real o el simulado de dev/mock_nntp.py:
    /// NEBULA_NEWZNAB=http://127.0.0.1:8119 NEBULA_NEWZNAB_KEY=testkey cargo test live_indexer -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_indexer() {
        let ix = IndexerConfig {
            name: "Mock".into(),
            url: std::env::var("NEBULA_NEWZNAB").expect("NEBULA_NEWZNAB"),
            api_key: std::env::var("NEBULA_NEWZNAB_KEY").unwrap_or_default(),
            ..Default::default()
        };
        let t = test(&ix).await;
        println!("prueba: ok={} · {}", t.ok, t.message);
        assert!(t.ok);
        let r = search(&ix, "pelicula prueba", None, 0, 100).await.unwrap();
        println!("búsqueda: {} resultados (total {:?}) en {} ms", r.items.len(), r.total, r.ms);
        for it in &r.items {
            println!("  - {} · {} bytes · {} descargas · cat {:?}", it.title, it.size, it.grabs.unwrap_or(0), it.category_ids);
        }
        assert!(!r.items.is_empty());
        let none = search(&ix, "zzzz-no-existe", None, 0, 100).await.unwrap();
        assert!(none.items.is_empty());
        let bad = IndexerConfig { api_key: "incorrecta".into(), ..ix.clone() };
        let err = search(&bad, "x", None, 0, 100).await.unwrap_err();
        println!("API key mala: {}", err);
        assert_eq!(err, "API key incorrecta");
        let f = crate::fetch::fetch(&r.items[0].link).await.unwrap();
        println!("NZB descargado: {} bytes, nombre {:?}", f.data.len(), f.filename);
        assert!(crate::nzb::looks_like_nzb(&f.data));
        let dir = std::env::temp_dir().join(format!("nebula-live-{}", crate::util::new_id()));
        let saved = crate::fetch::save_nzb_file(&f.data, &dir.to_string_lossy(), &r.items[0].title).unwrap();
        let again = crate::fetch::save_nzb_file(&f.data, &dir.to_string_lossy(), &r.items[0].title).unwrap();
        println!("guardado: {} y {}", saved, again);
        assert_ne!(saved, again);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rfc2822() {
        assert_eq!(parse_rfc2822("Thu, 01 Jan 1970 00:01:00 +0000"), Some(60));
        assert_eq!(parse_rfc2822("Tue, 29 Sep 2026 12:00:00 +0200"), parse_rfc2822("29 Sep 2026 10:00:00 GMT"));
        assert_eq!(parse_rfc2822("Sat, 01 Mar 2025 00:00:00 +0000"), Some(1740787200));
    }

    #[test]
    fn url_building() {
        let mut ix = IndexerConfig { url: "https://api.nzbgeek.info/".into(), ..Default::default() };
        assert_eq!(api_url(&ix).unwrap(), "https://api.nzbgeek.info/api");
        ix.url = "https://nzbfinder.ws/api".into();
        assert_eq!(api_url(&ix).unwrap(), "https://nzbfinder.ws/api");
        ix.url = "nzbfinder.ws".into();
        assert!(api_url(&ix).is_err());
    }

    #[test]
    fn errors() {
        let e = parse_error(r#"<?xml version="1.0" encoding="UTF-8"?><error code="100" description="Incorrect user credentials"/>"#);
        assert_eq!(e.as_deref(), Some("API key incorrecta"));
        assert!(parse_error("<rss></rss>").is_none());
    }

    #[test]
    fn rss() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom" xmlns:newznab="http://www.newznab.com/DTD/2010/feeds/attributes/">
<channel>
<newznab:response offset="0" total="2"/>
<item>
 <title>Some.Show.S01E01.1080p.WEB.h264-GRP</title>
 <guid isPermaLink="true">https://idx.example/details/abc</guid>
 <link>https://idx.example/getnzb/abc.nzb&amp;i=1&amp;r=KEY</link>
 <comments>https://idx.example/details/abc#comments</comments>
 <pubDate>Tue, 29 Sep 2026 10:00:00 +0000</pubDate>
 <category>TV &gt; HD</category>
 <enclosure url="https://idx.example/getnzb/abc.nzb&amp;i=1&amp;r=KEY" length="1500000000" type="application/x-nzb"/>
 <newznab:attr name="category" value="5000"/>
 <newznab:attr name="category" value="5040"/>
 <newznab:attr name="size" value="1500000000"/>
 <newznab:attr name="grabs" value="42"/>
 <newznab:attr name="password" value="0"/>
 <newznab:attr name="usenetdate" value="Mon, 28 Sep 2026 10:00:00 +0000"/>
</item>
<item><title>Sin enlace</title></item>
</channel></rss>"#;
        let (items, total) = parse_rss(xml, "i1", "Idx").unwrap();
        assert_eq!(total, Some(2));
        assert_eq!(items.len(), 1);
        let it = &items[0];
        assert_eq!(it.link, "https://idx.example/getnzb/abc.nzb&i=1&r=KEY");
        assert_eq!(it.details.as_deref(), Some("https://idx.example/details/abc"));
        assert_eq!(it.size, 1_500_000_000);
        assert_eq!(it.grabs, Some(42));
        assert_eq!(it.category, "TV > HD");
        assert_eq!(it.category_ids, vec![5000, 5040]);
        assert_eq!(it.date, parse_rfc2822("Mon, 28 Sep 2026 10:00:00 +0000").unwrap());
    }
}
