//! Entry point for `keystone-exporter`.
//!
//! Parses command-line arguments, initializes profilers and the job
//! scheduler, starts background metrics collection, and launches the
//! HTTP server.

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
use crate::profilers::default::DefaultProfiler;
use crate::profilers::nvidia::NvidiaProfiler;
use crate::profilers::system::SystemProfiler;
use crate::profilers::Profiler;
use crate::schedulers::slurm::SlurmScheduler;
use crate::schedulers::HpcScheduler;

/// Configures logging to syslog and optionally to stdout.
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

/// Initializes the HPC job scheduler.
fn init_hpc_scheduler() -> Box<dyn HpcScheduler + Send> {
    Box::new(SlurmScheduler::default())
}

/// Initializes hardware profilers based on the requested flags.
///
/// A [`DefaultProfiler`] is always included. Additional profilers are
/// added when the corresponding flag is `true`.
///
/// # Errors
///
/// Returns an error if a requested profiler fails to initialize.
fn init_profilers(
    system: bool,
    nvidia: bool,
) -> Result<Vec<Box<dyn Profiler + Send>>, Box<dyn Error>> {
    let mut profilers: Vec<Box<dyn Profiler + Send>> = Vec::new();

    // The default profiler is always enabled
    let default_profiler = DefaultProfiler::new();
    profilers.push(Box::new(default_profiler));

    if system {
        profilers.push(Box::new(SystemProfiler::new()?));
    }

    if nvidia {
        profilers.push(Box::new(NvidiaProfiler::new()?));
    }

    Ok(profilers)
}

/// Parses arguments, starts the collector thread, and runs the HTTP server.
///
/// Exits with status 1 on fatal initialization or server errors.
#[tokio::main]
async fn main() {
    let args = Args::parse();

    init_logging(args.quiet).expect("Failed to initialize logging");

    // Initialize system interfaces
    let hpc_scheduler = init_hpc_scheduler();
    let hardware_profilers = init_profilers(args.system, args.nvidia).unwrap_or_else(|e| {
        error!("failed to initialize profilers: {e}");
        std::process::exit(1);
    });

    // Launch metrics collection
    let metrics_store = Arc::new(ArcSwap::from_pointee(String::new()));
    collector::spawn(
        hardware_profilers,
        hpc_scheduler,
        Arc::clone(&metrics_store),
        Duration::from_secs(args.interval),
    );

    // Launch metrics server
    info!("starting HTTP server on {}:{}", args.host, args.port);
    api::serve(&args.host, args.port, metrics_store)
        .await
        .unwrap_or_else(|e| {
            error!("server error: {e}");
            std::process::exit(1);
        });
}
