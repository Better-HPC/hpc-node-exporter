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

use wait_timeout::ChildExt;

use crate::schedulers::{HpcProcess, HpcScheduler};

/// Column names expected in `scontrol listpids` output.
const COL_PID: &str = "PID";
const COL_JOBID: &str = "JOBID";
const COL_STEPID: &str = "STEPID";

/// Validated column indices from a `scontrol listpids` header.
struct ScontrolColumns {
    pid: usize,
    jobid: usize,
    stepid: usize,
}

/// A [`HpcScheduler`] interface for Slurm.
#[derive(Debug, Default)]
pub struct SlurmScheduler {
    command_timeout: Duration,
}

impl SlurmScheduler {
    pub fn new(command_timeout: Duration) -> Self {
        Self { command_timeout }
    }

    /// Executes `scontrol listpids` and returns the raw stdout lines.
    ///
    /// The returned vector is guaranteed to contain at least one line
    /// (the header row).
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails to spawn, exceeds the
    /// configured timeout, exits with a non-zero status, or produces
    /// empty output.
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

        // Ensure scontrol call exited successfully.
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

        // Validate output is not empty
        let lines: Vec<String> = stdout.trim().lines().map(String::from).collect();
        if lines.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "scontrol listpids returned empty output",
            ));
        }

        Ok(lines)
    }

    /// Parses and validates the header row of `scontrol listpids` output.
    ///
    /// Returns a [`ScontrolColumns`] containing the positional index of
    /// each required column.
    ///
    /// # Errors
    ///
    /// Returns an error naming every required column that is missing
    /// from the header.
    fn parse_scontrol_header(header: &str) -> Result<ScontrolColumns, Box<dyn Error>> {
        // Map column names to their index
        let columns: HashMap<&str, usize> = header
            .split_whitespace()
            .enumerate()
            .map(|(i, name)| (name, i))
            .collect();

        let pid = columns.get(COL_PID).copied();
        let jobid = columns.get(COL_JOBID).copied();
        let stepid = columns.get(COL_STEPID).copied();

        // Check for missing columns
        let required = [(COL_PID, pid), (COL_JOBID, jobid), (COL_STEPID, stepid)];
        let missing: Vec<&str> = required
            .iter()
            .filter(|(_, idx)| idx.is_none())
            .map(|(name, _)| *name)
            .collect();

        if !missing.is_empty() {
            return Err(format!(
                "scontrol listpids header missing column(s): {} (header was: {header:?})",
                missing.join(", ")
            )
            .into());
        }

        Ok(ScontrolColumns {
            pid: pid.unwrap(),
            jobid: jobid.unwrap(),
            stepid: stepid.unwrap(),
        })
    }

    /// Parses a single `scontrol listpids` data row using the given
    /// column indices.
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
    /// subsequent row.
    ///
    /// # Errors
    ///
    /// Returns an error if `scontrol` cannot be executed, its output is
    /// empty, or the header is missing required columns.
    fn get_processes(&self) -> Result<Vec<HpcProcess>, Box<dyn Error>> {
        let mut lines = self.fetch_scontrol_output()?;

        // Parse the header to discover column positions in the scontrol table.
        let header = lines.remove(0);
        let cols = Self::parse_scontrol_header(&header)?;

        Ok(lines
            .iter()
            .filter_map(|line| Self::parse_scontrol_line(line, cols.pid, cols.jobid, cols.stepid))
            .map(|(jobid, stepid, pid)| HpcProcess { jobid, stepid, pid })
            .collect())
    }
}
