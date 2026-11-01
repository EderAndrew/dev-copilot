//! Release measurement tool (specs/002-desktop-release-readiness, research R7).
//!
//! Development tooling only: not a dependency of any app crate and never distributed.
//!
//! ```text
//! perf-sampler sample (--process <name> | --pid <pid>) [--interval <s>] [--count <n>] --out <csv>
//! perf-sampler summarize --metrics <csv>
//! perf-sampler summarize --samples <csv> [--warmup-min <min>]
//! ```

mod sample;
mod stats;
mod summarize;
mod utc;

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};

/// Samples the app's CPU and memory and summarizes measurement CSVs.
#[derive(Debug, Parser)]
#[command(name = "perf-sampler", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Sample CPU (percent of total capacity) and RSS of one process into a CSV.
    Sample(SampleArgs),
    /// Summarize a metrics CSV (latency) or a samples CSV (CPU and RSS growth).
    Summarize(SummarizeArgs),
}

#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("target").required(true).args(["process", "pid"])))]
struct SampleArgs {
    /// Exact process name (must match a single process).
    #[arg(long)]
    process: Option<String>,
    /// Process id.
    #[arg(long)]
    pid: Option<u32>,
    /// Seconds between samples.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..))]
    interval: u64,
    /// Stop after this many samples (default: until the process exits).
    #[arg(long)]
    count: Option<u64>,
    /// Output CSV (`utc,cpu_percent_of_total,rss_bytes`).
    #[arg(long)]
    out: PathBuf,
}

#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("input").required(true).args(["metrics", "samples"])))]
struct SummarizeArgs {
    /// Metrics CSV written by the app with DEVCOPILOT_METRICS=1.
    #[arg(long)]
    metrics: Option<PathBuf>,
    /// Samples CSV written by `sample`.
    #[arg(long)]
    samples: Option<PathBuf>,
    /// Minutes after the first sample that are excluded as warm-up (samples only; default 10).
    #[arg(long, conflicts_with = "metrics")]
    warmup_min: Option<u32>,
}

/// Warm-up excluded from a samples summary (measurement plan: 10 minutes).
const DEFAULT_WARMUP_MIN: u32 = 10;

fn open(path: &PathBuf) -> Result<BufReader<File>> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    Ok(BufReader::new(file))
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Sample(args) => {
            let target = match (args.process, args.pid) {
                (_, Some(pid)) => sample::Target::Pid(pid),
                (Some(name), None) => sample::Target::Name(name),
                (None, None) => unreachable!("clap requires --process or --pid"),
            };
            let out = File::create(&args.out)
                .with_context(|| format!("cannot create {}", args.out.display()))?;
            let written =
                sample::run(&target, Duration::from_secs(args.interval), args.count, out)?;
            eprintln!("{written} samples written to {}", args.out.display());
        }
        Command::Summarize(args) => {
            if let Some(path) = &args.metrics {
                print!("{}", summarize::latency(open(path)?)?);
            }
            if let Some(path) = &args.samples {
                print!(
                    "{}",
                    summarize::resources(
                        open(path)?,
                        args.warmup_min.unwrap_or(DEFAULT_WARMUP_MIN)
                    )?
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("perf-sampler").chain(args.iter().copied()))
    }

    #[test]
    fn cli_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn sample_by_name_with_defaults() {
        let Command::Sample(args) = parse(&["sample", "--process", "devcopilot", "--out", "s.csv"])
            .unwrap()
            .command
        else {
            panic!("not sample");
        };
        assert_eq!(args.process.as_deref(), Some("devcopilot"));
        assert_eq!((args.pid, args.interval, args.count), (None, 5, None));
    }

    #[test]
    fn sample_by_pid() {
        let Command::Sample(args) = parse(&[
            "sample",
            "--pid",
            "42",
            "--interval",
            "2",
            "--count",
            "3",
            "--out",
            "s.csv",
        ])
        .unwrap()
        .command
        else {
            panic!("not sample");
        };
        assert_eq!(
            (args.pid, args.interval, args.count),
            (Some(42), 2, Some(3))
        );
    }

    #[test]
    fn sample_rejects_bad_targets_and_intervals() {
        assert!(parse(&["sample", "--out", "s.csv"]).is_err(), "no target");
        assert!(
            parse(&["sample", "--pid", "1", "--process", "x", "--out", "s.csv"]).is_err(),
            "two targets"
        );
        assert!(
            parse(&["sample", "--pid", "1", "--interval", "0", "--out", "s.csv"]).is_err(),
            "zero interval"
        );
        assert!(parse(&["sample", "--pid", "1"]).is_err(), "no output");
    }

    #[test]
    fn summarize_inputs() {
        let Command::Summarize(args) = parse(&["summarize", "--samples", "s.csv"]).unwrap().command
        else {
            panic!("not summarize");
        };
        assert_eq!(args.warmup_min, None);
        assert!(parse(&["summarize", "--samples", "s.csv", "--warmup-min", "5"]).is_ok());
        assert!(parse(&["summarize", "--metrics", "m.csv"]).is_ok());
        assert!(parse(&["summarize"]).is_err(), "no input");
        assert!(
            parse(&["summarize", "--metrics", "m.csv", "--samples", "s.csv"]).is_err(),
            "one input at a time"
        );
        assert!(
            parse(&["summarize", "--metrics", "m.csv", "--warmup-min", "5"]).is_err(),
            "warm-up only applies to samples"
        );
    }
}
