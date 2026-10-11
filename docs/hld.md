**RIOT**

**Multiplayer Top-Down Shooter**

High-Level Design + Implementation Prompt

**Rust / Tokio / ECS authoritative server • Browser + Desktop client •
No game engine**

| **Purpose.** This document is intended to be pasted into or attached to a coding model as the canonical build specification. Build the simplest correct version first, keep the protocol and game simulation clean, and prioritize a working multiplayer demo over unnecessary abstractions. |
|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|

Target demo: 2–10 players per match, multiple concurrent matches, same
client playable in a browser or packaged as a desktop window.

# 1. Product Goal and Hard Constraints

Build a polished but deliberately small top-down multiplayer arena
shooter. The game should be visually readable, fun in a LAN demo, and
technically interesting because the multiplayer architecture,
authoritative simulation, ECS game state, room routing, snapshots,
interpolation, and gameplay rules are implemented directly rather than
delegated to a game engine or multiplayer framework.

## Hard constraints

- No Unity, Unreal, Godot, Bevy, Phaser, or other game engine.

- Server is Rust. Use Tokio for asynchronous networking and task/channel
  orchestration.

- Use a lightweight Rust ECS library (prefer hecs) for per-match dynamic
  game state; do not use a full game engine just to obtain ECS.

- Browser client and desktop client must share the same
  gameplay/rendering code.

- Browser client must work by opening the server’s LAN IP from another
  device on the same Wi-Fi.

- Desktop build should be a normal native window produced with Tauri,
  embedding the same HTML/JavaScript/Canvas client.

- Use WebSocket only as the browser-compatible transport. Multiplayer
  protocol, messages, routing, room lifecycle, authoritative simulation,
  snapshots and interpolation are ours.

- Support multiple concurrent games/rooms in the same server process.

- A match supports up to 10 active players.

- Prefer a finished, demonstrable system over speculative scalability or
  over-engineering.

## Recommended technology

| **Layer**            | **Choice**                        | **Reason**                                                                 |
|----------------------|-----------------------------------|----------------------------------------------------------------------------|
| Authoritative server | Rust                              | Strong systems story, safe state ownership, good performance.              |
| Async networking     | Tokio                             | TCP accept loop, socket tasks, timers, mpsc channels.                      |
| WebSocket framing    | tokio-tungstenite or equivalent   | Browser compatibility; do not spend the 2-day scope implementing RFC 6455. |
| ECS                  | hecs                              | Small, focused ECS without adopting a game engine.                         |
| Wire format v1       | JSON via serde                    | Debuggable and fast to implement. Binary protocol can be v2.               |
| Browser client       | Plain JavaScript + HTML Canvas 2D | No frontend/game framework needed.                                         |
| Desktop client       | Tauri                             | Packages the exact same web client into a native window.                   |
| Assets               | PNG sprite sheets + audio files   | Simple, replaceable art pipeline.                                          |

| **Definition of “networking from scratch.”** Do not use a multiplayer/netcode framework. It is acceptable to use a WebSocket library for protocol framing/handshake. We implement our own application protocol, connection/session handling, room routing, player input flow, authoritative state, snapshot cadence, client interpolation, disconnect behavior and game lifecycle. |
|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|

# 2. System Architecture

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>Browser / Phone Desktop Window<br />
HTML + JS + Canvas Tauri + same client<br />
│ │<br />
└──────────────────┬─────────────────────┘<br />
│ WebSocket<br />
▼<br />
┌───────────────────┐<br />
│ Rust Server │<br />
│ Tokio runtime │<br />
└─────────┬─────────┘<br />
│<br />
┌────────────────┼─────────────────┐<br />
│ │ │<br />
Connection Layer Game Manager Static Web Host<br />
socket tasks room registry serves client/<br />
│ │<br />
│ HashMap&lt;GameId, GameHandle&gt;<br />
│ │<br />
└───────GameCommand───────────────┐<br />
│<br />
┌────────────────────┼───────────────┐<br />
▼ ▼ ▼<br />
Game 101 Game 102 Game N<br />
ECS World ECS World ECS World<br />
Tick Loop Tick Loop Tick Loop<br />
Round State Round State Round State</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

