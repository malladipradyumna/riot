# Binary game protocol

`game.proto` is shared by the Rust server and the plain JavaScript browser client.
WebSocket application messages use binary Protobuf envelopes in both directions.
Text messages receive a binary BINARY_REQUIRED error. Malformed protobuf frames
receive CORRUPT_MSG. Inputs are no longer echoed back as application messages.

Rust uses prost; build.rs generates bindings using a vendored protoc compiler.
No system protoc installation is required. The browser loads the local schema
with protobuf.js 8.8.0, vendored in vendor/ with its license. No CDN or frontend
build step is required. Source: https://github.com/protobufjs/protobuf.js and
https://registry.npmjs.org/protobufjs/-/protobufjs-8.8.0.tgz.

Each game builds and encodes one shared snapshot per update, fifteen times per
second; simulation still runs at 30 Hz.
The browser interpolates with a 100-ms delay.
The reference-counted bytes buffer is reused for every recipient. The acknowledgements array includes
player IDs and the latest processed request IDs, so state is identical for all
recipients. Other gameplay events retain their existing application behavior.
A player's respawn_count increments on death so clients teleport to the new
position immediately instead of interpolating across the map.
Player snapshots also include server-owned kills and deaths. Scores start at
zero on join, persist across respawns, and are included for late joiners. The
scoreboard shows current participants, sorted by kills descending, then deaths
ascending and player ID for ties. It updates from snapshots, not kill notices.

Train snapshots contain four separate carriages, their order, and a corner counter on the leading carriage for whistle playback. Player snapshots include remaining ammo and weapon capacity. The server consumes one round per shot and refills ammo and health while aboard any carriage. Respawning restores full health and ammo.

Server logging uses tracing with tracing-appender's bounded non-blocking writer.
The writer drains on a dedicated thread. The default level is info; input logs
are trace and per-movement logs are debug. To inspect them use RUST_LOG=debug
or RUST_LOG=trace. Formatting still occurs at enabled call sites; asynchronous
logging moves output I/O off the gameplay threads. Ctrl+C flushes the writer.

Tokio uses its default multi-thread scheduler and worker count. Override workers
with TOKIO_WORKER_THREADS; RIOT_EVENT_INTERVAL optionally changes how often the
scheduler polls timers and I/O (default 61). Both settings should be benchmarked
on the target machine. The server logs its actual worker count at startup.

RIOT_METRICS_ADDRESS defaults to 127.0.0.1:9000 and exposes /metrics (Prometheus)
and /healthz (liveness). See ../../deploy/README.md for ARM64 deployment and the
provisioned Grafana dashboard.
