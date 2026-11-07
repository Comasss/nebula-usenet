//! Formato de la salida de texto: tamaños, velocidades, tiempos y tablas alineadas.
use serde_json::Value;

pub fn bytes(b: u64) -> String {
    nebula_core::util::fmt_bytes(b)
}

pub fn speed(bps: u64) -> String {
    if bps == 0 {
        "—".into()
    } else {
        format!("{}/s", bytes(bps))
    }
}

pub fn eta(secs: u64) -> String {
    match secs {
        0 => "—".into(),
        s if s < 60 => format!("{}s", s),
        s if s < 3600 => format!("{}m {:02}s", s / 60, s % 60),
        s if s < 86400 => format!("{}h {:02}m", s / 3600, (s % 3600) / 60),
        s => format!("{}d {}h", s / 86400, (s % 86400) / 3600),
    }
}

/// Fecha local corta (AAAA-MM-DD hh:mm) de un instante unix.
pub fn date(secs: u64) -> String {
    if secs == 0 {
        return "—".into();
    }
    // Sin dependencias de zona horaria: UTC, que es lo que entiende cualquier script.
    let days = (secs / 86400) as i64;
    let (y, m, d) = civil(days);
    format!("{:04}-{:02}-{:02} {:02}:{:02}", y, m, d, (secs % 86400) / 3600, (secs % 3600) / 60)
}

/// Días desde 1970-01-01 a fecha (algoritmo de Howard Hinnant).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Nombre traducido del estado de una descarga ("downloading", "failed"…).
pub fn status(s: &str) -> String {
    match s {
        "queued" | "downloading" | "paused" | "verifying" | "repairing" | "extracting" | "moving" | "completed" | "failed" => {
            nebula_core::i18n::text(&format!("cli.st.{}", s))
        }
        other => other.to_string(),
    }
}

pub fn priority(p: i64) -> String {
    nebula_core::i18n::text(match p {
        i64::MIN..=-1 => "cli.prio.low",
        0 => "cli.prio.normal",
        1 => "cli.prio.high",
        _ => "cli.prio.force",
    })
}

/// "low", "normal", "high", "force" (o -1…2) → prioridad del motor.
pub fn parse_priority(s: &str) -> Option<i32> {
    match s.trim().to_ascii_lowercase().as_str() {
        "low" | "-1" => Some(-1),
        "normal" | "0" => Some(0),
        "high" | "1" => Some(1),
        "force" | "forced" | "2" => Some(2),
        _ => None,
    }
}

/// "10" (MB/s), "2.5M", "500K", "1G", "0"/"off"/"none" → KB/s del motor (0 = sin límite).
pub fn parse_speed(s: &str) -> Option<u64> {
    let t = s.trim().to_ascii_lowercase();
    if matches!(t.as_str(), "0" | "off" | "none" | "unlimited") {
        return Some(0);
    }
    let t = t.trim_end_matches("/s").trim_end_matches('b');
    let (num, mult) = match t.chars().last()? {
        'k' => (&t[..t.len() - 1], 1.0),
        'm' => (&t[..t.len() - 1], 1024.0),
        'g' => (&t[..t.len() - 1], 1024.0 * 1024.0),
        _ => (t, 1024.0),
    };
    let v: f64 = num.trim().parse().ok()?;
    (v >= 0.0 && v.is_finite()).then(|| (v * mult).round() as u64)
}

pub fn str<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

pub fn num(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// Recorta un texto a `max` caracteres con "…".
pub fn cut(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Imprime una tabla con columnas alineadas; la última columna no se rellena.
pub fn table(head: &[String], rows: &[Vec<String>]) {
    let cols = head.len();
    let mut w = vec![0usize; cols];
    for r in std::iter::once(&head.to_vec()).chain(rows.iter()) {
        for (i, c) in r.iter().enumerate().take(cols) {
            w[i] = w[i].max(c.chars().count());
        }
    }
    let line = |r: &[String]| {
        let mut out = String::new();
        for (i, c) in r.iter().enumerate() {
            if i + 1 == cols {
                out.push_str(c);
            } else {
                out.push_str(c);
                out.push_str(&" ".repeat(w[i] - c.chars().count() + 2));
            }
        }
        println!("{}", out.trim_end());
    };
    line(head);
    for r in rows {
        line(r);
    }
}

pub fn print_json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds() {
        assert_eq!(parse_speed("10"), Some(10240));
        assert_eq!(parse_speed("2.5M"), Some(2560));
        assert_eq!(parse_speed("500K"), Some(500));
        assert_eq!(parse_speed("1g"), Some(1048576));
        assert_eq!(parse_speed("5MB/s"), Some(5120));
        assert_eq!(parse_speed("off"), Some(0));
        assert_eq!(parse_speed("rápido"), None);
    }

    #[test]
    fn misc() {
        assert_eq!(parse_priority("Force"), Some(2));
        assert_eq!(parse_priority("x"), None);
        assert_eq!(eta(3725), "1h 02m");
        assert_eq!(date(0), "—");
        assert_eq!(date(1_700_000_000), "2023-11-14 22:13");
        assert_eq!(cut("abcdef", 4), "abc…");
    }
}
