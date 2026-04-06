//! The application command-line interface.
//!
//! This module is responsible for defining the application command-line
//! interface and provides for argument parsing and validation.

use clap::Parser;

const PROFILE_HEADER: &str = "Profiling";
const PROM_HEADER: &str = "Serve Metrics";
const DB_HEADER: &str = "Push Metrics";

#[derive(Parser, Debug)]
#[command(
    name = "hpc-node-exporter",
    about = "A job-aware metrics exporter for HPC systems.",
    version
)]
pub struct Args {
    /// Suppress console log output.
    #[arg(long, short)]
    pub quiet: bool,

    /// Enable system CPU and memory metrics.
    #[arg(long, help_heading = PROFILE_HEADER)]
    pub system: bool,

    /// Enable NVIDIA GPU metrics.
    #[arg(long, help_heading = PROFILE_HEADER)]
    pub nvidia: bool,

    /// Metric collection interval in seconds.
    #[arg(
        long,
        default_value_t = 1,
        value_name = "SECONDS",
        help_heading = PROFILE_HEADER
    )]
    pub interval: u64,

    /// Timeout in seconds for scheduler commands (e.g. scontrol).
    #[arg(
        long,
        default_value_t = 30,
        value_name = "SECONDS",
        help_heading = PROFILE_HEADER
    )]
    pub timeout: u64,

    /// Host interface to bind the Prometheus scrape endpoint to.
    #[arg(
        long,
        default_value = "127.0.0.1",
        help_heading = PROM_HEADER
    )]
    pub host: String,

    /// Port to listen on for Prometheus scrape requests.
    #[arg(
        long,
        default_value_t = 9105,
        help_heading = PROM_HEADER
    )]
    pub port: u16,

    /// Post metrics in Prometheus format to the given URL.
    #[arg(long, value_name = "URL", help_heading = DB_HEADER)]
    pub push_url: Option<String>,

    /// Timeout in seconds for an individual POST request.
    #[arg(
        long,
        default_value_t = 10,
        value_name = "SECONDS",
        requires = "push_url",
        help_heading = DB_HEADER
    )]
    pub push_timeout: u64,
}
