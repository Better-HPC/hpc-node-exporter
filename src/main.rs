mod cli;

use crate::cli::Args;

fn main() {
    let args = Args::parse();

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