## Ownership model

- The networking layer owns socket I/O and connection metadata, not
  gameplay state.

- GameManager owns the registry of live games and creates/destroys
  matches.

- Each Game task owns one ECS World exclusively. No other task directly
  mutates that world.

- Network reader tasks translate incoming messages into GameCommand
  values sent over mpsc.

- A Game sends ServerMessage values to per-connection writer queues; it
  never writes sockets directly.

- The client owns presentation only: sprites, animation frame selection,
  particles, sounds, camera, screen shake and HUD.

## Concurrency model

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>WebSocket<br />
│<br />
├── reader task ──&gt; decode ClientMessage ──&gt; GameCommand mpsc
──┐<br />
│ │<br />
└── writer task &lt;── ServerMessage mpsc
&lt;─────────────────────────┤<br />
▼<br />
one Game task<br />
owns ECS World<br />
fixed-rate ticks</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

For this scale, keep each game simulation sequential. Ten players do not
justify parallel ECS systems. Tokio gives concurrency between sockets
and between matches; the simulation itself stays predictable and easy to
debug.

# 3. Multiple Games / Room Lifecycle

## GameManager

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>struct GameManager {<br />
games: HashMap&lt;GameId, GameHandle&gt;,<br />
}<br />
<br />
struct GameHandle {<br />
tx: mpsc::Sender&lt;GameCommand&gt;,<br />
// optional metadata: player_count, created_at, joinable<br />
}</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

## Required room flows

1.  Client connects and receives/creates a ConnectionId.

2.  Client chooses Create Game or Join Game using a short room code.

3.  GameManager creates a new Game task if needed, or routes the
    connection to an existing joinable game.

4.  The Game allocates a PlayerId, spawns a player entity at a safe
    spawn point, and acknowledges the join.

5.  All subsequent Input messages are routed using ConnectionId → GameId
    → PlayerId.

6.  On disconnect, the server removes the player entity and cleans
    connection mappings.

7.  When a game remains empty for ~30 seconds, destroy it and drop its
    ECS World.

## Room rules

- Maximum 10 players per room.

- Room code should be short and typeable (for example 4–6 uppercase
  characters).

- Reject joins to a full room with a clear protocol error.

- A round can continue when people join/leave; late joiners spawn
  normally.

- Server process can host many independent games; each has its own tick
  counter, map state, dynamic fire, pickups, score and round timer.

# 4. Multiplayer Protocol

Start with JSON messages using serde. Every message has a type
discriminator. Keep protocol structs separate from ECS structs so the
simulation is not coupled to serialization.

## Client → Server messages

| **Message** | **Important fields**                          | **Meaning**                                               |
|-------------|-----------------------------------------------|-----------------------------------------------------------|
| hello       | protocol_version                              | Initial compatibility check.                              |
| create_game | username, character                           | Create a room and join it.                                |
| join_game   | room_code, username, character                | Join existing room.                                       |
| input       | seq, move_x, move_y, aim_angle, fire, pickup? | Current player intent. Never send authoritative position. |
| leave_game  | —                                             | Leave match cleanly.                                      |
| ping        | client_time                                   | RTT measurement / diagnostics.                            |

## Server → Client messages

| **Message** | **Important fields**                                 | **Meaning**                                     |
|-------------|------------------------------------------------------|-------------------------------------------------|
| welcome     | connection_id                                        | Connection established.                         |
| joined      | game_id, room_code, player_id, tick                  | Client is now in a match.                       |
| snapshot    | tick, players, projectiles, pickups, dynamic hazards | Authoritative world state.                      |
| event       | kind + payload                                       | Death, explosion, pickup, round start/end, etc. |
| pong        | server_time, echo client_time                        | RTT calculation.                                |
| error       | code, message                                        | Human-readable failure.                         |

## Input semantics

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>{<br />
"type": "input",<br />
"seq": 481,<br />
"move_x": 0.707,<br />
"move_y": -0.707,<br />
"aim_angle": 1.84,<br />
"fire": true<br />
}</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

