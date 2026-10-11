# OCI Ampere deployment

The game server and Caddy run in Docker. Node Exporter and VictoriaMetrics run
natively as systemd services on the host; monitoring does not use containers.
Snapshots are restored to 15/sec and browser interpolation to 100 ms.

## Install the host services

On an Ubuntu/Debian or Oracle Linux/RHEL host, run:

```sh
sudo bash deploy/install-host.sh
```

The installer detects ARM64/AMD64, downloads latest stable OSS releases through
the GitHub API, verifies their SHA256 release checksums, creates the unprivileged
victoriametrics account, and sets up restartable, boot-enabled systemd services.
It installs Docker Engine, Buildx and Compose from Docker's package repositories.
Oracle Linux uses Docker's CentOS-compatible EL packages. Existing conflicting
container packages are not automatically removed.

Configuration: /etc/victoriametrics/prometheus.yml. Data:
/var/lib/victoria-metrics-data. Existing scrape configuration is preserved on
reruns; generated service units are backed up before replacement. Retention is
seven days, with VictoriaMetrics caches limited to 256 MiB (not a total RAM cap).
Scraping runs every 15 seconds: host metrics at 127.0.0.1:9100, VictoriaMetrics at
127.0.0.1:8428, and game metrics at 127.0.0.1:9000. The game target is down until
the game is deployed. Logs: journalctl -u node_exporter -u victoriametrics.

## Deploy the game

Copy server/, client/, assets/, deploy/ and .dockerignore to the instance,
excluding server/target, deploy/.env, Git metadata and credentials. Build on
Ampere; do not copy this laptop's x86 binary. Then:

```sh
cd /path/to/riot/deploy
cp .env.example .env  # first setup only
nano .env
sudo docker compose config --quiet
sudo docker compose up -d --build
sudo docker compose ps
sudo docker compose logs --tail=100 server caddy
```

Set RIOT_SITE to your domain, with DNS pointing to the instance; Caddy obtains
HTTPS certificates automatically. For IP-only plain HTTP/WS, use
RIOT_SITE=http://PUBLIC_IP. Allow TCP 80/443 in the OCI NSG/security list and OS
firewall. Keep SSH accessible. Neither metrics nor the game port 8000 should be
public. Caddy serves assets and proxies same-origin /ws; the server loads its map
from RIOT_ASSETS_DIR=/app/assets. Local Python hosting on port 8080 still works.

## View metrics locally

From your laptop:

```sh
ssh -N -o ExitOnForwardFailure=yes -L 8428:127.0.0.1:8428 USER@INSTANCE_IP
```

Visit http://localhost:8428/vmui/ and query up{job="node"}; scrape status is at
http://localhost:8428/targets. No monitoring installation is needed locally.
Wait about 15 seconds for the first samples. Both native services bind only to
loopback. No public firewall rule for ports 8428/9100 is needed.

If you already run Grafana locally, add a Prometheus datasource pointing to
http://localhost:8428 over this tunnel. For a native Grafana service on OCI, use
http://127.0.0.1:8428 and tunnel Grafana's port 3000 as well. The optional
Grafana dashboard JSON is at grafana/dashboards/riot.json; datasource UID is
prometheus. Grafana itself is not installed by this script.

Metrics include games/players/connections, applied inputs, actual snapshot
writes, simulation tick duration and scheduling delays, bytes and protocol
errors, plus server-process CPU/RAM and host metrics. Server-process CPU uses
100% per logical CPU; host CPU averages across CPUs. Sent snapshots are server
writes, not client ACKs or end-to-end latency. Aggregate labels avoid per-game
or per-player cardinality. Empty games remain counted until expiry; disconnected
players remain counted until cleanup. Monitoring uses some host CPU/RAM.

Games are in memory. A server update/restart disconnects players and discards
games; metrics history stays in the native data directory. Docker logs rotate
at three 10-MB files per container. Check allocated OCPUs/RAM before choosing a
worker count; laptop benchmarks do not establish Ampere capacity.

Installer syntax, release assets/checksums, systemd unit syntax, VictoriaMetrics
scrape configuration, and Compose/Caddy configuration were checked locally.
Installation and ARM64 container builds require testing on the actual instance.
Automated tests and performance-testing code have been removed.

References: https://docs.victoriametrics.com/victoriametrics/single-server-victoriametrics/
https://docs.docker.com/engine/install/ubuntu/
https://docs.docker.com/engine/install/rhel/
https://docs.oracle.com/en-us/iaas/Content/Compute/Tasks/enablingmonitoring.htm
