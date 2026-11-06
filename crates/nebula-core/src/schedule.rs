//! Programador: pausar, reanudar o limitar la velocidad a ciertas horas (hora local).
//! El motor mira cada minuto qué tareas tocan; al arrancar aplica el estado que dejaron las
//! últimas tareas de la semana, por si Nebula estaba cerrado a esa hora.
use crate::config::ScheduleRule;
use chrono::{Datelike, Duration, NaiveDateTime, NaiveTime, Timelike};

fn parse_time(s: &str) -> Option<NaiveTime> {
    let (h, m) = s.trim().split_once(':')?;
    NaiveTime::from_hms_opt(h.trim().parse().ok()?, m.trim().parse().ok()?, 0)
}

fn valid(r: &ScheduleRule) -> bool {
    r.enabled && matches!(r.action.as_str(), "pause" | "resume" | "speed") && parse_time(&r.time).is_some()
}

fn on_day(r: &ScheduleRule, day: u32) -> bool {
    r.days.is_empty() || r.days.iter().any(|d| *d as u32 == day)
}

/// Tareas que tocan en el minuto `now`.
pub fn due(rules: &[ScheduleRule], now: NaiveDateTime) -> Vec<ScheduleRule> {
    let day = now.weekday().number_from_monday();
    rules
        .iter()
        .filter(|r| valid(r) && on_day(r, day))
        .filter(|r| parse_time(&r.time).is_some_and(|t| t.hour() == now.hour() && t.minute() == now.minute()))
        .cloned()
        .collect()
}

/// Estado que dejaron las tareas de los últimos 7 días: (¿en pausa?, límite KB/s).
pub fn current(rules: &[ScheduleRule], now: NaiveDateTime) -> (Option<bool>, Option<u64>) {
    let mut pause: Option<(NaiveDateTime, bool)> = None;
    let mut speed: Option<(NaiveDateTime, u64)> = None;
    for back in 0..8 {
        let date = (now - Duration::days(back)).date();
        let day = date.weekday().number_from_monday();
        for r in rules.iter().filter(|r| valid(r) && on_day(r, day)) {
            let Some(at) = parse_time(&r.time).map(|t| date.and_time(t)) else { continue };
            if at > now {
                continue;
            }
            match r.action.as_str() {
                "speed" => {
                    if speed.is_none_or(|(t, _)| at > t) {
                        speed = Some((at, r.value));
                    }
                }
                a => {
                    if pause.is_none_or(|(t, _)| at > t) {
                        pause = Some((at, a == "pause"));
                    }
                }
            }
        }
    }
    (pause.map(|p| p.1), speed.map(|s| s.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn rule(days: &[u8], time: &str, action: &str, value: u64) -> ScheduleRule {
        ScheduleRule { days: days.to_vec(), time: time.into(), action: action.into(), value, ..Default::default() }
    }

    fn at(d: u32, h: u32, m: u32) -> NaiveDateTime {
        // Noviembre de 2026: el día 2 es lunes.
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap().and_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn due_and_current() {
        let rules = vec![
            rule(&[1, 2, 3, 4, 5], "08:00", "pause", 0),
            rule(&[1, 2, 3, 4, 5], "18:30", "resume", 0),
            rule(&[], "01:00", "speed", 0),
            rule(&[], "17:00", "speed", 5120),
            rule(&[6], "9:5", "bogus", 0),
        ];
        assert_eq!(due(&rules, at(2, 8, 0)).len(), 1);
        assert!(due(&rules, at(7, 8, 0)).is_empty()); // sábado
        assert_eq!(due(&rules, at(7, 17, 0))[0].value, 5120);
        // Martes a las 12: en pausa desde las 8, límite de las 17 del lunes ya sustituido a la 1.
        assert_eq!(current(&rules, at(3, 12, 0)), (Some(true), Some(0)));
        // Martes a las 20: reanudado; límite de las 17.
        assert_eq!(current(&rules, at(3, 20, 0)), (Some(false), Some(5120)));
        // Domingo: lo último fue reanudar el viernes.
        assert_eq!(current(&rules, at(8, 12, 0)).0, Some(false));
        assert_eq!(current(&[], at(8, 12, 0)), (None, None));
    }
}