- Normalize movement input server-side so diagonal movement is not
  faster.

- The server decides actual movement speed, cooldowns, damage,
  collisions and pickup success.

- Keep the latest input per player; the simulation consumes it on ticks.

- Track sequence numbers even in v1. They make later
  prediction/reconciliation easier and are useful for debugging.

## Snapshot cadence

Recommended: simulate at 30 or 60 Hz, but send snapshots at ~15–20 Hz.
The client renders at requestAnimationFrame speed and interpolates
remote entities between snapshots. For the first working build, sending
full snapshots is acceptable; delta compression is explicitly out of
scope.

# 5. Server ECS Design

Use one ECS World per game. Static map data should not be represented as
thousands of ECS entities. ECS is for dynamic actors and hazards;
immutable terrain/collision data belongs in GameMap.

## Core components

| **Component**   | **Example data**                 | **Used by**                        |
|-----------------|----------------------------------|------------------------------------|
| Position        | Vec2                             | Movement, collision, snapshots     |
| Velocity        | Vec2                             | Movement / projectiles             |
| Rotation        | f32 radians                      | Aim direction, projectile spawn    |
| Player          | PlayerId, username, character_id | Identity / scoring / serialization |
| Health          | current, max                     | Damage / HUD                       |
| WeaponState     | weapon type, next_fire_tick      | Weapon logic                       |
| Collider        | radius or AABB                   | Collision                          |
| Projectile      | owner, damage, type              | Bullet/rocket behavior             |
| WeaponPickup    | weapon type                      | Pickup system                      |
| Lifetime        | expires_at_tick                  | Bullets, effects, pickups          |
| SpawnProtection | expires_at_tick                  | 5-second invulnerability           |
| FireHazard      | radius/intensity/expiry          | Dynamic fire damage/spread         |
| Dead/Respawn    | respawn_at_tick                  | Respawn state                      |

## Example entities

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>Player #42<br />
Position + Velocity + Rotation + Player + Health<br />
+ WeaponState + Collider + SpawnProtection<br />
<br />
AK bullet #91<br />
Position + Velocity + Rotation + Projectile + Collider + Lifetime<br />
<br />
Rocket #104<br />
Position + Velocity + Rotation + Projectile(Rocket) + Collider +
Lifetime<br />
<br />
AK pickup #211<br />
Position + WeaponPickup(AK47) + Collider + Lifetime(10 seconds)<br />
<br />
Fire patch #310<br />
Position + FireHazard + Lifetime</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

## System order per tick

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>Drain GameCommand queue<br />
↓<br />
Apply latest player inputs<br />
↓<br />
Player movement<br />
↓<br />
Terrain speed/effects<br />
↓<br />
Weapon firing / projectile spawn<br />
↓<br />
Projectile movement<br />
↓<br />
Collision detection<br />
↓<br />
Damage / explosions / burn<br />
↓<br />
Death + scoring<br />
↓<br />
Respawn<br />
↓<br />
Weapon pickup spawning / expiry<br />
↓<br />
Dynamic fire update<br />
↓<br />
Lifetime cleanup<br />
↓<br />
Round timer / round transition<br />
↓<br />
Build + broadcast snapshot/events</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

| **Borrow-checker rule of thumb.** Do not make one ECS system mutate every unrelated component. Use narrow queries and events/temporary hit lists. For example, collision detection can produce HitEvents, then DamageSystem mutates Health in a later phase. This keeps borrows simple and system boundaries clean. |
|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|

# 6. Gameplay Specification

## Arena and terrain

The map is one large rectangular arena with irregular biome regions
rather than hard quadrants. Keep the map deterministic per game/round
initially; procedural generation is optional.

| **Terrain**           | **Speed multiplier** | **Gameplay**                                 |
|-----------------------|----------------------|----------------------------------------------|
| Normal / dirt         | 1.00×                | Baseline movement.                           |
| Forest                | 0.85×                | Trees/rocks create cover and collision.      |
| Swamp                 | 0.55×                | High-risk slow area.                         |
| Fire / burning ground | 0.90×                | Periodic damage while inside.                |
| Tunnel                | 1.15–1.20×           | Narrow fast routes / ambush lanes.           |
| Rock / wall           | 0×                   | Solid collision; bullets may stop on impact. |

