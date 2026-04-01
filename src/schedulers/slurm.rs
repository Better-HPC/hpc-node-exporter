//! [`HpcScheduler`] implementation for the [Slurm] workload manager.
//!
//! Discovers active jobs and their PIDs by parsing `scontrol listpids`
//! output. Pending processes (PID `-1`) are excluded.

use std::collections::HashMap;
use std::error::Error;
use std::io;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::Duration;

use log::warn;
use wait_timeout::ChildExt;

use crate::schedulers::{HpcProcess, HpcScheduler};

/// Column names expected in `scontrol listpids` output.
const COL_PID: &str = "PID";
const COL_JOBID: &str = "JOBID";
const COL_STEPID: &str = "STEPID";

/// A [`HpcScheduler`] interface for Slurm.
#[derive(Debug, Default)]
pub struct SlurmScheduler {
    command_timeout: Duration,
}

impl SlurmScheduler {
    pub fn new(command_timeout: Duration) -> Self {
        Self { command_timeout }
    }

    /// Executes `scontrol listpids` and returns the raw stdout output.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails or exceeds the configured timeout.
    fn fetch_scontrol_output(&self) -> io::Result<Vec<String>> {
        // Launch `scontrol` call in a dedicated thread.
        let mut child = Command::new("scontrol")
            .arg("listpids")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Block until the child thread exits or the timeout elapses.
        let status = match child.wait_timeout(self.command_timeout)? {
            Some(status) => status,
            None => {
                child.kill()?;
                child.wait()?;
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("scontrol timed out after {:?}", self.command_timeout),
                ));
            }
        };

        // Make sure scontrol call exited successfully.
        if !status.success() {
            let mut stderr = String::new();
            if let Some(mut err) = child.stderr.take() {
                let _ = err.read_to_string(&mut stderr);
            }

            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("scontrol listpids failed ({status}): {stderr}"),
            ));
        }

        // Read command results from stdout.
        let mut stdout = String::new();
        if let Some(mut out) = child.stdout.take() {
            out.read_to_string(&mut stdout)?;
        }

        // Clean output and return a vector of stdout lines
        Ok(stdout.trim().lines().map(String::from).collect())
    }

    /// Builds a mapping from column name to positional index by parsing the
    /// header row of `scontrol listpids` output.
    fn parse_scontrol_header(header: &str) -> HashMap<&str, usize> {
        header
            .split_whitespace()
            .enumerate()
            .map(|(i, name)| (name, i))
            .collect()
    }

    /// Parses a single `scontrol listpids` data row using the given column indices.
    ///
    /// Returns `(jobid, stepid, pid)` for valid rows and `None` for
    /// malformed lines or negative (pending) PIDs.
    fn parse_scontrol_line(
        line: &str,
        pid_col_idx: usize,
        jobid_col_idx: usize,
        stepid_col_idx: usize,
    ) -> Option<(String, String, u32)> {
        let parts: Vec<&str> = line.split_whitespace().collect();

        let pid_str = parts.get(pid_col_idx)?;
        let jobid = parts.get(jobid_col_idx)?;
        let stepid = parts.get(stepid_col_idx)?;

        // Parse as i64 first to detect Slurm's -1 sentinel for pending PIDs.
        let pid: i64 = pid_str.parse().ok()?;
        if pid < 0 {
            return None;
        }

        let pid: u32 = pid.try_into().ok()?;
        Some((jobid.to_string(), stepid.to_string(), pid))
    }
}

impl HpcScheduler for SlurmScheduler {
    /// Returns the currently active HPC processes.
    ///
    /// Parses the header row of `scontrol listpids` to locate the
    /// required columns by name, then extracts process data from each
    /// subsequent row. Warns and returns an empty list if the expected
    /// columns are missing.
    fn get_processes(&self) -> Result<Vec<HpcProcess>, Box<dyn Error>> {
        let mut lines = self.fetch_scontrol_output()?;

        // Parse the header to discover column positions.
        if lines.is_empty() {
            return Ok(Vec::new());
        }

        let header = lines.remove(0);
        let columns = Self::parse_scontrol_header(&header);

        let (pid_idx, jobid_idx, stepid_idx) = match (
            columns.get(COL_PID),
            columns.get(COL_JOBID),
            columns.get(COL_STEPID),
        ) {
            (Some(&p), Some(&j), Some(&s)) => (p, j, s),
            _ => {
                warn!(
                    "scontrol listpids header missing expected columns \
                     (expected {COL_PID}, {COL_JOBID}, {COL_STEPID}): {header:?}"
                );
                return Ok(Vec::new());
            }
        };

        Ok(lines
            .iter()
            .filter_map(|line| Self::parse_scontrol_line(line, pid_idx, jobid_idx, stepid_idx))
            .map(|(jobid, stepid, pid)| HpcProcess { jobid, stepid, pid })
            .collect())
    }
}
