//! Opt-in metrics mode (002 research R7, FR-017): transcription timings in a CSV file in the
//! app log directory, for latency measurement on the installed app.
//!
//! - Turned on only by the environment variable `DEVCOPILOT_METRICS=1`. Off by default: no
//!   sink and no file.
//! - One file per UTC day, `metrics-<YYYY-MM-DD>.csv`, with the columns
//!   `kind,speech_start_ms,speech_end_ms,emitted_ms,provider`. [`SegmentTiming`] has no text
//!   field, so session content cannot reach the file (Art. IV.7).
//! - Writing never blocks transcription: timings go through a bounded queue to a writer
//!   thread, and are dropped (and counted) when the queue is full.
//! - Retention: on every startup, files older than [`RETENTION_DAYS`] are deleted, whether or
//!   not the mode is on (Art. IV.12).

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};

use devcopilot_application::ports::{MetricsSink, SegmentTiming};
use time::{Date, Month, OffsetDateTime};

/// Environment variable that turns the metrics mode on (value `1`).
pub const METRICS_ENV: &str = "DEVCOPILOT_METRICS";
/// Days a metrics file is kept.
pub const RETENTION_DAYS: i64 = 7;
/// CSV header; the only columns ever written.
pub const CSV_HEADER: &str = "kind,speech_start_ms,speech_end_ms,emitted_ms,provider";

const FILE_PREFIX: &str = "metrics-";
const FILE_SUFFIX: &str = ".csv";
/// Timings waiting for the writer thread before new ones are dropped.
const QUEUE_CAPACITY: usize = 1_024;

/// Whether `value` (the content of [`METRICS_ENV`]) turns the metrics mode on.
#[must_use]
pub fn enabled(value: Option<&str>) -> bool {
    value == Some("1")
}

/// The metrics sink for the running app: reads [`METRICS_ENV`] and writes to `log_dir`.
#[must_use]
pub fn sink_from_env(log_dir: &Path) -> Option<Arc<dyn MetricsSink>> {
    let value = std::env::var(METRICS_ENV).ok();
    sink_for(value.as_deref(), log_dir).map(|sink| sink as Arc<dyn MetricsSink>)
}

/// A [`CsvMetricsSink`] in `log_dir` when `value` turns the mode on, else `None`.
///
/// A sink that cannot start is logged and treated as off: measurement never stops the app.
#[must_use]
pub fn sink_for(value: Option<&str>, log_dir: &Path) -> Option<Arc<CsvMetricsSink>> {
    if !enabled(value) {
        return None;
    }
    match CsvMetricsSink::spawn(log_dir.to_path_buf()) {
        Ok(sink) => {
            tracing::info!("metrics mode on");
            Some(Arc::new(sink))
        }
        Err(err) => {
            tracing::warn!(error = %err, "metrics mode could not start");
            None
        }
    }
}

/// Name of the metrics file for `date`.
#[must_use]
pub fn file_name(date: Date) -> String {
    format!("{FILE_PREFIX}{date}{FILE_SUFFIX}")
}

/// The date in a metrics file name, if `name` is one.
fn date_of(name: &str) -> Option<Date> {
    let date = name.strip_prefix(FILE_PREFIX)?.strip_suffix(FILE_SUFFIX)?;
    let mut parts = date.splitn(3, '-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u8 = parts.next()?.parse().ok()?;
    let day: u8 = parts.next()?.parse().ok()?;
    Date::from_calendar_date(year, Month::try_from(month).ok()?, day).ok()
}

/// Deletes the metrics files in `dir` dated more than [`RETENTION_DAYS`] before `today`.
/// Other files are never touched. Returns how many were deleted.
///
/// # Errors
///
/// The directory cannot be read. A missing directory counts as empty.
pub fn remove_expired(dir: &Path, today: Date) -> io::Result<usize> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err),
    };
    let mut removed = 0;
    for entry in entries {
        let entry = entry?;
        let Some(date) = entry.file_name().to_str().and_then(date_of) else {
            continue;
        };
        if (today - date).whole_days() > RETENTION_DAYS {
            match std::fs::remove_file(entry.path()) {
                Ok(()) => removed += 1,
                Err(err) => tracing::warn!(error = %err, "expired metrics file not deleted"),
            }
        }
    }
    Ok(removed)
}

/// Today's date in UTC.
#[must_use]
pub fn today_utc() -> Date {
    OffsetDateTime::now_utc().date()
}

/// One CSV row (no trailing newline).
#[must_use]
pub fn csv_row(timing: &SegmentTiming) -> String {
    format!(
        "{},{},{},{},{}",
        timing.kind.code(),
        timing.speech_start_ms,
        timing.speech_end_ms,
        timing.emitted_ms,
        timing.provider
    )
}

/// [`MetricsSink`] writing CSV rows from a background thread.
pub struct CsvMetricsSink {
    queue: SyncSender<SegmentTiming>,
    dropped: AtomicU64,
}

impl CsvMetricsSink {
    /// Starts the writer thread for files in `dir`. The file of the day is created on the
    /// first timing.
    ///
    /// # Errors
    ///
    /// `dir` cannot be created or the thread cannot start.
    pub fn spawn(dir: PathBuf) -> io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        let (queue, timings) = mpsc::sync_channel(QUEUE_CAPACITY);
        std::thread::Builder::new()
            .name("devcopilot-metrics".into())
            .spawn(move || write_loop(&dir, &timings))?;
        Ok(Self {
            queue,
            dropped: AtomicU64::new(0),
        })
    }
}