## Dynamic fire

- Server periodically creates temporary FireHazard entities at valid
  random locations.

- Fire may optionally spread to one nearby valid position at a low
  probability; keep the first version simple.

- Clients render animated flame sprites/particles, but damage and hazard
  lifetime are entirely server-authoritative.

- Fire should never spawn directly on a freshly spawned protected
  player.

## Weapons

| **Weapon**      | **Role**            | **Suggested behavior**                                            |
|-----------------|---------------------|-------------------------------------------------------------------|
| Handgun         | Default / reliable  | Semi-auto, medium damage, accurate. Can be default weapon.        |
| Desert Eagle    | High skill burst    | Slow fire rate, high damage, strong kick/recoil presentation.     |
| AK-47           | Automatic           | High rate of fire, modest damage, spread grows while held.        |
| Rocket Launcher | Area damage         | Slow projectile, explosion radius, self-damage optional.          |
| Flamethrower    | Short-range control | Continuous short cone/segments; applies repeated low damage/burn. |

## Random weapon pickups

- At intervals, server chooses a valid unoccupied map location and
  spawns a WeaponPickup.

- A pickup lasts exactly 10 seconds unless collected first.

- Client shows the weapon sprite, name/icon and a subtle pulse; during
  the final 3 seconds, flash/fade faster.

- Pickup collection is server-authoritative based on distance/collision.
  Never trust a client “picked up weapon” claim.

- Keep a cap on simultaneous pickups so the arena stays readable.

## Spawn and respawn

Players spawn randomly but as far away from other living players as
practical. Use candidate scoring rather than an expensive optimizer:
sample N valid points, compute each candidate’s minimum distance to all
living players, penalize fire/hazards/recent deaths, and choose the
highest score.

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>score(candidate) =<br />
min_distance_to_alive_player(candidate)<br />
- fire_penalty<br />
- recent_death_penalty<br />
- wall_or_invalid_penalty</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

- After spawn/respawn, grant 5 seconds of invulnerability (effectively
  infinite health).

- Visually indicate spawn protection with a ring/glow/pulse and an
  on-screen countdown for the local player.

- Recommended anti-abuse rule: spawn protection ends immediately when
  the protected player fires a weapon.

- On death: increment victim deaths, increment killer kills when
  applicable, show a short death animation/effect, then respawn after a
  short delay (e.g. 1–2 seconds).

## Rounds and scoring

- Each round lasts 5 minutes.

- Deaths do not eliminate a player; players respawn and continue until
  the timer ends.

- HUD shows current round timer plus personal kills/deaths. Optional
  mini leaderboard shows top 3.

- At round end, freeze combat, display standings for ~5 seconds, clear
  temporary pickups/fire/projectiles, respawn all players, reset round
  K/D and start the next round.

- Lifetime/account-wide stats are not required. Keep round state in
  memory only.

# 7. Client / Frontend Architecture

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>WebSocket<br />
│<br />
▼<br />
network.js ──&gt; snapshot buffer + events<br />
│<br />
▼<br />
interpolation.js<br />
│<br />
▼<br />
render state<br />
│<br />
┌───────────┼───────────┐<br />
▼ ▼ ▼<br />
renderer.js animation.js ui.js<br />
│<br />
▼<br />
HTML Canvas 2D</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

## Client responsibilities

- Capture WASD/arrow movement, mouse/touch aim, and fire input; send
  intent to server.

- Maintain a short snapshot buffer and interpolate remote positions to
  avoid visible 15–20 Hz stepping.

- Render world, sprites, projectiles, pickups, effects and HUD.

- Play visual-only effects from server events: muzzle flash, explosion
  particles, hit flash, death puff, screen shake, pickup pulse.

- Never decide damage, kills, health, pickup ownership, spawn validity
  or round winners.

