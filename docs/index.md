# Keystone Exporter

Keystone Exporter is a job-aware Prometheus exporter for the Keystone HPC platform.
It collects node-level and per-job hardware telemetry from HPC compute nodes and exposes the data
via a `/metrics` endpoint in Prometheus text exposition format.

The exporter integrates with the [Slurm](https://slurm.schedmd.com/) workload manager to discover
active jobs and attribute resource usage back to individual job steps.

## Features

- **System profiling** — CPU utilization, memory usage, and network throughput at the node level.
  Per-job CPU, memory, and disk I/O usage attributed by Slurm job and step ID.
- **NVIDIA GPU profiling** — GPU utilization, memory, temperature, and power draw at the device level.
  Per-job GPU memory usage attributed by Slurm job and step ID.
- **Prometheus native** — Metrics are served in standard Prometheus text exposition format
  and are ready for scraping without additional adapters.
- **Job awareness** — Resource usage is broken down by Slurm job and step, enabling per-user
  and per-project accounting alongside traditional node monitoring.

## Quick Start

Enable one or more profilers and start the exporter:

```bash
keystone-exporter --system --nvidia
```

Metrics are available at `http://127.0.0.1:9105/metrics` by default.
See the [Configuration](configuration.md) page for the full list of available options.
