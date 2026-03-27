# Deployment

Keystone Exporter is designed to run as a long-lived service on each HPC compute node.
It should be deployed alongside the Slurm workload manager and any hardware it is configured to monitor.

## Prerequisites

The following dependencies must be available on each compute node where the exporter is deployed.

### Required

- **Slurm CLI tools** — The `scontrol` binary must be on the system `PATH`.
  The exporter invokes `scontrol listpids` to discover active jobs.

### Optional

- **NVIDIA driver and NVML** — Required only when using the `--nvidia` profiler.
  The exporter dynamically loads `libnvidia-ml.so` at startup, so the driver must be installed
  but no compile-time GPU dependency is needed.

## Installing the Binary

Download a prebuilt release binary or build from source using Cargo:

```bash
cargo build --release
install -m 755 target/release/keystone-exporter /usr/local/bin/
```

Verify the installation:

```bash
keystone-exporter --version
```

## Running with Systemd

The following unit file provides a starting point for running the exporter as a systemd service.
Adjust the `ExecStart` flags to match the profilers and network configuration required by your environment.

```toml
[Unit]
Description=Keystone Prometheus exporter for HPC node telemetry
After=network.target slurmd.service
Wants=slurmd.service

[Service]
Type=simple
ExecStart=/usr/local/bin/keystone-exporter --system --nvidia --host 0.0.0.0
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

Enable and start the service:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now keystone-exporter
```

Verify the service is running:

```bash
systemctl status keystone-exporter
curl -s http://localhost:9105/metrics | head
```

## Prometheus Configuration

Add a scrape target for each compute node running the exporter.
The following example uses a static configuration for a small cluster:

```yaml
scrape_configs:
  - job_name: keystone-exporter
    scrape_interval: 15s
    static_configs:
      - targets:
          - node01:9105
          - node02:9105
          - node03:9105
```

For larger deployments, consider using Prometheus service discovery (e.g., file-based or DNS-based)
to manage the target list automatically.

!!! note

    The exporter's `--interval` flag controls how often metrics are collected internally.
    The Prometheus `scrape_interval` controls how often Prometheus fetches those metrics.
    Setting the scrape interval shorter than the collection interval will result in duplicate readings.

## Deploying Across a Cluster

On clusters managed by configuration management tools (Ansible, Puppet, Salt, etc.), the exporter
binary and systemd unit file can be distributed to all compute nodes as part of the standard node image.

A typical deployment involves three steps:

1. Install the `keystone-exporter` binary to a shared location (e.g., `/usr/local/bin/`).
2. Deploy the systemd unit file to `/etc/systemd/system/`.
3. Enable and start the service on each node.

The exporter requires no configuration files or persistent state.
All settings are provided via command-line flags in the systemd unit file.

## Firewall Considerations

The exporter listens on a single TCP port (`9105` by default).
If a host firewall is active, ensure the port is open for inbound connections from the Prometheus server.

```bash
# Example using firewalld
sudo firewall-cmd --permanent --add-port=9105/tcp
sudo firewall-cmd --reload
```

## Health Checking

The exporter does not expose a dedicated health endpoint.
Service health can be verified by querying the `/metrics` endpoint directly:

```bash
curl -sf http://localhost:9105/metrics > /dev/null && echo "healthy" || echo "unhealthy"
```

For systemd-level health monitoring, the unit file can be extended with a watchdog or
an `ExecStartPost` check against the metrics endpoint.
