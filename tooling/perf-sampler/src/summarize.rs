//! `summarize`: latency from a metrics CSV, CPU and RSS growth from a samples CSV.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;

use anyhow::{Context, Result, bail};

use crate::sample::Sample;
use crate::stats::{Summary, summarize};
use crate::utc;

/// Columns written by the app's metrics mode (002 T014).
pub const METRICS_HEADER: [&str; 5] = [
    "kind",
    "speech_start_ms",
    "speech_end_ms",
    "emitted_ms",
    "provider",
];

/// Latency summary of a metrics CSV.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatencyReport {
    /// First partial of each utterance: `emitted − speech_start`.
    pub first_partial: Option<Summary>,
    /// Each final: `emitted − speech_end`.
    pub finals: Option<Summary>,
}

fn write_summary(f: &mut fmt::Formatter<'_>, name: &str, summary: Option<&Summary>) -> fmt::Result {
    match summary {
        Some(s) => writeln!(
            f,
            "{name}: count={} p50={} p95={} max={}",
            s.count, s.p50, s.p95, s.max
        ),
        None => writeln!(f, "{name}: count=0"),
    }
}

impl fmt::Display for LatencyReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_summary(f, "first_partial_latency_ms", self.first_partial.as_ref())?;
        write_summary(f, "final_latency_ms", self.finals.as_ref())
    }
}

fn signed(value: u64) -> Result<i64> {
    i64::try_from(value).context("timing out of range")
}

/// Reads a metrics CSV. Partials sharing a `speech_start_ms` belong to one utterance; only
/// the earliest is its first partial.
///
/// # Errors
///
/// The header is not the metrics header, or a row is malformed or has an unknown kind.
pub fn latency(input: impl Read) -> Result<LatencyReport> {
    let mut reader = csv::Reader::from_reader(input);
    let header: Vec<String> = reader.headers()?.iter().map(str::to_owned).collect();
    if header != METRICS_HEADER {
        bail!("not a metrics CSV: header is {header:?}, expected {METRICS_HEADER:?}");
    }
    // (provider, speech_start) → earliest partial emission.
    let mut first_partials: BTreeMap<(String, u64), u64> = BTreeMap::new();
    let mut finals = Vec::new();
    for (index, record) in reader.records().enumerate() {
        let line = index + 2;
        let record = record.with_context(|| format!("line {line}"))?;
        let number = |column: usize| -> Result<u64> {
            record
                .get(column)
                .unwrap_or_default()
                .parse::<u64>()
                .with_context(|| {
                    format!(
                        "line {line}: column {} is not a number",
                        METRICS_HEADER[column]
                    )
                })
        };
        let (start, end, emitted) = (number(1)?, number(2)?, number(3)?);
        let provider = record.get(4).unwrap_or_default().to_owned();
        match record.get(0).unwrap_or_default() {
            "partial" => {
                first_partials
                    .entry((provider, start))
                    .and_modify(|first| *first = (*first).min(emitted))
                    .or_insert(emitted);
            }
            "final" => finals.push(signed(emitted)? - signed(end)?),
            other => bail!("line {line}: unknown kind `{other}`"),
        }
    }
    let partials = first_partials
        .into_iter()
        .map(|((_, start), emitted)| Ok(signed(emitted)? - signed(start)?))
        .collect::<Result<Vec<_>>>()?;
    Ok(LatencyReport {
        first_partial: summarize(partials),
        finals: summarize(finals),
    })
}

/// CPU and memory summary of a samples CSV after the warm-up.
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceReport {
    /// All samples read.
    pub samples: usize,
    /// Samples at or after the end of the warm-up.
    pub after_warmup: usize,
    /// Mean CPU after the warm-up, percent of total capacity.
    pub cpu_mean_percent: f64,
    /// RSS of the first sample at or after the end of the warm-up.
    pub rss_baseline_bytes: u64,
    /// RSS of the last sample.
    pub rss_last_bytes: u64,
    /// `(last − baseline) / baseline × 100`.
    pub rss_growth_percent: f64,
}

impl fmt::Display for ResourceReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "samples: {} (after warm-up: {})",
            self.samples, self.after_warmup
        )?;
        writeln!(f, "cpu_mean_percent_of_total: {:.2}", self.cpu_mean_percent)?;
        writeln!(
            f,
            "rss_bytes: baseline={} last={} growth_percent={:+.2}",
            self.rss_baseline_bytes, self.rss_last_bytes, self.rss_growth_percent
        )
    }
}

