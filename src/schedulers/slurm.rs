//! [`HpcScheduler`] implementation for the [Slurm](https://slurm.schedmd.com/) workload manager.
//!
//! Discovers active jobs and their associated PIDs by parsing the output of
//! `scontrol listpids`, which produces a whitespace-delimited table:
//!
//! ```text
//! PID    JOBID  STEPID
//! 12345  100    0
//! 12346  100    0
//! -1     101    0
//! ```
//!
//! Pending processes are represented by Slurm as PID `-1` and are excluded
//! from the results.

use std::collections::HashMap;
use std::io;
use std::process::Command;

use crate::schedulers::{HpcJob, HpcScheduler};

/// A [`HpcScheduler`] that discovers jobs via the Slurm `scontrol` CLI.
pub struct SlurmScheduler {}

impl SlurmScheduler {

    /// Execute `scontrol listpids` and return the output lines.
    ///
    /// The header row is stripped from the output before returning.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] if the command fails to spawn or exits with
    /// a non-zero status code.
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

    /// Parse a single line of `scontrol listpids` output.
    ///
    /// Expects whitespace-delimited columns in the order: `PID JOBID STEPID`.
    ///
    /// # Arguments
    ///
    /// * `line` - A single row from the `scontrol listpids` output table.
    ///
    /// # Returns
    ///
    /// A `(jobid, stepid, pid)` tuple, or `None` if the line is malformed,
    /// the PID is not a valid integer, or the PID is negative (pending).
    fn parse_scontrol_line(line: &str) -> Option<(String, String, u32)> {
        let parts: Vec<&str> = line.split_whitespace().collect();

        // Skip malformed lines
        if parts.len() != 3 {
            return None;
        }

        // Parse PID as i64 first so we can detect Slurm's -1 sentinel for pending PIDs
        // Cast it back to a u32 type later on
        let pid: i64 = parts[0].parse().ok()?;
        if pid < 0 {
            return None;
        }

        let pid: u32 = pid.try_into().ok()?;
        Some((parts[1].to_string(), parts[2].to_string(), pid))
    }
}

impl HpcScheduler for SlurmScheduler {
    /// Discover active Slurm jobs and their PIDs.
    ///
    /// Calls `scontrol listpids`, parses the output, and groups PIDs by
    /// their job and step ID. Returns an empty list on failure.
    fn get_job_pids(&self) -> Vec<HpcJob> {
        let lines = match SlurmScheduler::fetch_scontrol_lines() {
            Ok(lines) => lines,
            Err(e) => {
                eprintln!("failed to fetch job pids: {}", e);
                return Vec::new();
            }
        };

        // Group PIDs by (jobid, stepid) so each HpcJob contains all PIDs for that step
        let mut jobs: HashMap<(String, String), HpcJob> = HashMap::new();

        for line in &lines {
            let (jobid, stepid, pid) = match SlurmScheduler::parse_scontrol_line(line) {
                Some(parsed) => parsed,
                None => continue,
            };

            let key = (jobid.clone(), stepid.clone());
            jobs.entry(key)
                .or_insert_with(|| HpcJob {
                    jobid,
                    stepid,
                    pids: Vec::new(),
                })
                .pids
                .push(pid);
        }

        jobs.into_values().collect()
    }
}
