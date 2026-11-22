//! Journal minimal : `%APPDATA%\soomfon-deck\soomfon-deck.log` + console.
//! Niveau par défaut : info. `SOOMFON_LOG=debug` pour plus de détails.

use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use log::{Level, LevelFilter, Log, Metadata, Record};

/// Au-delà, le fichier est vidé au démarrage.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

struct Logger {
    file: Option<Mutex<File>>,
    level: LevelFilter,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level && metadata.target().starts_with("soomfon")
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let tag = match record.level() {
            Level::Error => "ERREUR",
            Level::Warn => "ATTENTION",
            Level::Info => "info",
            Level::Debug | Level::Trace => "debug",
        };
        let line = format!("{} {tag:<9} {}\n", utc_time(), record.args());
        print!("{line}");
        if let Some(file) = &self.file {
            if let Ok(mut f) = file.lock() {
                let _ = f.write_all(line.as_bytes());
            }
        }
    }

    fn flush(&self) {}
}

pub fn init(path: &Path) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let too_big = std::fs::metadata(path).map(|m| m.len() > MAX_LOG_BYTES).unwrap_or(false);
    let file = OpenOptions::new()
        .create(true)
        .append(!too_big)
        .write(true)
        .truncate(too_big)
        .open(path)
        .ok()
        .map(Mutex::new);

    let level = match std::env::var("SOOMFON_LOG").as_deref() {
        Ok("debug") => LevelFilter::Debug,
        Ok("warn") => LevelFilter::Warn,
        _ => LevelFilter::Info,
    };
    let _ = log::set_boxed_logger(Box::new(Logger { file, level }));
    log::set_max_level(level);
}

/// Heure UTC « AAAA-MM-JJ HH:MM:SS » sans dépendance supplémentaire.
fn utc_time() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z", rem / 3600, (rem / 60) % 60, rem % 60)
}

/// Algorithme de Howard Hinnant : jours depuis 1970-01-01 → (année, mois, jour).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates() {
        assert_eq!(super::civil_from_days(0), (1970, 1, 1));
        assert_eq!(super::civil_from_days(20_401), (2025, 11, 9));
    }
}
