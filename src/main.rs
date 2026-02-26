mod cli;
mod schedulers;

use crate::cli::Args;
use crate::schedulers::slurm::SlurmScheduler;

fn main() {
    let args = Args::parse();
    let exporter = SlurmScheduler {};

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
}
