# Default Metrics

Default metrics are always enabled and require no additional flags. They report exporter status
and scheduler-level job counts derived from the active process list.

---

## Metrics

| Metric | Type | Labels | Description |
|---|---|---|---|
| `hpcexp_running_jobs` | Gauge | `hostname` | Number of HPC jobs currently running on the node. Jobs are counted by unique job ID; multiple steps within the same job are counted once. |
| `hpcexp_scrape_time` | Gauge | `hostname` | Unix timestamp of the last completed metrics collection pass. |

---

## Example Output

```
# HELP hpcexp_running_jobs Number of HPC jobs currently running on the node.
# TYPE hpcexp_running_jobs gauge
hpcexp_running_jobs{hostname="node01"} 3.0000

# HELP hpcexp_scrape_time Unix timestamp of the last metrics collection pass.
# TYPE hpcexp_scrape_time gauge
hpcexp_scrape_time{hostname="node01"} 1743600000.0000
```

---

## PromQL Examples

Nodes with no running jobs:

```promql
hpcexp_running_jobs == 0
```

Detect a stale exporter — no successful collection in the last 60 seconds:

```promql
time() - hpcexp_scrape_time > 60
```
