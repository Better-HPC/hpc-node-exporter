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

/// Initialize a profiler or exit with an error message.
///
/// Unwraps the profiler construction result and boxes it as a trait
/// object. If initialization fails, the error is printed to stderr
/// and the process exits immediately — profiler availability is
/// considered a hard requirement at startup.
///
/// # Arguments
///
/// * `result` - The result of constructing the profiler.
/// * `name` - A human-readable name for the profiler, used in error messages.
///
/// # Returns
///
/// A boxed [`Profiler`] trait object ready for use by the collector.
fn init_profiler<P: Profiler + Send + 'static>(
    result: Result<P, Box<dyn Error>>,
    name: &str,
) -> Box<dyn Profiler + Send> {
    match result {
        Ok(p) => Box::new(p),
        Err(e) => {
            error!("failed to initialize {name} profiler: {e}");
            std::process::exit(1);
        }
    }
}

/// Parse arguments, start the collector thread, and run the HTTP server.
///
/// The process exits with status 1 if no profilers are enabled or the
/// HTTP server fails to start.
#[tokio::main]
async fn main() {
    env_logger::init();

    let args = Args::parse();

    let hpc_scheduler = Box::new(SlurmScheduler::default());

    let mut profilers: Vec<Box<dyn Profiler + Send>> = Vec::new();
    if args.system {
        profilers.push(init_profiler(SystemProfiler::new(), "system"));
    }

    if args.nvidia {
        profilers.push(init_profiler(NvidiaProfiler::new(), "NVIDIA"));
    }

    if profilers.is_empty() {
        error!("no profilers enabled — specify one or more profilers using CLI flags");
        std::process::exit(1);
    }

    // Instantiate a metrics store for caching metrics in between profiler passes
    let metrics_store = Arc::new(ArcSwap::from_pointee(String::new()));

    // Spawn the background collector thread
    let interval = Duration::from_secs(args.interval);
    collector::spawn(
        profilers,
        hpc_scheduler,
        Arc::clone(&metrics_store),
        interval,
    );

    // Start the HTTP server on the async runtime
    info!("starting HTTP server on {}:{}", args.host, args.port);
    if let Err(e) = api::serve(&args.host, args.port, metrics_store).await {
        error!("server error: {e}");
        std::process::exit(1);
    }
}
