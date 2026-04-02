//! Command-line argument parsing.

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "hpc-node-exporter",
    about = "A job-aware Prometheus exporter for the HPC systems.",
    version
)]
pub struct Args {
    /// Enable system CPU metrics.
    #[arg(long)]
    pub system: bool,

    /// Enable NVIDIA GPU metrics.
    #[arg(long)]
    pub nvidia: bool,

    /// Host interface to bind to.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Host port to listen on.
    #[arg(long, default_value_t = 9105)]
    pub port: u16,

    /// Metric collection interval in seconds.
    #[arg(long, default_value_t = 1, value_name = "SECONDS")]
    pub interval: u64,

    /// Timeout in seconds for scheduler commands (e.g. scontrol).
    #[arg(long, default_value_t = 30, value_name = "SECONDS")]
    pub sched_timeout: u64,

    /// Suppress console log output.
    #[arg(long)]
    pub quiet: bool,
}

impl Args {
    /// Parses command-line arguments.
    pub fn parse() -> Self {
        Parser::parse()
    }
}
