//! Cuota de descarga por servidor: cuánto se ha bajado de cada uno en el periodo actual
//! (día, semana o mes, en hora local). Se guarda en `quota.json`, aparte de los ajustes.
use crate::config::{AppPaths, ServerConfig};
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct QuotaUse {
    /// Bytes descargados desde `since`.
    pub used: u64,
    /// Inicio del periodo (segundos unix).
    pub since: u64,
}

fn file(paths: &AppPaths) -> std::path::PathBuf {
    paths.data.join("quota.json")
}

pub fn load(paths: &AppPaths) -> HashMap<String, QuotaUse> {
    std::fs::read(file(paths)).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
}

pub fn save(paths: &AppPaths, q: &HashMap<String, QuotaUse>) {
    if let Ok(d) = serde_json::to_vec(q) {
        let _ = crate::config::write_atomic(&file(paths), &d);
    }
}

/// Comienzo del periodo de cuota que contiene `now` (hora local). `None` si la cuota no se reinicia.
pub fn period_start(period: &str, reset_day: u32, now: NaiveDateTime) -> Option<NaiveDateTime> {
    let today = now.date();
    let date = match period {
        "day" => today,
        "week" => {
            let want = reset_day.clamp(1, 7);
            let back = (today.weekday().number_from_monday() + 7 - want) % 7;
            today - Duration::days(back as i64)
        }
        "month" => {
            let d = reset_day.clamp(1, 28);
            if today.day() >= d {
                NaiveDate::from_ymd_opt(today.year(), today.month(), d)?
            } else {
                let (y, m) = if today.month() == 1 { (today.year() - 1, 12) } else { (today.year(), today.month() - 1) };
                NaiveDate::from_ymd_opt(y, m, d)?
            }
        }
        _ => return None,
    };
    date.and_hms_opt(0, 0, 0)
}

/// Inicio del periodo actual de un servidor, en segundos unix (0 si nunca se reinicia).
pub fn current_start(srv: &ServerConfig) -> u64 {
    let now = Local::now().naive_local();
    period_start(&srv.quota_period, srv.quota_reset_day, now)
        .and_then(|p| Local.from_local_datetime(&p).earliest())
        .map(|t| t.timestamp().max(0) as u64)
        .unwrap_or(0)
}

/// Pone a cero las cuotas cuyo periodo ha terminado. Devuelve si alguna cambió.
pub fn roll(servers: &[ServerConfig], q: &mut HashMap<String, QuotaUse>) -> bool {
    let mut changed = false;
    for s in servers.iter().filter(|s| s.quota_bytes() > 0) {
        let start = current_start(s);
        let u = q.entry(s.id.clone()).or_default();
        if start > 0 && u.since < start {
            changed |= u.used > 0;
            *u = QuotaUse { used: 0, since: start };
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, m: u32, d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, 30, 0).unwrap()
    }

    #[test]
    fn periods() {
        // 2026-11-06 es viernes.
        let now = at(2026, 11, 6, 15);
        assert_eq!(period_start("day", 1, now), Some(at(2026, 11, 6, 0) - Duration::minutes(30)));
        assert_eq!(period_start("week", 1, now).unwrap().date(), NaiveDate::from_ymd_opt(2026, 11, 2).unwrap());
        assert_eq!(period_start("week", 5, now).unwrap().date(), NaiveDate::from_ymd_opt(2026, 11, 6).unwrap());
        assert_eq!(period_start("week", 6, now).unwrap().date(), NaiveDate::from_ymd_opt(2026, 10, 31).unwrap());
        assert_eq!(period_start("month", 1, now).unwrap().date(), NaiveDate::from_ymd_opt(2026, 11, 1).unwrap());
        assert_eq!(period_start("month", 15, now).unwrap().date(), NaiveDate::from_ymd_opt(2026, 10, 15).unwrap());
        assert_eq!(period_start("month", 15, at(2026, 1, 3, 1)).unwrap().date(), NaiveDate::from_ymd_opt(2025, 12, 15).unwrap());
        assert_eq!(period_start("never", 1, now), None);
    }

    #[test]
    fn rolls_over() {
        let mut s = ServerConfig { quota_gb: 1.0, quota_period: "day".into(), ..Default::default() };
        s.id = "a".into();
        let mut q = HashMap::new();
        q.insert("a".to_string(), QuotaUse { used: 500, since: 1 });
        assert!(roll(&[s.clone()], &mut q));
        assert_eq!(q["a"].used, 0);
        assert!(!roll(&[s.clone()], &mut q));
        // Sin reinicio: el uso se conserva.
        s.quota_period = "never".into();
        q.get_mut("a").unwrap().used = 7;
        roll(&[s], &mut q);
        assert_eq!(q["a"].used, 7);
    }
}
