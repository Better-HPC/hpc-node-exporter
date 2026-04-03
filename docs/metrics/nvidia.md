# NVIDIA GPU Metrics

NVIDIA GPU metrics are enabled with the `--nvidia` flag. They require a compatible NVIDIA driver
and the NVIDIA Management Library (NVML) to be present on the host. The exporter will fail to start
with `--nvidia` if no GPU devices are detected or NVML cannot be initialised.

---

## Card-Level Metrics

Card-level metrics are reported once per physical GPU. Each device is identified by its UUID,
which is stable across reboots and driver updates.

| Metric                                  | Type  | Labels                 | Description                                          |
|-----------------------------------------|-------|------------------------|------------------------------------------------------|
| `hpcexp_gpu_utilization_percent`        | Gauge | `hostname`, `gpu_uuid` | GPU core utilization as a percentage.                |
| `hpcexp_gpu_memory_utilization_percent` | Gauge | `hostname`, `gpu_uuid` | GPU memory controller utilization as a percentage.   |
| `hpcexp_gpu_memory_total_bytes`         | Gauge | `hostname`, `gpu_uuid` | Total GPU memory on this device in bytes.            |
| `hpcexp_gpu_memory_used_bytes`          | Gauge | `hostname`, `gpu_uuid` | GPU memory currently in use on this device in bytes. |
| `hpcexp_gpu_memory_free_bytes`          | Gauge | `hostname`, `gpu_uuid` | GPU memory currently free on this device in bytes.   |
| `hpcexp_gpu_temperature_celsius`        | Gauge | `hostname`, `gpu_uuid` | GPU core temperature in degrees Celsius.             |
| `hpcexp_gpu_power_usage_watts`          | Gauge | `hostname`, `gpu_uuid` | GPU power draw in watts.                             |
| `hpcexp_gpu_clock_graphics_mhz`         | Gauge | `hostname`, `gpu_uuid` | Current GPU graphics clock speed in MHz.             |
| `hpcexp_gpu_clock_memory_mhz`           | Gauge | `hostname`, `gpu_uuid` | Current GPU memory clock speed in MHz.               |
| `hpcexp_gpu_fan_speed_percent`          | Gauge | `hostname`, `gpu_uuid` | GPU fan speed as a percentage of maximum.            |

---

## Per-Job Metrics

Per-job GPU metrics are aggregated across all processes belonging to a given job step on a specific
device. A job step on a device is uniquely identified by `jobid`, `stepid`, and `gpu_uuid`.

| Metric                             | Type  | Labels                                    | Description                                                            |
|------------------------------------|-------|-------------------------------------------|------------------------------------------------------------------------|
| `hpcexp_gpu_job_memory_used_bytes` | Gauge | `hostname`, `jobid`, `stepid`, `gpu_uuid` | GPU memory used by a job step on a specific device, in bytes.          |
| `hpcexp_gpu_job_process_count`     | Gauge | `hostname`, `jobid`, `stepid`, `gpu_uuid` | Number of processes belonging to a job step running on a specific GPU. |

!!! note
Per-job GPU metrics are only emitted for job steps that have at least one active compute process
on a GPU. Steps with no GPU activity will not appear in the output.

---

## Example Output

```
# HELP hpcexp_gpu_utilization_percent GPU core utilization as a percentage.
# TYPE hpcexp_gpu_utilization_percent gauge
hpcexp_gpu_utilization_percent{hostname="node01",gpu_uuid="GPU-abc123"} 87.0000

# HELP hpcexp_gpu_temperature_celsius GPU core temperature in degrees Celsius.
# TYPE hpcexp_gpu_temperature_celsius gauge
hpcexp_gpu_temperature_celsius{hostname="node01",gpu_uuid="GPU-abc123"} 74.0000

# HELP hpcexp_gpu_job_memory_used_bytes GPU memory used by an HPC job step on a specific device, in bytes.
# TYPE hpcexp_gpu_job_memory_used_bytes gauge
hpcexp_gpu_job_memory_used_bytes{hostname="node01",jobid="100042",stepid="0",gpu_uuid="GPU-abc123"} 8589934592.0000
```

---

## PromQL Examples

GPU memory utilization as a percentage per device:

```promql
hpcexp_gpu_memory_used_bytes / hpcexp_gpu_memory_total_bytes * 100
```

Jobs using more than 8 GiB of GPU memory on any single device:

```promql
hpcexp_gpu_job_memory_used_bytes > 8 * 1024^3
```

GPUs exceeding 80°C:

```promql
hpcexp_gpu_temperature_celsius > 80
```

Average power draw across all GPUs on a node:

```promql
avg by (hostname) (hpcexp_gpu_power_usage_watts)
```
