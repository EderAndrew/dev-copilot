//! `sample`: CPU and RSS of one process at a fixed interval, as CSV.

use std::ffi::OsStr;
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::utc;

/// One row of the samples CSV: `utc,cpu_percent_of_total,rss_bytes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// `YYYY-MM-DDTHH:MM:SSZ`.
    pub utc: String,
    /// CPU of the process as a share of the whole machine: 100 means every logical core.
    pub cpu_percent_of_total: f64,
    /// Resident set size.
    pub rss_bytes: u64,
}

/// Which process to sample.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Exact process name; must match exactly one process.
    Name(String),
    /// Process id.
    Pid(u32),
}

/// Converts sysinfo's per-core CPU (100 per busy core) to a share of total capacity.
#[must_use]
pub fn normalize_cpu(per_core_percent: f32, logical_cpus: usize) -> f64 {
    #[allow(clippy::cast_precision_loss)] // core counts are tiny
    let cpus = logical_cpus.max(1) as f64;
    f64::from(per_core_percent) / cpus
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn resolve(system: &mut System, target: &Target) -> Result<Pid> {
    match target {
        Target::Pid(pid) => Ok(Pid::from_u32(*pid)),
        Target::Name(name) => {
            system.refresh_processes(ProcessesToUpdate::All, true);
            let pids: Vec<Pid> = system
                .processes_by_exact_name(OsStr::new(name))
                .map(sysinfo::Process::pid)
                .collect();
            match pids.as_slice() {
                [pid] => Ok(*pid),
                [] => bail!("no process named `{name}`"),
                many => bail!(
                    "{} processes named `{name}` ({many:?}); pass --pid",
                    many.len()
                ),
            }
        }
    }
}

/// Samples `target` every `interval` until it exits or `max_samples` rows are written,
/// writing CSV rows to `out` (flushed after each row, so a killed run keeps its data).
///
/// # Errors
///
/// The process cannot be found, or `out` cannot be written.
pub fn run(
    target: &Target,
    interval: Duration,
    max_samples: Option<u64>,
    out: impl Write,
) -> Result<u64> {
    let logical_cpus = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let mut system = System::new().map_err(|err| anyhow!("cannot read processes: {err}"))?;
    let pid = resolve(&mut system, target)?;
    let refresh = ProcessRefreshKind::nothing().with_cpu().with_memory();
    let refresh_target = |system: &mut System| {
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, refresh)
    };
    // CPU usage needs two refreshes to have a time base.
    refresh_target(&mut system);
    if system.process(pid).is_none() {
        bail!("no process with pid {pid}");
    }
    let mut writer = csv::Writer::from_writer(out);
    let mut written = 0_u64;
    while max_samples.is_none_or(|max| written < max) {
        std::thread::sleep(interval.max(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL));
        refresh_target(&mut system);
        let Some(process) = system.process(pid) else {
            eprintln!("process {pid} exited; {written} samples written");
            break;
        };
        writer.serialize(Sample {
            utc: utc::format(now_unix()),
            cpu_percent_of_total: normalize_cpu(process.cpu_usage(), logical_cpus),
            rss_bytes: process.memory(),
        })?;
        writer.flush().context("cannot write samples")?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn cpu_is_a_share_of_all_cores() {
        assert_eq!(normalize_cpu(400.0, 8), 50.0);
        assert_eq!(normalize_cpu(100.0, 1), 100.0);
        assert_eq!(normalize_cpu(50.0, 0), 50.0, "zero cores treated as one");
    }

    #[test]
    fn csv_columns_and_round_trip() {
        let mut out = Vec::new();
        {
            let mut writer = csv::Writer::from_writer(&mut out);
            writer
                .serialize(Sample {
                    utc: "2026-11-01T12:00:05Z".into(),
                    cpu_percent_of_total: 1.5,
                    rss_bytes: 123_456,
                })
                .unwrap();
            writer.flush().unwrap();
        }
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "utc,cpu_percent_of_total,rss_bytes\n2026-11-01T12:00:05Z,1.5,123456\n"
        );
        let back: Sample = csv::Reader::from_reader(text.as_bytes())
            .deserialize()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(back.rss_bytes, 123_456);
    }

    #[test]
    fn samples_this_process() {
        let mut out = Vec::new();
        let written = run(
            &Target::Pid(std::process::id()),
            Duration::from_millis(1),
            Some(2),
            &mut out,
        )
        .unwrap();
        assert_eq!(written, 2);
        let rows: Vec<Sample> = csv::Reader::from_reader(out.as_slice())
            .deserialize()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.rss_bytes > 0));
        assert!(rows.iter().all(|r| utc::parse(&r.utc).is_ok()));
    }

    #[test]
    fn unknown_name_is_an_error() {
        let err = run(
            &Target::Name("perf-sampler-no-such-process-7f3a".into()),
            Duration::from_millis(1),
            Some(1),
            Vec::new(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("no process named"));
    }
}
