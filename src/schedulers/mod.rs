pub mod slurm;

use std::error::Error;

/// A system process being run by an HPC scheduler.
#[derive(Debug)]
pub struct HpcProcess {
    pub scheduler: &'static str,
    pub jobid: String,
    pub stepid: String,
    pub pid: u32,
}

pub trait HpcScheduler {
    /// Discover active system processes from the HPC scheduler.
    fn get_processes(&self) -> Result<Vec<HpcProcess>, Box<dyn Error>>;
}
