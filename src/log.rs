//! Plain-text log file.
//!
//! Lines go to `awkward-html.log` in the config folder (next to
//! `recent.json`) and to stderr. On Windows release builds there is no
//! console, so the file is the only place to see them.
//!
//! When the file passes 1 MB it is renamed to `awkward-html.log.old`
//! (replacing the previous one) and a new file is started.
//!
//! Set `AWKWARD_HTML_DEBUG=1` to also log every API call.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

const MAX_SIZE: u64 = 1024 * 1024;

static LOCK: Mutex<()> = Mutex::new(());

pub fn path() -> PathBuf {
    crate::server::config_dir().join("awkward-html.log")
}

pub fn debug_enabled() -> bool {
    std::env::var("AWKWARD_HTML_DEBUG").is_ok_and(|v| !v.is_empty() && v != "0")
}

pub fn info(msg: &str) {
    write("INFO", msg);
}

pub fn error(msg: &str) {
    write("ERROR", msg);
}

pub fn debug(msg: &str) {
    if debug_enabled() {
        write("DEBUG", msg);
    }
}

/// Log a message that came from the editor UI.
pub fn from_ui(level: &str, msg: &str) {
    let level = match level {
        "error" => "ERROR",
        "warn" => "WARN",
        "debug" => "DEBUG",
        _ => "INFO",
    };
    write(level, &format!("[ui] {msg}"));
}

fn write(level: &str, msg: &str) {
    let line = format!("{} {level:<5} {}\n", timestamp(), msg.trim_end());
    eprint!("{line}");
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let file = path();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&file).is_ok_and(|m| m.len() > MAX_SIZE) {
        let _ = std::fs::rename(&file, file.with_extension("log.old"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Log panics (crashes) before the program dies.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        error(&format!("panic: {info}"));
        default(info);
    }));
}

/// UTC time as `2026-11-18 12:30:05Z`, without pulling in a date crate.
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d) = civil_from_days((secs / 86400) as i64);
    let t = secs % 86400;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z", t / 3600, t / 60 % 60, t % 60)
}

/// Days since 1970-01-01 to (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20775), (2026, 11, 18));
        assert_eq!(civil_from_days(11016), (2000, 2, 29));
    }
}
