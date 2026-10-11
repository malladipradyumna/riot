#!/usr/bin/env bash
# Native monitoring + Docker Engine/Compose for the RIOT application.
# References: https://docs.victoriametrics.com/victoriametrics/single-server-victoriametrics/
# https://github.com/prometheus/node_exporter https://docs.docker.com/engine/install/
set -Eeuo pipefail
umask 022

fail() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
trap 'printf "ERROR: setup failed at line %s. Check the output above.\n" "$LINENO" >&2' ERR
[[ $EUID -eq 0 ]] || fail 'Run this script using sudo bash.'
[[ -d /run/systemd/system ]] || fail 'This installer requires a systemd host.'
[[ -r /etc/os-release ]] || fail '/etc/os-release is missing.'
source /etc/os-release
case "$ID" in
  ubuntu|debian) family=apt ;;
  ol|rhel|rocky|almalinux|centos) family=rpm ;;
  *) fail "Unsupported distribution: $ID" ;;
esac
case "$(uname -m)" in
  aarch64|arm64) arch=arm64 ;;
  x86_64|amd64) arch=amd64 ;;
  *) fail 'Supported architectures are ARM64 and AMD64.' ;;
esac

mkdir -p /run/lock
exec 9>/run/lock/riot-host-setup.lock
flock -n 9 || fail 'Another setup process is running.'
work_dir=$(mktemp -d /tmp/riot-host-setup.XXXXXXXX)
trap 'rm -rf -- "$work_dir"' EXIT

printf 'Installing prerequisites on %s (%s)...\n' "$PRETTY_NAME" "$arch"
if [[ $family == apt ]]; then
  export DEBIAN_FRONTEND=noninteractive
  apt-get update
  apt-get install -y --no-install-recommends ca-certificates curl jq tar gzip \
    coreutils util-linux passwd gnupg
else
  command -v dnf >/dev/null || fail 'Oracle Linux/RHEL 8 or newer with dnf is required.'
  dnf install -y ca-certificates curl jq tar gzip coreutils util-linux shadow-utils gnupg2
fi

download() {
  curl --fail --silent --show-error --location --retry 3 --retry-delay 2 \
    --connect-timeout 10 --max-time 180 --proto '=https' --proto-redir '=https' \
    "$1" --output "$2"
}

fetch_release() {
  download "https://api.github.com/repos/$1/releases/latest" "$2"
  jq -e '.draft == false and .prerelease == false and
    (.tag_name | test("^v[0-9]+\\.[0-9]+\\.[0-9]+$"))' "$2" >/dev/null \
    || fail "No supported stable release found for $1."
}

asset_url() {
  jq -er --arg name "$2" '.assets[] | select(.name == $name) | .browser_download_url' "$1"
}

verify_archive() {
  local archive=$1 checksums=$2 expected actual
  expected=$(awk -v name="$(basename "$archive")" \
    '$2 == name || $2 == "*" name {print $1}' "$checksums")
  [[ $expected =~ ^[[:xdigit:]]{64}$ ]] || fail "Missing checksum for $archive."
  actual=$(sha256sum "$archive")
  [[ ${actual%% *} == "$expected" ]] || fail "SHA256 mismatch for $archive."
}

printf 'Fetching latest stable monitoring releases...\n'
fetch_release prometheus/node_exporter "$work_dir/node.json"
fetch_release VictoriaMetrics/VictoriaMetrics "$work_dir/victoria.json"
node_tag=$(jq -r .tag_name "$work_dir/node.json")
vm_tag=$(jq -r .tag_name "$work_dir/victoria.json")
node_archive="node_exporter-${node_tag#v}.linux-${arch}.tar.gz"
vm_archive="victoria-metrics-linux-${arch}-${vm_tag}.tar.gz"
download "$(asset_url "$work_dir/node.json" "$node_archive")" "$work_dir/$node_archive"
download "$(asset_url "$work_dir/node.json" sha256sums.txt)" "$work_dir/node-checksums.txt"
download "$(asset_url "$work_dir/victoria.json" "$vm_archive")" "$work_dir/$vm_archive"
download "$(asset_url "$work_dir/victoria.json" "${vm_archive%.tar.gz}_checksums.txt")" \
  "$work_dir/vm-checksums.txt"
verify_archive "$work_dir/$node_archive" "$work_dir/node-checksums.txt"
verify_archive "$work_dir/$vm_archive" "$work_dir/vm-checksums.txt"