## Suggested file layout

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>client/<br />
index.html<br />
style.css<br />
js/<br />
main.js<br />
network.js<br />
input.js<br />
state.js<br />
interpolation.js<br />
renderer.js<br />
camera.js<br />
animation.js<br />
particles.js<br />
ui.js<br />
assets/<br />
characters/<br />
weapons/<br />
terrain/<br />
effects/<br />
audio/</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

# 8. Visual Style, Sprites and Animation

Target a clean, readable 2D style that looks intentional but does not
require a large art pipeline. The recommended compromise is pixel-ish or
clean illustrated terrain plus simple stylized characters. Gameplay
clarity matters more than detailed art.

## Characters

- Provide up to 10 selectable character appearances. Characters are
  cosmetic only in v1; identical hitbox, speed, health and abilities.

- Recommended base footprint: roughly 48×48 or 64×64 source sprites,
  rendered at a consistent world scale.

- Separate the character body and weapon when possible. Rotate the
  weapon toward the aim angle independently; this dramatically reduces
  the number of required directional art frames.

- For a 2-day prototype, 2–4 walk frames + idle is enough. Do not create
  8-direction × many-frame sheets unless art is already available.

- A simple death animation can be a 3–5 frame sprite sequence or a
  scale/fade + particles; it does not need a dedicated complex sheet.

## Sprite sheet convention

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>character_01.png (example 4-frame horizontal sheet)<br />
<br />
┌────────┬────────┬────────┬────────┐<br />
│ idle │ walk-1 │ walk-2 │ walk-3 │<br />
│ 48x48 │ 48x48 │ 48x48 │ 48x48 │<br />
└────────┴────────┴────────┴────────┘<br />
<br />
At runtime:<br />
frame = floor(animation_time / frame_duration) % frame_count<br />
drawImage(sheet, srcX, srcY, frameW, frameH, dstX, dstY, dstW,
dstH)</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

## Player overhead HUD

Every player must show their username (maximum 7 characters) and current
health above the character. Keep this readable at all times and do not
let it rotate with the sprite.

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>prad123<br />
███████░░ 78<br />
↓<br />
[body]────► gun</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

- Name centered above the player, with a small dark shadow/outline for
  legibility.

- Health bar directly beneath the name. Show numeric health (0–100)
  beside or within the bar.

- Spawn-protected players get a pulsing ring/glow plus a small “shield”
  cue; do not rely only on color.

- Optional: local player name/health can be slightly emphasized; enemies
  should not become visually noisy.

## Weapons and projectiles

| **Asset**             | **Rendering guidance**                                                                                                                               |
|-----------------------|------------------------------------------------------------------------------------------------------------------------------------------------------|
| Handgun / Deagle / AK | Separate gun sprite rotated around the player. Muzzle flash is a tiny transient sprite/particle.                                                     |
| Bullet                | Tiny bright sprite/line; may use tracer line for readability.                                                                                        |
| Rocket                | Visible projectile sprite with smoke trail particles. Explosion is a short animated sprite + radial particles + screen shake.                        |
| Flamethrower          | Do not model hundreds of visual flames as server entities. Render a local stream/cone of flame particles based on authoritative weapon state/events. |
| Pickup                | Weapon icon on the ground, soft bob/pulse, 10-second lifetime. Flash during last 3 seconds.                                                          |

## Terrain rendering layers

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>1. Ground / biome base<br />
2. Decals (mud, ash, grass tufts)<br />
3. Static obstacles (trees, rocks, walls)<br />
4. Dynamic hazards / pickups<br />
5. Players and projectiles<br />
6. Particles / muzzle flash / explosions<br />
7. Floating names + health bars<br />
8. Global HUD / timer / scoreboard</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

## Animations / polish that are cheap but valuable

- Walking bob or 2–4 frame walk cycle.

- Gun recoil: briefly offset/rotate weapon sprite after a shot.

- Muzzle flash for 50–100 ms.

- Hit flash on victim sprite.

- Small knockback-looking visual offset without changing authoritative
  physics.

- Rocket smoke trail and explosion particles.

- Flamethrower flame/noise particles.

- Weapon pickup pulse and countdown flash.

- Subtle screen shake for local explosions only.

