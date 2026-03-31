//! [`HpcScheduler`] implementation for the [Slurm] workload manager.
//!
//! Discovers active jobs and their PIDs by parsing `scontrol listpids`
//! output. Pending processes (PID `-1`) are excluded.

use std::error::Error;
use std::io;
use std::process::Command;

use crate::schedulers::{HpcProcess, HpcScheduler};

/// A [`HpcScheduler`] interface for Slurm.
#[derive(Debug, Default)]
pub struct SlurmScheduler;

impl SlurmScheduler {
    /// Executes `scontrol listpids` and returns the body lines (header stripped).
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails to spawn or exits non-zero.
    fn fetch_scontrol_lines() -> io::Result<Vec<String>> {
        let output = Command::new("scontrol").arg("listpids").output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("scontrol listpids failed ({}): {}", output.status, stderr),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.trim().lines().skip(1).map(String::from).collect())
    }

    /// Parses a single `scontrol listpids` output line.
    ///
    /// Expects whitespace-delimited columns: `PID JOBID STEPID`.
    /// Returns `(jobid, stepid, pid)` for successfully parsed lines
    /// and `None` for malformed lines or negative (pending) PIDs.
    fn parse_scontrol_line(line: &str) -> Option<(String, String, u32)> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            return None;
        }

        // Parse as i64 first to detect Slurm's -1 sentinel for pending PIDs.
        let pid: i64 = parts[0].parse().ok()?;
        if pid < 0 {
            return None;
        }

        let pid: u32 = pid.try_into().ok()?;
        Some((parts[1].to_string(), parts[2].to_string(), pid))
    }
}

impl HpcScheduler for SlurmScheduler {
    /// Returns the currently active HPC processes.
    fn get_processes(&self) -> Result<Vec<HpcProcess>, Box<dyn Error>> {
        let lines = SlurmScheduler::fetch_scontrol_lines()?;
        Ok(lines
            .iter()
            .filter_map(|line| SlurmScheduler::parse_scontrol_line(line))
            .map(|(jobid, stepid, pid)| HpcProcess { jobid, stepid, pid })
            .collect())
    }
}
