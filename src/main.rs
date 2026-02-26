mod cli;
mod schedulers;

use crate::cli::Args;
use crate::schedulers::slurm::SlurmScheduler;
use crate::schedulers::HpcScheduler;

fn main() {
    let args = Args::parse();
    let scheduler = SlurmScheduler::default();

    if args.sys_job {
        println!("Sys job is running");
    }

    if args.sys_node {
        println!("Sys node is running");
    }

    if args.nvidia_job {
        println!("Nvidia job is running");
    }

    if args.nvidia_node {
        println!("Nvidia node is running");
    }

    match scheduler.get_processes() {
        Ok(processes) => {
            for p in processes {
                println!("{} {} {} {}", p.scheduler, p.jobid, p.stepid, p.pid);
            }
        }
        Err(e) => {
            eprintln!("failed to fetch job pids: {e}");
        }
    }
}
