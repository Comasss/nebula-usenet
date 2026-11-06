use parking_lot::Mutex;
use serde::Serialize;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Clone, Serialize)]
pub struct LogEntry {
    pub ts: u64,
    pub level: &'static str,
    pub msg: String,
}

struct Logger {
    buf: Mutex<VecDeque<LogEntry>>,
    file: Mutex<Option<File>>,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();
const MAX_ENTRIES: usize = 2000;

pub fn init(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join("nebula.log");
    // Rotación simple: si supera 5 MB, se renombra a .old
    if std::fs::metadata(&path).map(|m| m.len() > 5 * 1024 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("nebula.old.log"));
    }
    let file = OpenOptions::new().create(true).append(true).open(path).ok();
    let _ = LOGGER.set(Logger {
        buf: Mutex::new(VecDeque::with_capacity(MAX_ENTRIES)),
        file: Mutex::new(file),
    });
}

pub fn log(level: &'static str, msg: String) {
    let entry = LogEntry { ts: crate::util::now_ms(), level, msg };
    #[cfg(debug_assertions)]
    eprintln!("[{}] {}", level, entry.msg);
    let Some(l) = LOGGER.get() else { return };
    if let Some(f) = l.file.lock().as_mut() {
        let _ = writeln!(f, "{} [{}] {}", entry.ts, level.to_uppercase(), entry.msg);
    }
    {
        let mut b = l.buf.lock();
        if b.len() >= MAX_ENTRIES {
            b.pop_front();
        }
        b.push_back(entry.clone());
    }
    crate::bus::emit(crate::bus::Event::Log(entry));
}

pub fn entries() -> Vec<LogEntry> {
    LOGGER.get().map(|l| l.buf.lock().iter().cloned().collect()).unwrap_or_default()
}

pub fn clear() {
    if let Some(l) = LOGGER.get() {
        l.buf.lock().clear();
    }
}

#[macro_export]
macro_rules! linfo {
    ($($arg:tt)*) => { $crate::logger::log("info", format!($($arg)*)) };
}
#[macro_export]
macro_rules! lwarn {
    ($($arg:tt)*) => { $crate::logger::log("warn", format!($($arg)*)) };
}
#[macro_export]
macro_rules! lerror {
    ($($arg:tt)*) => { $crate::logger::log("error", format!($($arg)*)) };
}
