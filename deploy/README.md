# OCI Ampere deployment

The game server and Caddy run in Docker. Caddy serves the frontend and proxies
same-origin `/ws` connections to the game server on the internal Docker network.

## Install Docker

On Ubuntu/Debian or Oracle Linux/RHEL, run from the repository root:

```sh
sudo bash deploy/install-host.sh
```

The installer supports ARM64 and AMD64 and installs Docker Engine, Buildx, and
Compose from Docker's package repositories. Existing conflicting container
packages are not automatically removed.

## Deploy the game

Build on the Ampere instance; do not copy an x86 binary from your laptop.

```sh
cd /path/to/riot/deploy
cp .env.example .env  # first setup only; preserve an existing .env
nano .env
sudo docker compose config --quiet
sudo docker compose up -d --build
sudo docker compose ps
sudo docker compose logs --tail=100 server caddy
```

Set `RIOT_SITE` to your domain, with DNS pointing to the instance. Caddy obtains
HTTPS certificates automatically. For IP-only HTTP, use `RIOT_SITE=http://PUBLIC_IP`.
Allow TCP 80/443 in the OCI security rules and host firewall, and keep SSH
accessible. The game port 8000 is internal and must not be published publicly.
The server loads its map from `RIOT_ASSETS_DIR=/app/assets`.

## Update or stop

```sh
cd /path/to/riot
git pull --ff-only origin main
cd deploy
sudo docker compose up -d --build
```

To stop the deployment, run `sudo docker compose down` from `deploy/`.
Games are held in memory; restarting the server disconnects players and discards
active games. Caddy certificates persist in Docker volumes. Container logs
rotate at three 10-MB files per container.

Caddy starts after the server container starts. Check its logs and confirm that
a browser can join a game after deployment. CPU and RAM are shared with any
other applications on the instance.