# Extract only the required regular files from the verified archives.
mkdir "$work_dir/node" "$work_dir/victoria"
tar --extract --gzip --file="$work_dir/$node_archive" --directory="$work_dir/node" \
  --no-same-owner --no-same-permissions "${node_archive%.tar.gz}/node_exporter"
tar --extract --gzip --file="$work_dir/$vm_archive" --directory="$work_dir/victoria" \
  --no-same-owner --no-same-permissions victoria-metrics-prod
node_binary="$work_dir/node/${node_archive%.tar.gz}/node_exporter"
vm_binary="$work_dir/victoria/victoria-metrics-prod"
[[ -f $node_binary && ! -L $node_binary && -f $vm_binary && ! -L $vm_binary ]] \
  || fail 'Expected regular binary files were not found.'
"$node_binary" --version
"$vm_binary" --version

getent group victoriametrics >/dev/null || groupadd --system victoriametrics
if ! id victoriametrics >/dev/null 2>&1; then
  useradd --system --gid victoriametrics --home-dir /var/lib/victoria-metrics-data \
    --no-create-home --shell "$(command -v nologin)" victoriametrics
fi
[[ $(id -u victoriametrics) -ne 0 && $(id -g victoriametrics) -ne 0 ]] \
  || fail 'The monitoring account must be unprivileged.'
[[ $(id -gn victoriametrics) == victoriametrics ]] \
  || fail 'The existing victoriametrics account has a different primary group.'
case "$(getent passwd victoriametrics | cut -d: -f7)" in
  */nologin|*/false) ;;
  *) fail 'Existing victoriametrics account must have a non-login shell.' ;;
esac
install -d -o root -g victoriametrics -m 0750 /etc/victoriametrics
install -d -o victoriametrics -g victoriametrics -m 0750 /var/lib/victoria-metrics-data

cat >"$work_dir/prometheus.yml" <<'YAML'
global:
  scrape_interval: 15s
  scrape_timeout: 10s
scrape_configs:
  - job_name: node
    static_configs:
      - targets: ['127.0.0.1:9100']
  - job_name: victoriametrics
    static_configs:
      - targets: ['127.0.0.1:8428']
  - job_name: riot
    static_configs:
      - targets: ['127.0.0.1:9000']
YAML
if [[ ! -e /etc/victoriametrics/prometheus.yml ]]; then
  install -o root -g victoriametrics -m 0640 "$work_dir/prometheus.yml" \
    /etc/victoriametrics/prometheus.yml
else
  printf 'Preserving existing /etc/victoriametrics/prometheus.yml.\n'
fi
chown root:victoriametrics /etc/victoriametrics/prometheus.yml
chmod 0640 /etc/victoriametrics/prometheus.yml
"$vm_binary" -promscrape.config=/etc/victoriametrics/prometheus.yml \
  -promscrape.config.dryRun -loggerLevel=ERROR

# Rename into place: safe even while the previous binary is running.
for name in node_exporter victoria-metrics-prod; do
  if [[ $name == node_exporter ]]; then binary=$node_binary; else binary=$vm_binary; fi
  install -o root -g root -m 0755 "$binary" "/usr/local/bin/$name.new"
  mv -f "/usr/local/bin/$name.new" "/usr/local/bin/$name"
done

cat >"$work_dir/node_exporter.service" <<'UNIT'
[Unit]
Description=Prometheus Node Exporter
Wants=network-online.target
After=network-online.target
StartLimitIntervalSec=60
StartLimitBurst=10

[Service]
Type=simple
User=victoriametrics
Group=victoriametrics
ExecStart=/usr/local/bin/node_exporter --web.listen-address=127.0.0.1:9100
Restart=on-failure
RestartSec=5s
TimeoutStopSec=30s
NoNewPrivileges=true
CapabilityBoundingSet=
RestrictSUIDSGID=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
LockPersonality=true
UMask=0027

[Install]
WantedBy=multi-user.target
UNIT

cat >"$work_dir/victoriametrics.service" <<'UNIT'
[Unit]
Description=VictoriaMetrics single-node metrics storage and VMUI
Wants=network-online.target node_exporter.service
After=network-online.target node_exporter.service
StartLimitIntervalSec=60
StartLimitBurst=10