- Round-end overlay and quick transition.

# 9. Map, Camera and HUD

## Map representation

Use a GameMap object separate from ECS. A simple grid is sufficient for
terrain lookup, while walls/rocks/trees can be stored as static
collision shapes. Dynamic hazards remain ECS entities.

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>struct GameMap {<br />
width: f32,<br />
height: f32,<br />
terrain: TerrainGrid,<br />
static_colliders: Vec&lt;StaticCollider&gt;,<br />
tunnels: Vec&lt;TunnelRegion&gt;,<br />
spawn_regions: Vec&lt;Rect&gt;,<br />
}</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

## Camera

- Canvas should fill the available viewport while world coordinates
  remain independent from pixels.

- Camera follows the local player with mild smoothing.

- Clamp camera to map edges.

- Render only visible static objects if convenient, but do not build an
  elaborate spatial index for v1.

- Support device-pixel-ratio so sprites/text are not blurry on high-DPI
  screens.

## HUD

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>┌──────────────────────────────────────────────────────────────┐<br />
│ K: 07 D: 04 ROUND 2 03:42 │<br />
│ #1 bob1234 11 │<br />
│ #2 prad123 7 │<br />
│ │<br />
│ [GAME WORLD] │<br />
│ │<br />
│ Current: AK-47 HP ███████░ 78 │<br />
└──────────────────────────────────────────────────────────────┘</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

- Always visible: 5-minute round timer, personal kills/deaths, current
  weapon and local HP.

- Optional compact top-3 leaderboard; full standings appear at round
  end.

- Show room code in pre-game/lobby UI, not permanently over the action.

- Lobby screen needs username input (max 7), character picker (up to
  10), Create Game and Join Game.

# 10. Server Module Structure

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>server/<br />
Cargo.toml<br />
src/<br />
main.rs<br />
config.rs<br />
protocol/<br />
mod.rs<br />
client_message.rs<br />
server_message.rs<br />
network/<br />
listener.rs<br />
connection.rs<br />
registry.rs<br />
games/<br />
manager.rs<br />
handle.rs<br />
game.rs<br />
round.rs<br />
map.rs<br />
spawn.rs<br />
ecs/<br />
components.rs<br />
systems/<br />
input.rs<br />
movement.rs<br />
terrain.rs<br />
weapons.rs<br />
projectiles.rs<br />
collision.rs<br />
damage.rs<br />
death.rs<br />
respawn.rs<br />
pickups.rs<br />
fire.rs<br />
lifetime.rs<br />
snapshot.rs</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

## Key data structures

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>type ConnectionId = u64;<br />
type PlayerId = u64;<br />
type GameId = u64;<br />
<br />
struct ConnectionMeta {<br />
player_id: Option&lt;PlayerId&gt;,<br />
game_id: Option&lt;GameId&gt;,<br />
outgoing: mpsc::Sender&lt;ServerMessage&gt;,<br />
}<br />
<br />
enum GameCommand {<br />
Join { connection_id, username, character, outgoing },<br />
Input { player_id, input },<br />
Disconnect { player_id },<br />
}<br />
<br />
struct Game {<br />
id: GameId,<br />
world: hecs::World,<br />
map: GameMap,<br />
round: RoundState,<br />
tick: u64,<br />
commands: mpsc::Receiver&lt;GameCommand&gt;,<br />
}</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

## Game loop

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>loop {<br />
ticker.tick().await;<br />
<br />
drain_commands();<br />
run_input_system();<br />
run_movement_system();<br />
run_terrain_system();<br />
run_weapon_system();<br />
run_projectile_system();<br />
detect_collisions_into_events();<br />
apply_damage_events();<br />
process_deaths_and_respawns();<br />
update_pickups_and_fire();<br />
cleanup_expired_entities();<br />
update_round();<br />
<br />
if should_send_snapshot(tick) {<br />
broadcast_snapshot();<br />
}<br />
<br />
tick += 1;<br />
}</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

| **Important implementation constraint.** Do not hold Tokio mutex guards across .await inside the simulation. Prefer ownership + channels. A Game task should own its mutable simulation state and receive commands over mpsc, which avoids shared mutable state almost entirely. |
|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|

