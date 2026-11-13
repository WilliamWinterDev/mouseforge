//! A small log file at %APPDATA%\MouseForge\mouseforge.log. The release build
//! has no console, so this is the only record of startup problems or crashes.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

const MAX_BYTES: u64 = 512 * 1024;
static LOCK: Mutex<()> = Mutex::new(());

pub fn path() -> PathBuf {
    crate::config::Config::path().with_file_name("mouseforge.log")
}

pub fn log(msg: impl AsRef<str>) {
    let msg = msg.as_ref();
    eprintln!("{msg}");
    let _guard = LOCK.lock();
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Keep it small: start over once it gets big.
    if std::fs::metadata(&path).map_or(false, |m| m.len() > MAX_BYTES) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let _ = writeln!(f, "[{}] {msg}", timestamp(secs));
    }
}

/// Record panics (from any thread) in the log before the process dies.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log(format!("PANIC: {info}"));
        default(info);
    }));
}

/// UTC "YYYY-MM-DD HH:MM:SS" without pulling in a date crate.
fn timestamp(secs: u64) -> String {
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if mo <= 2 { 1 } else { 0 };
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02} UTC")
}

#[cfg(test)]
mod tests {
    #[test]
    fn timestamp_formats_known_dates() {
        assert_eq!(super::timestamp(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(super::timestamp(951_782_400), "2000-02-29 00:00:00 UTC");
        assert_eq!(super::timestamp(1_799_884_800), "2027-01-14 00:00:00 UTC");
    }
}
