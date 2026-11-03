//! Local crash reports and a rolling log file (spec section 7). Nothing leaves the machine.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const LOG_FILE: &str = "duet.log";
pub const LOG_LIMIT: u64 = 5 * 1024 * 1024;

/// `%LOCALAPPDATA%\duet`, or `duet` in the temp folder when LOCALAPPDATA is unset.
pub fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("duet")
}

/// `%LOCALAPPDATA%\duet\logs`, or the temp folder when LOCALAPPDATA is unset.
pub fn log_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("DUET_LOG_DIR") {
        return PathBuf::from(dir);
    }
    data_dir().join("logs")
}

pub fn report_text(message: &str, location: Option<&str>, backtrace: &str) -> String {
    format!(
        "duet {} crashed\nmessage: {message}\nlocation: {}\n\nbacktrace:\n{backtrace}\n",
        env!("CARGO_PKG_VERSION"),
        location.unwrap_or("unknown"),
    )
}

pub fn write_report(dir: &Path, text: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let path = dir.join(format!("crash-{millis}.txt"));
    fs::write(&path, text)?;
    Ok(path)
}

/// Writes a crash report for every panic, then runs the previous hook.
pub fn install_panic_hook(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".into());
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()));
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let _ = write_report(
            &dir,
            &report_text(&message, location.as_deref(), &backtrace),
        );
        tracing::error!(%message, "panic");
        previous(info);
    }));
}

/// Keeps one previous log: `duet.log` becomes `duet.log.1` once it exceeds `limit`.
pub fn rotate_if_large(path: &Path, limit: u64) -> io::Result<()> {
    match fs::metadata(path) {
        Ok(meta) if meta.len() > limit => fs::rename(path, path.with_extension("log.1")),
        Ok(_) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

pub fn init_logging(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join(LOG_FILE);
    rotate_if_large(&path, LOG_LIMIT)?;
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let writer = Mutex::new(file);
    let _ = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(writer)
        .try_init();
    Ok(())
}

/// Flushes nothing extra; kept so tests can open the log after logging.
pub fn read_log(dir: &Path) -> io::Result<String> {
    let mut text = String::new();
    io::Read::read_to_string(&mut File::open(dir.join(LOG_FILE))?, &mut text)?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn report_names_the_message_and_location() {
        let text = report_text("boom", Some("src/x.rs:7"), "frames");
        assert!(text.contains("message: boom") && text.contains("location: src/x.rs:7"));
        assert!(text.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn reports_are_written_into_the_log_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_report(&dir.path().join("logs"), "hello").unwrap();
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("crash-")
        );
        assert_eq!(fs::read_to_string(path).unwrap(), "hello");
    }

    #[test]
    fn large_logs_rotate_once() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join(LOG_FILE);
        fs::write(&log, vec![b'x'; 11]).unwrap();
        rotate_if_large(&log, 10).unwrap();
        assert!(!log.exists());
        assert_eq!(fs::read(dir.path().join("duet.log.1")).unwrap().len(), 11);
        rotate_if_large(&log, 10).unwrap();
        let mut small = File::create(&log).unwrap();
        small.write_all(b"ok").unwrap();
        rotate_if_large(&log, 10).unwrap();
        assert!(log.exists(), "a small log stays");
    }
}