# 11. Implementation Order / 2-Day Target

The complete design is larger than a strict 2-day implementation. The
first milestone should prove the architecture and produce a playable LAN
demo. Build in this order and do not skip ahead into art polish before
networking works.

## Day 1 — playable authoritative multiplayer

8.  Create Rust workspace/server, Tokio listener, WebSocket connection
    reader/writer tasks.

9.  Define JSON protocol and connection registry.

10. Implement GameManager, room creation/join by code, one task + ECS
    World per game.

11. Implement player entity, input, movement, boundaries, health and
    simple collision.

12. Serve the browser client from the server (or a tiny local static
    server if simpler).

13. Canvas renders local/remote players; show username + health bar.

14. Send snapshots and interpolate remote player positions.

15. Implement one gun (handgun), projectile, damage, death and respawn.

16. Verify two browser tabs/devices can join the same room over LAN and
    a different room stays isolated.

## Day 2 — make it look and feel like the intended game

17. Add character selection and sprite rendering/animation.

18. Package same client with Tauri and verify browser ↔ desktop
    cross-play.

19. Add terrain speed modifiers (forest/swamp/tunnel) and static
    obstacles.

20. Add random weapon pickup spawn + 10-second expiry.

21. Add remaining weapons in descending simplicity: Deagle, AK, rocket,
    flamethrower.

22. Add spawn-point scoring + 5-second protection.

23. Add dynamic fire if time remains.

24. Add 5-minute round timer, K/D, round-end standings/reset.

25. Add visual polish: muzzle flash, hit flash, rocket explosion, pickup
    pulse, particles, audio.

## Strictly optional v2

- Binary serialization / compact snapshots.

- Client-side prediction + server reconciliation for the local player.

- Interest management / spatial partitioning.

- Artificial latency/jitter/loss simulator for demo/testing.

- Replay/spectator mode.

- Persistent profiles or matchmaking service.

- Procedural map generation.

- UDP native transport. Browser path should remain
  WebSocket/WebTransport-based.

# 12. Acceptance Criteria

## Core architecture

- One server process can host at least two independent games
  simultaneously.

- Each match owns a distinct ECS World; players/events cannot leak
  across rooms.

- At least two clients on the same Wi-Fi can connect by LAN IP and play
  together.

- Browser and Tauri desktop build use the same client code and can join
  the same match.

- Clients send inputs, not trusted positions/health/damage.

- Disconnecting a client removes its player cleanly without crashing the
  game task.

## Gameplay

- Up to 10 cosmetic characters can be selected; same gameplay stats.

- Username is capped at 7 characters and displayed above the sprite.

- Health bar + numeric health render above players and update from
  authoritative snapshots.

- Biome regions visibly differ and movement speed changes according to
  terrain.

- Handgun, Deagle, AK-47, rocket launcher and flamethrower are
  represented with distinct behavior.

- Weapon pickups spawn randomly, disappear after 10 seconds, and are
  collected server-side.

- Respawn location is selected to maximize distance from existing
  players among sampled candidates.

- Fresh spawns are invulnerable for 5 seconds; protection is visually
  obvious.

- Kills and deaths are tracked; death causes respawn rather than
  elimination.

- Rounds last 5 minutes and end with a score overlay before the next
  round resets transient state.

## Presentation

- Movement of remote players is visually smooth via interpolation.

- Sprites are rendered via Canvas without a game engine.

- Weapons rotate/aim correctly; core actions have simple
  animation/effects.

- Rocket explosion and flamethrower are visually distinctive.

- UI works at common laptop resolutions and remains readable on a phone
  browser in landscape orientation.

# 13. Canonical Prompt for the Coding Model

| **Instruction.** Treat everything above as the canonical product/HLD specification. When there is ambiguity, choose the smallest implementation that preserves the architecture. Do not introduce a game engine or large framework unless explicitly requested. |
|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|

