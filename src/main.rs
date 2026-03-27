//! Entry point for the `keystone-exporter` binary.
//!
//! Parses command-line arguments, initializes the requested profilers,
//! starts background metrics collection, and launches the HTTP server.

mod api;
mod cli;
mod collector;
mod profilers;
mod schedulers;

use arc_swap::ArcSwap;
use log::{error, info};
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use crate::cli::Args;
use crate::profilers::nvidia::NvidiaProfiler;
use crate::profilers::system::SystemProfiler;
use crate::profilers::Profiler;
use crate::schedulers::slurm::SlurmScheduler;

/// Configure logging to syslog and optionally to stdout.
///
/// # Arguments
///
/// * `quiet` - When `true`, suppresses console log output.
///
/// # Errors
///
/// Returns an error if the syslog socket cannot be opened or the
/// global logger has already been set.
fn init_logging(quiet: bool) -> Result<(), Box<dyn Error>> {
    let syslog_formatter = syslog::Formatter3164 {
        facility: syslog::Facility::LOG_USER,
        hostname: None,
        process: "keystone-exporter".to_owned(),
        pid: 0,
    };

    let format =
        |out: fern::FormatCallback, message: &std::fmt::Arguments, record: &log::Record| {
            out.finish(format_args!("[{}] {}", record.level(), message))
        };

    let mut config = fern::Dispatch::new()
        .level(log::LevelFilter::Info)
        .format(format)
        .chain(syslog::unix(syslog_formatter)?);

    if !quiet {
        config = config.chain(std::io::stdout());
    }

    config.apply()?;
    Ok(())
}

/// Initialize hardware profilers.
///
/// Returns a vector of boxed profiler trait objects. Exits the process
/// if a requested profiler fails to initialize or if no profilers are
/// enabled.
///
/// # Arguments
///
/// * `system` - Whether to enable the system CPU/memory profiler.
/// * `nvidia` - Whether to enable the NVIDIA GPU profiler.
fn init_profilers(system: bool, nvidia: bool) -> Vec<Box<dyn Profiler + Send>> {
    let mut profilers: Vec<Box<dyn Profiler + Send>> = Vec::new();

    if system {
        match SystemProfiler::new() {
            Ok(p) => profilers.push(Box::new(p)),
            Err(e) => {
                error!("failed to initialize system profiler: {e}");
                std::process::exit(1);
            }
        }
    }

    if nvidia {
        match NvidiaProfiler::new() {
            Ok(p) => profilers.push(Box::new(p)),
            Err(e) => {
                error!("failed to initialize NVIDIA profiler: {e}");
                std::process::exit(1);
            }
        }
    }

    if profilers.is_empty() {
        error!("no profilers enabled — specify one or more profilers using CLI flags");
        std::process::exit(1);
    }

    profilers
}

/// Parse arguments, start the collector thread, and run the HTTP server.
///
/// The process exits with status 1 if no profilers are enabled or the
/// HTTP server fails to start.
#[tokio::main]
async fn main() {
    let args = Args::parse();

    init_logging(args.quiet).expect("Failed to initialize logging");

    let hpc_scheduler = Box::new(SlurmScheduler::default());
    let hardware_profilers = init_profilers(args.system, args.nvidia);
    let metrics_store = Arc::new(ArcSwap::from_pointee(String::new()));

    let interval = Duration::from_secs(args.interval);
    collector::spawn(hardware_profilers, hpc_scheduler, Arc::clone(&metrics_store), interval);

    info!("starting HTTP server on {}:{}", args.host, args.port);
    if let Err(e) = api::serve(&args.host, args.port, metrics_store).await {
        error!("server error: {e}");
        std::process::exit(1);
    }
}
