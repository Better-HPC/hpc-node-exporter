//! Argument parsing for the package's command-line interface.

use clap::Parser;

/// Parsed command-line arguments.
#[derive(Parser, Debug)]
#[command(
    name = "keystone-exporter",
    about = "A job-aware Prometheus exporter for the Keystone HPC platform.",
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

    /// Suppress console log output.
    #[arg(long)]
    pub quiet: bool,
}

impl Args {
    /// Parse command-line arguments and return them as an [`Args`] instance.
    pub fn parse() -> Self {
        Parser::parse()
    }
}
