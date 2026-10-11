# Arena map

arena.txt is a 96 × 36 human-readable grid of 50-unit tiles: 4800 × 1800 world units.

| Symbol | Tile | Walking speed |
|---|---|---:|
| . | Floor | 200 |
| f | Forest | 120 |
| w | River/water | 70 |
| s | Sand | 150 |
| # | Wall | Blocked |
| t | Railway | 200 |
| b | Bridge | 200 |
| u | Tunnel passage | 160 |

Walls block players and bullets. Tunnel passages are walkable openings in walls.
Water slows walking; bridges provide faster crossings. The outer rectangle bounds
the map. The inner rectangular railway runs through tile centers (12,6), (84,6),
(84,30), and (12,30). The train follows this route clockwise at 400 world units/s.
Standing fully on its platform automatically carries a player, including around
corners; walking off leaves the train. All train positions are server-authoritative.

Tile speeds are in server/src/terrain.rs and client/src/assets.js. Train geometry
and route are in server/src/train.rs and server/src/constants.rs. The frontend
renders the server's train snapshots; it does not simulate a separate train.

Keep grid dimensions synchronized with server/client map constants. Restart the
server and refresh clients after editing the file. Assets live at repository root;
client/assets is a symlink so the existing local HTTP server serves them.

The arena includes station-side cover, broken courtyards, forest pockets, and multiple river crossings. Keep the four-carriage railway corridor clear when editing walls. Riding any carriage restores 10 HP and 10 rounds per second, capped by player health and weapon capacity.
