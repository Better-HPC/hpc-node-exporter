# Configuration

Keystone Exporter is configured entirely through command-line flags.
At least one profiler (`--system` or `--nvidia`) must be enabled or the process will exit with an error.

## Command-Line Flags

| Flag         | Default       | Description                                |
|--------------|---------------|--------------------------------------------|
| `--system`   | Disabled      | Enable the system CPU/memory profiler.     |
| `--nvidia`   | Disabled      | Enable the NVIDIA GPU profiler.            |
| `--host`     | `127.0.0.1`   | Network interface to bind the HTTP server. |
| `--port`     | `9105`        | TCP port to listen on.                     |
| `--interval` | `1` (second)  | Metric collection interval in seconds.     |
| `--quiet`    | Disabled      | Suppress console log output.               |

## Profilers

### System Profiler

Enabled with the `--system` flag.
Collects CPU, memory, and network metrics at the node level, and per-job CPU, memory, and disk I/O
metrics by cross-referencing active processes against the Slurm scheduler.

The system profiler requires a Linux host.
Initialization will fail on unsupported operating systems.

### NVIDIA GPU Profiler

Enabled with the `--nvidia` flag.
Collects GPU utilization, memory, temperature, and power metrics at the device level, and per-job
GPU memory usage by matching running compute processes against the Slurm scheduler.

The NVIDIA profiler requires the NVIDIA driver and `libnvidia-ml.so` to be available on the host.
Initialization will fail if no GPU devices are detected.

## Logging

Logs are always written to syslog.
By default, logs are also written to stdout.
Console output can be suppressed using the `--quiet` flag.

## Scheduler Integration

The exporter discovers active jobs by invoking `scontrol listpids` from the Slurm command-line tools.
The `scontrol` binary must be available on the system `PATH`.
If the scheduler query fails during a collection pass, the error is logged and the exporter continues
to report node-level metrics without per-job attribution.

## Example Usage

Start the exporter with both profilers, binding to all interfaces on port 9200 with a 5-second
collection interval:

```bash
keystone-exporter --system --nvidia --host 0.0.0.0 --port 9200 --interval 5
```

Run with only system metrics and no console output:

```bash
keystone-exporter --system --quiet
```
