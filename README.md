# RIOT

A browser multiplayer shooter with a Rust authoritative server, Protobuf WebSockets, and a shared terrain map. The moving four-carriage train heals riders and refills ammunition.

## Features

- Create or join a game by ID and choose one of three characters.
- Random weapon assignment: pistol, AK, Desert Eagle, or rocket launcher.
- Server-owned movement, bullets, damage, respawning, kills, and deaths.
- Four moving train carriages that restore health and ammunition aboard.
- Terrain-dependent movement, river crossings, tunnels, and cover.
- Smooth canvas rendering, a player-following camera, radar, and scoreboard.
- Native host monitoring with VictoriaMetrics and Node Exporter.

## Requirements

For local development, install a recent stable Rust toolchain with Cargo and Python 3. There is no Node.js build step or frontend package installation. Dependencies and the initial Rust build require internet access.

For deployment, use Docker Engine with the Compose plugin. The host installer also installs these tools on its supported Linux distributions.

## Run locally

From the repository root, start the server:

```bash
cargo run --release --manifest-path server/Cargo.toml --bin server
```

In another terminal, serve the browser client:

```bash
python3 -m http.server 8080 --directory client
```

Open http://localhost:8080. Create a game and share its ID with another player. Move with WASD or arrow keys, aim with the mouse, and fire with F or the left mouse button. Step onto a train carriage to ride, heal, and refill ammunition.

The server listens on port 8000. Its metrics and health endpoints are http://127.0.0.1:9000/metrics and http://127.0.0.1:9000/healthz.

## Deploy on OCI Ampere

See [the deployment guide](deploy/README.md). The game and Caddy run in Docker; Node Exporter and VictoriaMetrics run as native host services. The installer supports Ubuntu/Debian and Oracle Linux/RHEL on ARM64 or AMD64.

Game state is in memory and resets when the game server restarts. Metrics persist on the host with seven-day retention. View VMUI locally through the SSH tunnel described in the deployment guide.

Assets and their license notes are in [assets/README.md](assets/README.md). The shared wire schema is [assets/protocol/game.proto](assets/protocol/game.proto). Keep private SSH keys and local deployment configuration outside version control.

## Local Docker deployment

Stop the native Rust server first to free port 9000, then run:

```bash
sudo env RIOT_SITE=http://localhost docker compose -f deploy/compose.yaml up -d --build
```

Open http://localhost. Caddy serves the client and proxies WebSocket connections to the game server. Check container status and logs:

```bash
sudo env RIOT_SITE=http://localhost docker compose -f deploy/compose.yaml ps
sudo env RIOT_SITE=http://localhost docker compose -f deploy/compose.yaml logs --tail=100 server caddy
```

## Project layout

| Path | Contents |
| --- | --- |
| `server/` | Rust game simulation, WebSocket server, and metrics endpoint |
| `client/` | Browser HTML, CSS, and JavaScript |
| `assets/` | Shared map, protocol schema, sprites, and sounds |
| `deploy/` | Dockerfiles, Caddy, host installer, and optional Grafana dashboard |
| `docs/` | Design notes |

Edit the map using [its terrain guide](assets/maps/README.md). Restart the server and refresh the browser after map or protocol changes. Automated tests and performance-testing code are not included.

## License and attribution

Original project code and original assets are licensed under the [MIT License](LICENSE), copyright 2026 malladipradyumna. Copies or substantial portions must preserve the copyright and permission notice. Visible credit in your README or application is appreciated, but is not an additional license condition.

Third-party assets and libraries retain their own licenses; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). The project MIT license does not replace those licenses.
