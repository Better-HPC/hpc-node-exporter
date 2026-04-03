# Installation & Deployment

## Prerequisites

| Requirement          | Notes                                   |
|----------------------|-----------------------------------------|
| Rust 1.75+           | Install via [rustup](https://rustup.rs) |
| Slurm                | `scontrol` must be available on `PATH`  |
| NVIDIA driver + NVML | Required only when using `--nvidia`     |

---

## Building from Source

Clone the repository and build a release binary:

```bash
git clone https://github.com/your-org/hpc-node-exporter.git
cd hpc-node-exporter
cargo build --release
```

The compiled binary will be available at `target/release/hpc-node-exporter`.

!!! tip
For production deployments, always build with `--release`. Debug builds include additional
runtime checks that will meaningfully increase CPU overhead at short collection intervals.

---

## Installation

Copy the binary to a system-wide location:

```bash
install -m 755 target/release/hpc-node-exporter /usr/local/bin/hpc-node-exporter
```

---

## Permissions

The exporter requires read access to `/proc` for process-level metrics and the ability to execute
`scontrol listpids`. It does not require root privileges under a standard Slurm configuration.

!!! warning
Running the exporter as root is strongly discouraged. Create a dedicated service account instead:

    ```bash
    useradd --system --no-create-home --shell /usr/sbin/nologin hpc-exporter
    ```

---

## systemd Deployment

Create a unit file at `/etc/systemd/system/hpc-node-exporter.service`:

```ini
[Unit]
Description=HPC Node Exporter
Documentation=https://your-org.github.io/hpc-node-exporter
After=network.target

[Service]
User=hpc-exporter
Group=hpc-exporter
ExecStart=/usr/local/bin/hpc-node-exporter --system --nvidia --host 0.0.0.0
Restart=on-failure
RestartSec=5s
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

Enable and start the service:

```bash
systemctl daemon-reload
systemctl enable --now hpc-node-exporter
```

Confirm it is running:

```bash
systemctl status hpc-node-exporter
curl http://localhost:9105/metrics
```

---

## CLI Reference

| Flag              | Type    | Default     | Description                     |
|-------------------|---------|-------------|---------------------------------|
| `--system`        | bool    | `false`     | Enable CPU and memory metrics   |
| `--nvidia`        | bool    | `false`     | Enable NVIDIA GPU metrics       |
| `--host`          | string  | `127.0.0.1` | Interface to bind to            |
| `--port`          | u16     | `9105`      | Port to listen on               |
| `--interval`      | seconds | `1`         | Metric collection interval      |
| `--sched-timeout` | seconds | `30`        | Timeout for `scontrol` commands |
| `--quiet`         | bool    | `false`     | Suppress console log output     |

!!! warning
The default `--host` value of `127.0.0.1` binds to loopback only. Set `--host 0.0.0.0` to
expose the exporter to Prometheus running on another host.
