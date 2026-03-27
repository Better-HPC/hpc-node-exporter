# Metrics Reference

All metrics are served at the `/metrics` endpoint in Prometheus text exposition format.
Every metric includes a `hostname` label identifying the originating compute node.

## System Metrics

Collected when the `--system` flag is enabled.

### Node-Level

| Metric                         | Labels     | Description                                                       |
|--------------------------------|------------|-------------------------------------------------------------------|
| `node_cpu_usage_percent`       | `hostname` | Total CPU usage summed across all cores. 100% = one fully loaded core. |
| `node_memory_total_bytes`      | `hostname` | Total installed physical memory in bytes.                         |
| `node_memory_used_bytes`       | `hostname` | Physical memory currently in use in bytes.                        |
| `node_memory_available_bytes`  | `hostname` | Physical memory available for new allocations in bytes.           |
| `node_net_rx_bytes`            | `hostname` | Network bytes received since the last collection interval.        |
| `node_net_tx_bytes`            | `hostname` | Network bytes transmitted since the last collection interval.     |

### Job-Level

Job-level system metrics are attributed to individual Slurm job steps.

| Metric                    | Labels                          | Description                                         |
|---------------------------|---------------------------------|-----------------------------------------------------|
| `job_cpu_usage_percent`   | `hostname`, `jobid`, `stepid`   | CPU usage for the job step. 100% = one fully loaded core. |
| `job_memory_used_bytes`   | `hostname`, `jobid`, `stepid`   | Physical memory used by the job step in bytes.      |
| `job_io_read_bytes`       | `hostname`, `jobid`, `stepid`   | Disk bytes read by the job step since the last interval. |
| `job_io_write_bytes`      | `hostname`, `jobid`, `stepid`   | Disk bytes written by the job step since the last interval. |

## NVIDIA GPU Metrics

Collected when the `--nvidia` flag is enabled.

### Node-Level

Node-level GPU metrics are reported per device, identified by the GPU's unique hardware UUID.

| Metric                      | Labels                  | Description                                |
|-----------------------------|-------------------------|--------------------------------------------|
| `gpu_utilization_percent`   | `hostname`, `gpu_uuid`  | GPU core utilization as a percentage.      |
| `gpu_memory_total_bytes`    | `hostname`, `gpu_uuid`  | Total GPU memory in bytes.                 |
| `gpu_memory_used_bytes`     | `hostname`, `gpu_uuid`  | GPU memory currently in use in bytes.      |
| `gpu_memory_free_bytes`     | `hostname`, `gpu_uuid`  | GPU memory available in bytes.             |
| `gpu_temperature_celsius`   | `hostname`, `gpu_uuid`  | GPU temperature in degrees Celsius.        |
| `gpu_power_usage_watts`     | `hostname`, `gpu_uuid`  | GPU power draw in watts.                   |

### Job-Level

Job-level GPU metrics are attributed to individual Slurm job steps on each device.

| Metric                        | Labels                                    | Description                                    |
|-------------------------------|-------------------------------------------|------------------------------------------------|
| `job_gpu_memory_used_bytes`   | `hostname`, `jobid`, `stepid`, `gpu_uuid` | GPU memory used by the job step in bytes.      |

## Labels

| Label      | Description                                                         |
|------------|---------------------------------------------------------------------|
| `hostname` | The hostname of the compute node running the exporter.              |
| `jobid`    | The Slurm job ID.                                                   |
| `stepid`   | The Slurm job step ID.                                              |
| `gpu_uuid` | The unique hardware UUID of the NVIDIA GPU device.                  |
