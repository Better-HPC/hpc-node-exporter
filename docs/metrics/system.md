# System Metrics

System metrics are enabled with the `--system` flag. They cover node-level CPU, memory, and swap
utilization, as well as per-job resource consumption aggregated across all processes belonging to
each job step.

---

## Node Metrics

### CPU

| Metric | Type | Labels | Description |
|---|---|---|---|
| `hpcexp_sys_cpu_usage_percent` | Gauge | `hostname` | Total CPU usage across all cores as a percentage. |
| `hpcexp_sys_cpu_count` | Gauge | `hostname` | Number of logical CPU cores available on this node. |
| `hpcexp_sys_cpu_core_usage_percent` | Gauge | `hostname`, `core` | CPU usage per logical core as a percentage. |
| `hpcexp_sys_load_avg_1m` | Gauge | `hostname` | System load average over the last 1 minute. |
| `hpcexp_sys_load_avg_5m` | Gauge | `hostname` | System load average over the last 5 minutes. |
| `hpcexp_sys_load_avg_15m` | Gauge | `hostname` | System load average over the last 15 minutes. |

### Memory

| Metric | Type | Labels | Description |
|---|---|---|---|
| `hpcexp_sys_memory_total_bytes` | Gauge | `hostname` | Total physical memory available on this node in bytes. |
| `hpcexp_sys_memory_used_bytes` | Gauge | `hostname` | Physical memory currently in use on this node in bytes. |
| `hpcexp_sys_memory_available_bytes` | Gauge | `hostname` | Physical memory currently available on this node in bytes. |
| `hpcexp_sys_swap_total_bytes` | Gauge | `hostname` | Total swap space on this node in bytes. |
| `hpcexp_sys_swap_used_bytes` | Gauge | `hostname` | Swap space currently in use on this node in bytes. |
| `hpcexp_sys_swap_free_bytes` | Gauge | `hostname` | Swap space currently free on this node in bytes. |

---

## Per-Job Metrics

Per-job metrics are aggregated across all processes belonging to a given job step. A job step is
uniquely identified by the combination of `jobid` and `stepid`.

| Metric | Type | Labels | Description |
|---|---|---|---|
| `hpcexp_sys_job_cpu_usage_percent` | Gauge | `hostname`, `jobid`, `stepid` | Total CPU usage for a job step across all its processes, as a percentage. |
| `hpcexp_sys_job_memory_used_bytes` | Gauge | `hostname`, `jobid`, `stepid` | Physical memory used by a job step across all its processes, in bytes. |
| `hpcexp_sys_job_virtual_memory_bytes` | Gauge | `hostname`, `jobid`, `stepid` | Virtual memory used by a job step across all its processes, in bytes. |
| `hpcexp_sys_job_io_read_bytes` | Counter | `hostname`, `jobid`, `stepid` | Bytes read from disk by a job step since it started. |
| `hpcexp_sys_job_io_write_bytes` | Counter | `hostname`, `jobid`, `stepid` | Bytes written to disk by a job step since it started. |
| `hpcexp_sys_job_process_count` | Gauge | `hostname`, `jobid`, `stepid` | Number of running processes belonging to a job step. |

!!! note
`hpcexp_sys_job_io_read_bytes` and `hpcexp_sys_job_io_write_bytes` are cumulative counters
measured from process start, not rates. Use `rate()` in PromQL to derive throughput.

---

## Example Output

```
# HELP hpcexp_sys_cpu_usage_percent Total CPU usage across all cores as a percentage.
# TYPE hpcexp_sys_cpu_usage_percent gauge
hpcexp_sys_cpu_usage_percent{hostname="node01"} 42.1800

# HELP hpcexp_sys_memory_used_bytes Physical memory currently in use on this node in bytes.
# TYPE hpcexp_sys_memory_used_bytes gauge
hpcexp_sys_memory_used_bytes{hostname="node01"} 17179869184.0000

# HELP hpcexp_sys_job_cpu_usage_percent Total CPU usage for an HPC job step across all its processes, as a percentage.
# TYPE hpcexp_sys_job_cpu_usage_percent gauge
hpcexp_sys_job_cpu_usage_percent{hostname="node01",jobid="100042",stepid="0"} 38.4000
```

---

## PromQL Examples

Memory utilization as a percentage:

```promql
hpcexp_sys_memory_used_bytes / hpcexp_sys_memory_total_bytes * 100
```

Jobs consuming more than 32 GiB of physical memory:

```promql
hpcexp_sys_job_memory_used_bytes > 32 * 1024^3
```

Disk write throughput per job step in bytes per second:

```promql
rate(hpcexp_sys_job_io_write_bytes[1m])
```

Nodes where load average exceeds core count:

```promql
hpcexp_sys_load_avg_1m > hpcexp_sys_cpu_count
```
