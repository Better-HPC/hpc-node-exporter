//! Traits for abstracting HPC job schedulers.

pub mod slurm;

use std::error::Error;

/// A system process managed by an HPC scheduler.
#[derive(Debug)]
pub struct HpcProcess {
    pub jobid: String,
    pub stepid: String,
    pub pid: u32,
}

/// Discovers active processes from an HPC job scheduler.
pub trait HpcScheduler {
    /// Returns the currently active HPC processes.
    ///
    /// # Errors
    ///
    /// Returns an error if the scheduler cannot be queried.
    fn get_processes(&self) -> Result<Vec<HpcProcess>, Box<dyn Error>>;
}
