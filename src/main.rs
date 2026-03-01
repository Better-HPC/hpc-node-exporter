mod cli;
mod profilers;
mod schedulers;

use std::error::Error;

use crate::cli::Args;
use crate::profilers::nvidia::NvidiaProfiler;
use crate::profilers::system::SystemProfiler;
use crate::profilers::Profiler;
use crate::schedulers::slurm::SlurmScheduler;
use crate::schedulers::HpcScheduler;

/// Initialize a profiler or exit with an error message.
fn init_profiler<P: Profiler + 'static>(
    result: Result<P, Box<dyn Error>>,
    name: &str,
) -> Box<dyn Profiler> {
    match result {
        Ok(p) => Box::new(p),
        Err(e) => {
            eprintln!("failed to initialize {name} profiler: {e}");
            std::process::exit(1);
        }
    }
}

fn main() {
    let args = Args::parse();
    let scheduler = SlurmScheduler::default();

    // Build the list of enabled profilers from CLI flags
    let mut profilers: Vec<Box<dyn Profiler>> = Vec::new();

    if args.system {
        profilers.push(init_profiler(SystemProfiler::new(), "system"));
    }

    if args.nvidia {
        profilers.push(init_profiler(NvidiaProfiler::new(), "NVIDIA"));
    }

    // Fetch active processes from the scheduler
    let processes = match scheduler.get_processes() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("failed to fetch job pids: {e}");
            std::process::exit(1);
        }
    };

    // Collect and display metrics from each enabled profiler
    for profiler in &mut profilers {
        match profiler.collect_metrics(&processes) {
            Ok(metrics) => {
                for m in &metrics {
                    println!("{}", m.to_prometheus());
                }
            }
            Err(e) => {
                eprintln!("failed to collect metrics: {e}");
            }
        }
    }
}
