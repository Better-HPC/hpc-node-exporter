pub mod slurm;

/// An job reported by an HPC scheduler.
pub struct HpcJob {
    pub jobid: String,
    pub stepid: String,
    pub pids: Vec<u32>,
}

pub trait HpcScheduler {
    /// Discover active HPC jobs from the scheduler and return their PIDs.
    fn get_job_pids(&self) -> Vec<HpcJob>;
}
