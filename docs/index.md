# HPC Node Exporter

A job-aware Prometheus exporter designed for the HPC systems.

The exporter runs on HPC compute nodes and exposes hardware telemetry combined with metadata from the
underlying HPC scheduler. This telemetry is published over HTTP in Prometheus format, enabling operators to monitor
resource consumption at the node and job level.

## Developer Quickstart

The standard `run` command is used to build and launch a development version of the exporter.
Commandline flags are used to enable various hardware profilers.
For example:

```bash
cargo run -- --system
```

The exporter will begin serving metrics at http://127.0.0.1:9105/metrics.
To verify the exporter is running:

```bash
# Health check
curl -s http://127.0.0.1:9105/

# Fetch metrics
curl -s http://127.0.0.1:9105/metrics
```

The exporter requires Slurm's `scontrol` to be available on the host in order to discover active jobs.
On nodes without Slurm, the exporter will still run but job-level metrics will not be reported.

## Architecture

The exporter is structured around four primary subsystems: a scheduler interface, a set of hardware profilers,
a metrics collector, and an HTTP server.
These components are connected through a shared, lock-free snapshot that decouples metric collection from request
serving.

<p align="center">
  <img src="assets/architecture.svg" />
</p>

### Scheduler

The scheduler is responsible for discovering HPC jobs running on the local node and their corresponding process IDs.
This information is later used to aggregate hardware usage on a per-job level.

### Profilers

Profilers are responsible for measuring hardware utilization at the global and job levels.
Each profiler is responsible for a different hardware type and returns updated metric values on each collection pass:

- **Default** — Always enabled. Reports general metadata for the underlying scheduler and exporter status.
- **System** — Opt-in via `--system`. Collects CPU and memory resource usage through the OS process interface.
- **NVIDIA** — Opt-in via `--nvidia`. Collects GPU telemetry through the NVIDIA Management Library (NVML).

Profiler failures are isolated and partial results are always preferred over a complete failure.
If a profiler fails to collect an individual metric, the remaining metrics will still render.

### Collection Loop

Hardware profiling is run as a loop in a dedicated background thread.
On each iteration, the thread queries the scheduler for active jobs, passes the result to every enabled profiler,
renders the collected metrics into a Prometheus-format string, and publishes the result to a shared memory object.
The loop then sleeps for a configurable interval before repeating.

### Metrics Snapshot

The metrics snapshot is shared between the collection thread and the HTTP server.
The collection thread atomically updates the snapshot on every profiling pass, allowing HTTP handlers to load the
latest snapshot without blocking the collector.
This design decouples HTTP response latency from the metrics collection time.
It also isolates the collection process from incoming HTTP requests, protecting the collector from high request volumes
and potential DOS attacks.

### HTTP Server

The HTTP server exposes two routes: a root health check endpoint (`/`) and the metrics endpoint (`/metrics`).
Prometheus metrics are read directly from the shared snapshot, minimizing the overhead incurred by incoming HTTP
requests.
