#!/usr/bin/env bash
# Docker Engine/Compose for the RIOT application.
# Reference: https://docs.docker.com/engine/install/
set -Eeuo pipefail
umask 022

fail() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
trap 'printf "ERROR: setup failed at line %s. Check the output above.\n" "$LINENO" >&2' ERR
[[ $EUID -eq 0 ]] || fail 'Run this script using sudo bash.'
[[ -d /run/systemd/system ]] || fail 'This installer requires a systemd host.'
[[ -r /etc/os-release ]] || fail '/etc/os-release is missing.'
# Use the host identity to select the matching package repository.
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

# Hold a lock while changing package sources; parallel setup can race apt/dnf.
mkdir -p /run/lock
exec 9>/run/lock/riot-host-setup.lock
flock -n 9 || fail 'Another setup process is running.'
# Downloads live in a temporary directory and are removed on every exit.
work_dir=$(mktemp -d /tmp/riot-host-setup.XXXXXXXX)
trap 'rm -rf -- "$work_dir"' EXIT

printf 'Installing prerequisites on %s (%s)...\n' "$PRETTY_NAME" "$arch"
if [[ $family == apt ]]; then
  export DEBIAN_FRONTEND=noninteractive
  apt-get update
  apt-get install -y --no-install-recommends ca-certificates curl coreutils util-linux gnupg
else
  command -v dnf >/dev/null || fail 'Oracle Linux/RHEL 8 or newer with dnf is required.'
  dnf install -y ca-certificates curl coreutils util-linux gnupg2
fi

# Retry transient failures, but never follow a redirect onto plain HTTP.
download() {
  curl --fail --silent --show-error --location --retry 3 --retry-delay 2 \
    --connect-timeout 10 --max-time 180 --proto '=https' --proto-redir '=https' \
    "$1" --output "$2"
}

printf '\nInstalling Docker Engine and Compose for the application...\n'
# Keep an existing Engine/Compose install; do not silently remove conflicting packages.
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
# Starting the daemon and checking its API catches installation failures immediately.
systemctl enable --now docker.service
docker info >/dev/null
docker compose version

printf '\nDocker status: %s\n' "$(systemctl is-active docker.service)"
cat <<'INFO'

Deploy the game from your RIOT checkout:
  cd /path/to/riot/deploy
  cp .env.example .env       # first setup only; preserve an existing .env
  nano .env                 # set RIOT_SITE to your domain or http://PUBLIC_IP
  sudo docker compose up -d --build
INFO
