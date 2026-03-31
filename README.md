# Keystone Exporter

A job-aware Prometheus exporter designed for the Keystone HPC platform.

Keystone Exporter runs on HPC compute nodes and exposes hardware telemetry enriched with job metadata from underlying
HPC scheduler. This telemetry is published over HTTP in Prometheus text format, enabling operators to monitor resource
consumption at both the node and job level.

## Architecture

Keystone Exporter is structured around three subsystems: a scheduler interface, a set of hardware profilers, and an HTTP
server. These components are connected through a shared, lock-free snapshot that decouples metric collection from
request serving.

![](assets/architecture.svg)

### Scheduler

The scheduler is responsible for discovering which HPC jobs are running on the local node.
Support is currently limited to the Slurm scheduler via CLI calls to the `scontrol` utility.

### Profilers

Profilers are the metric collection units of the exporter.
Each profiler implements a common trait and returns a vector of labeled metric values on each collection pass.

- **Default** — Always enabled. Reports the number of running jobs and the current scrape timestamp.
- **System** — Opt-in via `--system`. Collects CPU, and memory resource usage through the OS process interface.
- **NVIDIA** — Opt-in via `--nvidia`. Collects GPU telemetry through the NVIDIA Management Library (NVML).

Profiler failures are isolated and partial results are always preferred over a complete failure.
If a single profiler encounters an error, the remaining profilers still report their
metrics. Similarly, if a profiler fails to resolve an individual metric, the remaining metrics will still render.

### Collection Loop

Hardware profiling is run as a loop in a dedicated background thread.
On each iteration, the thread queries the scheduler for active jobs, passes the result to every enabled profiler,
renders the collected metrics into a Prometheus-format string, and publishes the result to a shared memory object.
The loop then sleeps for a configurable interval (default: 1 second) before repeating.

### Metrics Snapshot

The snapshot is shared between the collection thread and the HTTP server.
The collection thread atomically swaps in a new snapshot on every pass, and HTTP handlers load the current snapshot
without locking or blocking the collector. 
This design ensures that scrape latency is independent of collection cost.
It also isolates the collection process, providing protection against a sudden burst of incoming HTTP requests. 

### HTTP Server

The HTTP layer is an async Axum server running on Tokio.
It exposes two routes: a root health check endpoint (`/`) and the metrics endpoint (`/metrics`).
Both handlers read directly from the shared snapshot, minimizing the overhead incurred by incoming HTTP requests.