<table>
<colgroup>
<col style="width: 100%" />
</colgroup>
<thead>
<tr class="header">
<th>You are implementing RIOT, a small real-time multiplayer top-down
shooter. Build it as a working repository, not as pseudocode.<br />
<br />
Server: Rust + Tokio. Use a lightweight ECS such as hecs. Each game room
owns a separate ECS World and runs an authoritative fixed-tick
simulation. A GameManager supports multiple concurrent games and routes
players using GameId / PlayerId / ConnectionId. Use Tokio mpsc channels
so network tasks never directly mutate a game world. The
browser-compatible transport is WebSocket; using a WebSocket framing
library is allowed, but do not use any multiplayer/netcode framework.
Define our own JSON application protocol, input sequencing, room
lifecycle, snapshots, events and disconnect handling.<br />
<br />
Client: plain HTML/CSS/JavaScript + Canvas 2D. No game engine and no
React is required. The same client must run directly in a browser and be
packageable in a Tauri desktop window. A browser on another device on
the same Wi-Fi must be able to connect using the server laptop's LAN IP.
The client sends player intent, receives authoritative snapshots,
buffers snapshots and interpolates remote movement. The client owns
rendering, sprites, animation, particles, sound, camera, HUD and screen
effects only.<br />
<br />
Gameplay: up to 10 players per room. Each player chooses one of up to 10
cosmetic characters, enters a username of max 7 chars, and spawns at a
valid point chosen to be far from other living players. Spawn/respawn
grants 5 seconds of invulnerability; preferably cancel it if that player
fires. Show username, health bar and numeric health above every player.
On death, increment K/D, briefly show a death effect, and respawn after
a short delay.<br />
<br />
Arena: one large rectangular map with irregular forest, swamp,
normal/dirt, fire/burnt and tunnel regions. Terrain changes speed
(normal 1.0, forest about 0.85, swamp about 0.55, tunnel about
1.15–1.20, fire about 0.9 plus damage). Include static obstacles such as
trees, rocks and walls. Keep static map data outside ECS; dynamic fire
can be ECS entities.<br />
<br />
Weapons: handgun, Desert Eagle, AK-47, rocket launcher and flamethrower.
They must feel different. Random weapon pickups spawn at valid
locations, stay for 10 seconds, then vanish if not collected. Pickup
ownership, cooldowns, hits, damage, health, explosions and deaths are
server-authoritative. Rockets are visible slow projectiles with AoE
explosion. Flamethrower is short-range continuous damage; its many
visual flame particles must remain client-side rather than becoming
hundreds of server entities.<br />
<br />
Rounds: 5 minutes. HUD shows timer, personal kills/deaths, current
weapon and HP. Players keep respawning during the round. At round end
freeze combat, show standings for around 5 seconds, clear transient
projectiles/pickups/fire, reset round stats, respawn everyone, then
start the next round.<br />
<br />
Rendering/art: use PNG sprites/sprite sheets. Prefer a small 2–4 frame
walking cycle per character and separate weapon sprites rotated toward
the aim angle. Add inexpensive polish: muzzle flash, recoil visual, hit
flash, rocket smoke/explosion, flame particles, pickup pulse, screen
shake and a round-end overlay. Use layered rendering: ground → decals →
static obstacles → hazards/pickups → players/projectiles → particles →
overhead labels/health → HUD.<br />
<br />
Implementation priority: first make room creation/join, authoritative
movement, snapshots, interpolation, one gun, health/death/respawn and
LAN multiplayer work. Then add
sprites/Tauri/terrain/weapons/pickups/spawn protection/rounds/polish.
Keep the code modular according to the HLD. Avoid premature binary
protocols, persistence, Kubernetes, databases, parallel ECS scheduling
or elaborate optimization.<br />
<br />
When producing code, prefer complete compiling files and coherent
incremental milestones. Explain any deviation from this specification
before making it.</th>
</tr>
</thead>
<tbody>
</tbody>
</table>

# 14. Final Design Principle

**The impressive part is not that the game has many features.** The
impressive part is that a browser and a native desktop client can join
the same authoritative Rust server, multiple matches run independently,
game state is modeled cleanly with ECS, and the networking/game
simulation remain understandable enough to explain in an interview.