/// Reads a samples CSV and summarizes it from `warmup_min` minutes after its first sample.
///
/// # Errors
///
/// The CSV is malformed, or no sample falls after the warm-up.
pub fn resources(input: impl Read, warmup_min: u32) -> Result<ResourceReport> {
    let mut reader = csv::Reader::from_reader(input);
    let mut samples = Vec::new();
    for (index, row) in reader.deserialize::<Sample>().enumerate() {
        let sample = row.with_context(|| format!("line {}", index + 2))?;
        let at = utc::parse(&sample.utc).with_context(|| format!("line {}", index + 2))?;
        samples.push((at, sample));
    }
    let Some(&(start, _)) = samples.first() else {
        bail!("no samples");
    };
    let warm = start + i64::from(warmup_min) * 60;
    let after: Vec<&Sample> = samples
        .iter()
        .filter(|(at, _)| *at >= warm)
        .map(|(_, sample)| sample)
        .collect();
    let (Some(baseline), Some(last)) = (after.first(), after.last()) else {
        bail!(
            "no sample after the {warmup_min}-minute warm-up ({} samples over {} s)",
            samples.len(),
            samples.last().map_or(0, |(at, _)| at - start)
        );
    };
    #[allow(clippy::cast_precision_loss)] // sample counts and byte sizes far below 2^52
    let report = ResourceReport {
        samples: samples.len(),
        after_warmup: after.len(),
        cpu_mean_percent: after.iter().map(|s| s.cpu_percent_of_total).sum::<f64>()
            / after.len() as f64,
        rss_baseline_bytes: baseline.rss_bytes,
        rss_last_bytes: last.rss_bytes,
        rss_growth_percent: if baseline.rss_bytes == 0 {
            0.0
        } else {
            (last.rss_bytes as f64 - baseline.rss_bytes as f64) / baseline.rss_bytes as f64 * 100.0
        },
    };
    Ok(report)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    const HEADER: &str = "kind,speech_start_ms,speech_end_ms,emitted_ms,provider\n";
    const P: &str = "0b9c6a8e-1f1a-4c1e-9a55-3f1d2b7c9e10";

    #[test]
    fn first_partial_and_final_latency() {
        let csv = format!(
            "{HEADER}\
             partial,1000,1500,2500,{P}\n\
             partial,1000,2000,3100,{P}\n\
             final,1000,3000,5000,{P}\n\
             partial,6000,6200,9000,{P}\n\
             final,6000,8000,8500,{P}\n"
        );
        let report = latency(csv.as_bytes()).unwrap();
        // First partials: 2500-1000 = 1500 and 9000-6000 = 3000 (the later partial is ignored).
        assert_eq!(
            report.first_partial,
            Some(Summary {
                count: 2,
                p50: 1500,
                p95: 3000,
                max: 3000
            })
        );
        // Finals: 5000-3000 = 2000 and 8500-8000 = 500.
        assert_eq!(
            report.finals,
            Some(Summary {
                count: 2,
                p50: 500,
                p95: 2000,
                max: 2000
            })
        );
        assert_eq!(
            report.to_string(),
            "first_partial_latency_ms: count=2 p50=1500 p95=3000 max=3000\n\
             final_latency_ms: count=2 p50=500 p95=2000 max=2000\n"
        );
    }

    #[test]
    fn earliest_partial_wins_even_out_of_order() {
        let csv = format!("{HEADER}partial,0,0,900,{P}\npartial,0,0,400,{P}\n");
        let report = latency(csv.as_bytes()).unwrap();
        assert_eq!(report.first_partial.unwrap().max, 400);
        assert_eq!(report.finals, None);
    }

    #[test]
    fn rejects_foreign_or_malformed_csv() {
        assert!(latency("utc,cpu_percent_of_total,rss_bytes\n".as_bytes()).is_err());
        let unknown = format!("{HEADER}gap,0,0,1,{P}\n");
        assert!(latency(unknown.as_bytes()).is_err());
        let not_number = format!("{HEADER}final,0,x,1,{P}\n");
        assert!(latency(not_number.as_bytes()).is_err());
    }

    fn samples_csv(rows: &[(i64, f64, u64)]) -> String {
        let mut csv = String::from("utc,cpu_percent_of_total,rss_bytes\n");
        for (secs, cpu, rss) in rows {
            csv.push_str(&format!(
                "{},{cpu},{rss}\n",
                utc::format(1_800_000_000 + secs)
            ));
        }
        csv
    }

    #[test]
    fn resources_after_warmup() {
        let csv = samples_csv(&[
            (0, 90.0, 100),   // warm-up, ignored
            (300, 90.0, 150), // warm-up, ignored
            (600, 10.0, 200), // baseline at 10 min
            (900, 20.0, 210),
            (1_200, 30.0, 220),
        ]);
        let report = resources(csv.as_bytes(), 10).unwrap();
        assert_eq!(report.samples, 5);
        assert_eq!(report.after_warmup, 3);
        assert_eq!(report.cpu_mean_percent, 20.0);
        assert_eq!(
            (report.rss_baseline_bytes, report.rss_last_bytes),
            (200, 220)
        );
        assert_eq!(report.rss_growth_percent, 10.0);
        assert!(report.to_string().contains("growth_percent=+10.00"));
    }

    #[test]
    fn resources_needs_samples_after_warmup() {
        let short = samples_csv(&[(0, 1.0, 1), (60, 1.0, 1)]);
        assert!(resources(short.as_bytes(), 10).is_err());
        assert!(resources(samples_csv(&[]).as_bytes(), 10).is_err());
        let bad_time = "utc,cpu_percent_of_total,rss_bytes\nyesterday,1,1\n";
        assert!(resources(bad_time.as_bytes(), 0).is_err());
    }
}
