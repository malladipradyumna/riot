# RIOT

A browser multiplayer shooter with a Rust authoritative server, Protobuf WebSockets, and a shared terrain map. The moving four-carriage train heals riders and refills ammunition.

## Features

- Create a game with a random three-word ID or join by ID, and choose one of nine characters.
- Start with a pistol; walk over randomly spawned guns to equip them.
- Ground guns expire after 30 seconds, shown by a filling square border.
- Server-owned movement, bullets, damage, respawning, kills, and deaths.
- Server-controlled 5-minute rounds with a 10-second results countdown; scores reset for the next round.
- Four moving train carriages that restore health and ammunition aboard.
- Positional gunshots, corner whistles, rhythmic train wheels, water ambience, and grass footsteps.
- Damage flashes, a health bar that is green aboard the train and red outside, and one-click game-ID copying.
- Terrain-dependent movement, river crossings, tunnels, and cover.
- Smooth canvas rendering, a player-following camera, radar, and scoreboard.

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

Open http://localhost:8080. Choose 0–50 bots and a difficulty when creating a game, then share its ID with another player. Level 1 bots walk and fight humans or bots with 30° aim standard deviation; level 2 bots walk and hunt only humans with 15° deviation; level 3 bots sprint and hunt only humans with exact aim. Each bot shot draws an independent Gaussian aim error. Joining keeps the creator’s settings. Move with WASD or arrow keys, hold Shift to sprint at 1.9× walking speed (maximum 380; train speed 400), aim with the mouse, and fire with F or the left mouse button. Step onto a train carriage to ride, heal, and refill ammunition.

When switching from Docker, stop its containers first:

```bash
sudo env RIOT_SITE=http://localhost docker compose -f deploy/compose.yaml down
```

The server listens for WebSocket connections on port 8000.

At startup, `server/src/bot.rs` runs Dijkstra from every walkable tile and keeps
a next-step table in memory. Costs use terrain walking speeds, with four-way
movement and walls blocked. This is static navigation between tile centers;
train rides, sprinting, and moving players are not part of the route costs.
Five randomly selected long routes are written to `bot-paths/path-1.txt` through
`path-5.txt` in the working directory, using the original map symbols with `p`
marking each path. Set `RIOT_BOT_PATHS_DIR` to change the output directory.
Docker writes these previews to `/tmp/bot-paths` inside the server container.

Each game starts with Hitman 1, Hitman 2, and Hitman 3. Bots choose the nearest
reachable human by terrain travel time, follow the precomputed routes, and fire
when walls do not block their shots. When out of ammo, they seek a ground gun,
then choose the nearest human again. Bots use the same health, respawns, and
scoreboard as humans; they do not keep a game alive after its humans leave.
`BOTS_PER_GAME` in `server/src/constants.rs` controls their number.

## Deploy on OCI Ampere

See [the deployment guide](deploy/README.md). The game server and Caddy run in Docker. The installer supports Ubuntu/Debian and Oracle Linux/RHEL on ARM64 or AMD64.

Game state is in memory and resets when the game server restarts.

Assets and their license notes are in [assets/README.md](assets/README.md). The shared wire schema is [assets/protocol/game.proto](assets/protocol/game.proto). Keep private SSH keys and local deployment configuration outside version control.

## Local Docker deployment

Run:

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
| `server/` | Rust game simulation and WebSocket server |
| `client/` | Browser HTML, CSS, and JavaScript |
| `assets/` | Shared map, protocol schema, sprites, and sounds |
| `deploy/` | Dockerfiles, Caddy, and Docker host installer |
| `docs/` | Design notes |

Edit the map using [its terrain guide](assets/maps/README.md). Restart the server and refresh the browser after map or protocol changes. Automated tests and performance-testing code are not included.

## Reading the code

Start with `server/src/main.rs` for connection handling, then `server/src/game.rs`
for room ownership, the tick loop, input, and rounds. The game task owns its ECS
world; the manager only stores channel handles. A connection remembers its game
handle after joining, so regular input does not lock the manager.

The larger systems are grouped under `server/src/game/`: `combat.rs` handles
shots, continuous collisions, damage, and respawns; `spawning.rs` creates human
and bot players and manages pickups; `snapshot.rs` builds and encodes the shared
frame. `bot.rs` owns the route table and AI input decisions. `terrain.rs` reads
the ASCII map, `train.rs` moves carriages and riders, and `collision.rs` contains
the geometry helpers.

In the browser, `wire.js` translates protobuf messages, `assets.js` loads art and
handles positional sound, and `main.js` runs the lobby, input, snapshot buffer,
and rendering. Incoming frames replace current state; a bounded history smooths
positions between frames. Health, ammo, scores, and round transitions stay
server-authoritative. The schema is `assets/protocol/game.proto`; removed wire
IDs are reserved rather than reused.

## License and attribution

Original project code and original assets are licensed under the [MIT License](LICENSE), copyright 2026 malladipradyumna. Copies or substantial portions must preserve the copyright and permission notice. Visible credit in your README or application is appreciated, but is not an additional license condition.

Third-party assets and libraries retain their own licenses; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). The project MIT license does not replace those licenses.

Terrain affects player walking speed only. Bullets launch along aim at three times terrain/sprint walking speed, plus the shooter’s actual world velocity (walking and train transport). They keep that combined launch velocity across tiles. Bullets pass through train carriages; the train always follows its route at a fixed speed.
