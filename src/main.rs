mod api;
mod cli;
mod collector;
mod profilers;
mod schedulers;

use arc_swap::ArcSwap;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use crate::cli::Args;
use crate::profilers::nvidia::NvidiaProfiler;
use crate::profilers::system::SystemProfiler;
use crate::profilers::Profiler;
use crate::schedulers::slurm::SlurmScheduler;

/// Initialize a profiler or exit with an error message.
fn init_profiler<P: Profiler + Send + 'static>(
    result: Result<P, Box<dyn Error>>,
    name: &str,
) -> Box<dyn Profiler + Send> {
    match result {
        Ok(p) => Box::new(p),
        Err(e) => {
            eprintln!("failed to initialize {name} profiler: {e}");
            std::process::exit(1);
        }
    }
}

#[tokio::main]
async fn main() {
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
        eprintln!("no profilers enabled. Specify one or more profilers using CLI flags.");
        std::process::exit(1);
    }

    let metrics_store = Arc::new(ArcSwap::from_pointee(String::new()));

    // Spawn the background collector thread
    let interval = Duration::from_secs(args.interval);
    collector::spawn(profilers, hpc_scheduler, Arc::clone(&metrics_store), interval);

    // Start the HTTP server on the async runtime
    if let Err(e) = api::serve(&args.host, args.port, metrics_store).await {
        eprintln!("server error: {e}");
        std::process::exit(1);
    }
}