[Service]
Type=simple
User=victoriametrics
Group=victoriametrics
ExecStart=/usr/local/bin/victoria-metrics-prod -storageDataPath=/var/lib/victoria-metrics-data -promscrape.config=/etc/victoriametrics/prometheus.yml -promscrape.configCheckInterval=30s -httpListenAddr=127.0.0.1:8428 -retentionPeriod=7d -memory.allowedBytes=256MiB
Restart=on-failure
RestartSec=5s
TimeoutStopSec=120s
LimitNOFILE=65536
NoNewPrivileges=true
CapabilityBoundingSet=
PrivateTmp=true
PrivateDevices=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/victoria-metrics-data
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictSUIDSGID=true
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
LockPersonality=true
UMask=0027

[Install]
WantedBy=multi-user.target
UNIT

for service in node_exporter victoriametrics; do
  unit="/etc/systemd/system/$service.service"
  [[ ! -e $unit ]] || cp -a "$unit" "$unit.bak.$(date +%Y%m%d%H%M%S)"
  install -o root -g root -m 0644 "$work_dir/$service.service" "$unit"
done
if command -v restorecon >/dev/null; then
  restorecon -F /usr/local/bin/node_exporter /usr/local/bin/victoria-metrics-prod
  restorecon -RF /etc/victoriametrics /var/lib/victoria-metrics-data
fi
systemd-analyze verify /etc/systemd/system/node_exporter.service \
  /etc/systemd/system/victoriametrics.service
systemctl daemon-reload
systemctl enable node_exporter.service victoriametrics.service
systemctl restart node_exporter.service
systemctl restart victoriametrics.service

wait_ready() {
  local service=$1 url=$2
  for ((attempt=0; attempt<30; attempt++)); do
    if systemctl is-active --quiet "$service" && \
      curl --fail --silent --output /dev/null --max-time 2 "$url"; then return; fi
    sleep 1
  done
  systemctl status "$service" --no-pager --full || true
  journalctl -u "$service" -n 30 --no-pager || true
  fail "$service did not become healthy."
}
wait_ready node_exporter.service http://127.0.0.1:9100/metrics
wait_ready victoriametrics.service http://127.0.0.1:8428/health

printf '\nInstalling Docker Engine and Compose for the application...\n'
if ! command -v docker >/dev/null || ! docker compose version >/dev/null 2>&1; then
  if [[ $family == apt ]]; then
    install -d -m 0755 /etc/apt/keyrings
    download "https://download.docker.com/linux/$ID/gpg" "$work_dir/docker.asc"
    install -o root -g root -m 0644 "$work_dir/docker.asc" /etc/apt/keyrings/docker.asc
    [[ -n ${VERSION_CODENAME:-} ]] || fail 'Distribution codename is missing.'
    printf 'deb [arch=%s signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/%s %s stable\n' \
      "$(dpkg --print-architecture)" "$ID" "$VERSION_CODENAME" \
      >/etc/apt/sources.list.d/docker.list
    apt-get update
    apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
  else
    major=${VERSION_ID%%.*}
    [[ $major =~ ^[0-9]+$ && $major -ge 8 ]] || fail 'Docker setup requires EL 8 or newer.'
    docker_repo=centos
    [[ $ID != rhel ]] || docker_repo=rhel
    cat >/etc/yum.repos.d/docker-ce.repo <<REPO
[docker-ce-stable]
name=Docker CE Stable
baseurl=https://download.docker.com/linux/$docker_repo/$major/\$basearch/stable
enabled=1
gpgcheck=1
gpgkey=https://download.docker.com/linux/$docker_repo/gpg
REPO
    dnf install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
  fi
fi
systemctl enable --now docker.service
docker info >/dev/null
docker compose version

printf '\nInstalled Node Exporter %s and VictoriaMetrics %s.\n' "$node_tag" "$vm_tag"
for service in node_exporter victoriametrics docker; do
  printf '%-18s %s\n' "$service" "$(systemctl is-active "$service")"
done
cat <<'INFO'

From your laptop, keep this SSH tunnel open:
  ssh -N -o ExitOnForwardFailure=yes -L 8428:127.0.0.1:8428 USER@INSTANCE_IP

Open: http://localhost:8428/vmui/
Query: up{job="node"}
Scrape status: http://localhost:8428/targets
Wait about 15 seconds for the first samples.
Ports 8428 and 9100 listen only on loopback; no public firewall rule is needed.
The RIOT target stays down until the game server is deployed on localhost:9000.

Deploy the game's existing Dockerfiles from your RIOT checkout:
  cd /path/to/riot/deploy
  cp .env.example .env       # first setup only; preserve an existing .env
  nano .env                 # set RIOT_SITE to your domain or http://PUBLIC_IP
  sudo docker compose up -d --build

Only the game server and Caddy run in Docker; monitoring runs on the host.
INFO