impl MetricsSink for CsvMetricsSink {
    fn record(&self, timing: SegmentTiming) {
        match self.queue.try_send(timing) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                let dropped = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                if dropped == 1 || dropped.is_multiple_of(1_000) {
                    tracing::warn!(dropped, "metrics queue full; timings dropped");
                }
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

/// The open file of one UTC day.
struct DayFile {
    date: Date,
    writer: BufWriter<File>,
}

impl DayFile {
    fn open(dir: &Path, date: Date) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(file_name(date)))?;
        let empty = file.metadata()?.len() == 0;
        let mut writer = BufWriter::new(file);
        if empty {
            writeln!(writer, "{CSV_HEADER}")?;
        }
        Ok(Self { date, writer })
    }
}

/// Writes timings until every sender is gone; flushes after each burst.
fn write_loop(dir: &Path, timings: &Receiver<SegmentTiming>) {
    let mut file: Option<DayFile> = None;
    while let Ok(first) = timings.recv() {
        let burst = std::iter::once(first).chain(timings.try_iter());
        for timing in burst {
            if let Err(err) = write_one(dir, &mut file, &timing) {
                tracing::warn!(error = %err, "metrics row not written");
                file = None;
            }
        }
        if let Some(day) = file.as_mut()
            && let Err(err) = day.writer.flush()
        {
            tracing::warn!(error = %err, "metrics file not flushed");
            file = None;
        }
    }
}

fn write_one(dir: &Path, file: &mut Option<DayFile>, timing: &SegmentTiming) -> io::Result<()> {
    let today = today_utc();
    let day = match file {
        Some(day) if day.date == today => day,
        _ => {
            if let Some(mut previous) = file.take() {
                previous.writer.flush()?;
            }
            file.insert(DayFile::open(dir, today)?)
        }
    };
    writeln!(day.writer, "{}", csv_row(timing))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::time::{Duration, Instant};

    use devcopilot_application::ports::TimingKind;
    use devcopilot_domain::ids::ProviderId;

    use super::*;

    fn date(y: i32, m: Month, d: u8) -> Date {
        Date::from_calendar_date(y, m, d).unwrap()
    }

    fn touch(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), "x").unwrap();
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn only_the_value_1_turns_the_mode_on() {
        assert!(enabled(Some("1")));
        for value in [None, Some(""), Some("0"), Some("true"), Some(" 1")] {
            assert!(!enabled(value), "{value:?}");
        }
    }

    #[test]
    fn off_creates_no_sink_and_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("logs");
        assert!(sink_for(None, &logs).is_none());
        assert!(sink_for(Some("0"), &logs).is_none());
        assert!(!logs.exists());
    }

    #[test]
    fn file_names_round_trip() {
        let day = date(2026, Month::November, 1);
        assert_eq!(file_name(day), "metrics-2026-11-01.csv");
        assert_eq!(date_of(&file_name(day)), Some(day));
        for other in [
            "devcopilot.log.2026-11-01",
            "metrics-2026-13-01.csv",
            "metrics-latest.csv",
            "metrics-2026-11-01.txt",
        ] {
            assert_eq!(date_of(other), None, "{other}");
        }
    }

    #[test]
    fn cleanup_deletes_only_metrics_files_older_than_7_days() {
        let dir = tempfile::tempdir().unwrap();
        let today = date(2026, Month::November, 10);
        for name in [
            "metrics-2026-10-01.csv", // 40 days
            "metrics-2026-11-02.csv", // 8 days
            "metrics-2026-11-03.csv", // 7 days: kept
            "metrics-2026-11-10.csv", // today
            "devcopilot.log.2026-10-01",
            "notes.csv",
        ] {
            touch(dir.path(), name);
        }
        assert_eq!(remove_expired(dir.path(), today).unwrap(), 2);
        assert_eq!(
            names(dir.path()),
            [
                "devcopilot.log.2026-10-01",
                "metrics-2026-11-03.csv",
                "metrics-2026-11-10.csv",
                "notes.csv",
            ]
        );
    }

    #[test]
    fn cleanup_of_a_missing_directory_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            remove_expired(&dir.path().join("absent"), today_utc()).unwrap(),
            0
        );
    }

    #[test]
    fn sink_writes_the_header_and_one_row_per_timing() {
        let dir = tempfile::tempdir().unwrap();
        let sink = sink_for(Some("1"), dir.path()).unwrap();
        let provider = ProviderId::new();
        for (kind, start) in [(TimingKind::Partial, 0), (TimingKind::Final, 100)] {
            sink.record(SegmentTiming {
                kind,
                speech_start_ms: start,
                speech_end_ms: start + 900,
                emitted_ms: start + 1_500,
                provider,
            });
        }
        let path = dir.path().join(file_name(today_utc()));
        let deadline = Instant::now() + Duration::from_secs(3);
        let content = loop {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            if content.lines().count() == 3 || Instant::now() > deadline {
                break content;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(
            content,
            format!(
                "{CSV_HEADER}\npartial,0,900,1500,{provider}\nfinal,100,1000,1600,{provider}\n"
            )
        );
    }
}
