mod cli;
mod profilers;
mod schedulers;

use crate::cli::Args;
use crate::profilers::system::SystemProfiler;
use crate::profilers::Profiler;
use crate::schedulers::slurm::SlurmScheduler;
use crate::schedulers::HpcScheduler;

fn main() {
    let args = Args::parse();
    let scheduler = SlurmScheduler::default();

    // Build the list of enabled profilers from CLI flags
    let mut profilers: Vec<Box<dyn Profiler>> = Vec::new();

    if args.system {
        profilers.push(Box::new(SystemProfiler::default()));
    }

    // Validate that all enabled profilers are supported
    for profiler in &profilers {
        if let Err(reason) = profiler.is_supported() {
            eprintln!("profiler not supported: {reason}");
            std::process::exit(1);
        }
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
