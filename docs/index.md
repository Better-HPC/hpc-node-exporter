# hpc-node-exporter

`hpc-node-exporter` is a job-aware Prometheus exporter for HPC systems. Unlike general-purpose node exporters,
it correlates hardware telemetry directly with the jobs and job steps running on each node, enabling per-job
resource accounting alongside traditional node-level metrics.

The exporter is designed to run as a lightweight daemon on each compute node, exposing a `/metrics` endpoint
that Prometheus can scrape on a configurable interval.

!!! note "Scheduler Support"
The exporter currently supports [Slurm](https://slurm.schedmd.com/) as its job scheduler backend.
See the [Architecture](development/architecture.md) page for details on adding support for other schedulers.

---

## Quick Start

### 1. Run the exporter

Start the exporter with the profilers appropriate for your hardware:

```bash
# CPU and memory metrics only
hpc-node-exporter --system

# CPU, memory, and NVIDIA GPU metrics
hpc-node-exporter --system --nvidia
```

By default, the exporter listens on `127.0.0.1:9105`. To expose it on a specific interface:

```bash
hpc-node-exporter --system --nvidia --host 0.0.0.0 --port 9105
```

### 2. Add a Prometheus scrape target

Add the following to your Prometheus configuration:

```yaml
scrape_configs:
  - job_name: hpc-node-exporter
    scrape_interval: 15s
    static_configs:
      - targets:
          - node01:9105
          - node02:9105
```

!!! tip
The exporter's internal collection interval (`--interval`) and the Prometheus scrape interval are
independent. Setting the scrape interval shorter than the collection interval will return the same
snapshot repeatedly without additional overhead.

### 3. Verify

```bash
curl http://localhost:9105/metrics
```

You should see output similar to:

```
# HELP hpcexp_running_jobs Number of HPC jobs currently running on the node.
# TYPE hpcexp_running_jobs gauge
hpcexp_running_jobs{hostname="node01"} 3.0000
```

---

## License

`hpc-node-exporter` is released under the [GNU General Public License v3.0](https://www.gnu.org/licenses/gpl-3.0.html).
