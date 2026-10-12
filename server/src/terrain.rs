use crate::{
    collision::{box_time, circle_time},
    types::Position,
};
use std::sync::OnceLock;

pub const TILE_SIZE: f32 = 50.0;
// All games share the same read-only map. Load it once, not once per game.
static MAP: OnceLock<Vec<Vec<u8>>> = OnceLock::new();

// Read and validate the shared ASCII map before accepting players.
pub fn load() -> &'static Vec<Vec<u8>> {
    MAP.get_or_init(|| {
        let assets = std::env::var_os("RIOT_ASSETS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"))
            });
        let text = std::fs::read_to_string(assets.join("maps/arena.txt"))
            .expect("Cannot load assets/maps/arena.txt");
        // Fail at startup rather than discover bad dimensions in a running game.
        let rows: Vec<Vec<u8>> = text.lines().map(|line| line.as_bytes().to_vec()).collect();
        assert_eq!(rows.len(), 36, "Map must have 36 rows");
        assert!(
            rows.iter()
                .all(|row| row.len() == 96 && row.iter().all(|tile| b".fws#tbu".contains(tile))),
            "Map must have 96 columns and valid terrain symbols"
        );
        rows
    })
}

// Return walking units per second for the tile under this position.
pub fn speed(position: Position) -> f32 {
    let tile = tile(position.x, position.y);
    match tile {
        b'f' => 120.0,
        b'w' => 70.0,
        b's' => 150.0,
        b'u' => 160.0,
        b'#' => 0.0,
        _ => 200.0,
    }
}

// Look up a map symbol; outside the map behaves like a wall.
pub fn tile(x: f32, y: f32) -> u8 {
    if x < 0.0 || y < 0.0 {
        return b'#';
    }
    load()
        .get((y / TILE_SIZE) as usize)
        .and_then(|row| row.get((x / TILE_SIZE) as usize))
        .copied()
        .unwrap_or(b'#')
}

// Check whether a circle fits without touching any neighboring wall tile.
pub fn can_stand(position: Position, radius: f32) -> bool {
    let min_x = ((position.x - radius) / TILE_SIZE).floor() as i32;
    let max_x = ((position.x + radius) / TILE_SIZE).floor() as i32;
    let min_y = ((position.y - radius) / TILE_SIZE).floor() as i32;
    let max_y = ((position.y + radius) / TILE_SIZE).floor() as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if tile(x as f32 * TILE_SIZE, y as f32 * TILE_SIZE) != b'#' {
                continue;
            }
            // The closest point on the wall box catches both face and corner overlap.
            let nearest_x = position
                .x
                .clamp(x as f32 * TILE_SIZE, (x + 1) as f32 * TILE_SIZE);
            let nearest_y = position
                .y
                .clamp(y as f32 * TILE_SIZE, (y + 1) as f32 * TILE_SIZE);
            if (position.x - nearest_x).powi(2) + (position.y - nearest_y).powi(2)
                <= radius * radius
            {
                return false;
            }
        }
    }
    true
}

// Sweep a circle against nearby wall tiles. The two face rectangles plus
// corner circles form the exact rounded rectangle, avoiding corner false hits.
pub fn segment_hit_fraction(start: Position, end: Position, radius: f32) -> Option<f32> {
    let mut first: Option<f32> = None;
    let min_x = ((start.x.min(end.x) - radius) / TILE_SIZE).floor() as i32;
    let max_x = ((start.x.max(end.x) + radius) / TILE_SIZE).floor() as i32;
    let min_y = ((start.y.min(end.y) - radius) / TILE_SIZE).floor() as i32;
    let max_y = ((start.y.max(end.y) + radius) / TILE_SIZE).floor() as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let left = x as f32 * TILE_SIZE;
            let top = y as f32 * TILE_SIZE;
            if tile(left, top) != b'#' {
                continue;
            }
            let right = left + TILE_SIZE;
            let bottom = top + TILE_SIZE;
            let contacts = [
                box_time(
                    start,
                    end,
                    Position {
                        x: left - radius,
                        y: top,
                    },
                    Position {
                        x: right + radius,
                        y: bottom,
                    },
                ),
                box_time(
                    start,
                    end,
                    Position {
                        x: left,
                        y: top - radius,
                    },
                    Position {
                        x: right,
                        y: bottom + radius,
                    },
                ),
                circle_time(start, end, Position { x: left, y: top }, radius),
                circle_time(start, end, Position { x: right, y: top }, radius),
                circle_time(start, end, Position { x: left, y: bottom }, radius),
                circle_time(
                    start,
                    end,
                    Position {
                        x: right,
                        y: bottom,
                    },
                    radius,
                ),
            ];
            for t in contacts.into_iter().flatten() {
                first = Some(first.map_or(t, |old| old.min(t)));
            }
        }
    }
    first
}
