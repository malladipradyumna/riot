use std::{
    cmp::Reverse, collections::BinaryHeap, io, path::PathBuf, sync::OnceLock, time::Instant,
};

use crate::{terrain, types::Position};

const NO_PATH: u16 = u16::MAX;
const PREVIEW_COUNT: usize = 5;
static ROUTES: OnceLock<Routes> = OnceLock::new();

pub struct Routes {
    width: usize,
    height: usize,
    // One entry per (start, destination), not a separate Vec for every full path.
    next_step: Vec<u16>,
}

impl Routes {
    // Coordinates are (column, row). The returned path includes both endpoints.
    pub fn path(
        &self,
        start: (usize, usize),
        destination: (usize, usize),
    ) -> Option<Vec<(usize, usize)>> {
        if start.0 >= self.width
            || start.1 >= self.height
            || destination.0 >= self.width
            || destination.1 >= self.height
        {
            return None;
        }
        let count = self.width * self.height;
        let mut current = start.1 * self.width + start.0;
        let destination = destination.1 * self.width + destination.0;
        let mut path = vec![start];
        // The bound also protects against a broken table accidentally looping forever.
        for _ in 0..count {
            let next = self.next_step[current * count + destination];
            if next == NO_PATH {
                return None;
            }
            if current == destination {
                return Some(path);
            }
            current = usize::from(next);
            path.push((current % self.width, current / self.width));
        }
        None
    }
}

pub fn initialize() -> io::Result<&'static Routes> {
    if let Some(routes) = ROUTES.get() {
        return Ok(routes);
    }
    let started = Instant::now();
    let map = terrain::load();
    let height = map.len();
    let width = map[0].len();
    let count = width * height;
    assert!(count < usize::from(NO_PATH), "Map too large for route IDs");
    let speeds: Vec<_> = (0..count)
        .map(|tile| {
            terrain::speed(Position {
                x: (tile % width) as f32 * terrain::TILE_SIZE + terrain::TILE_SIZE / 2.0,
                y: (tile / width) as f32 * terrain::TILE_SIZE + terrain::TILE_SIZE / 2.0,
            })
        })
        .collect();
    let mut neighbors = vec![Vec::new(); count];
    for tile in 0..count {
        if speeds[tile] == 0.0 {
            continue;
        }
        let x = tile % width;
        let y = tile / width;
        for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if nx < 0 || ny < 0 || nx >= width as isize || ny >= height as isize {
                continue;
            }
            let next = ny as usize * width + nx as usize;
            if speeds[next] == 0.0 {
                continue;
            }
            // Center to center: half the step is in each tile. Costs are nanoseconds
            // so the priority queue can compare integers without float ordering tricks.
            let seconds = f64::from(terrain::TILE_SIZE) / 2.0
                * (1.0 / f64::from(speeds[tile]) + 1.0 / f64::from(speeds[next]));
            neighbors[tile].push((next, (seconds * 1_000_000_000.0).round() as u64));
        }
    }
    let mut routes = Routes {
        width,
        height,
        next_step: vec![NO_PATH; count * count],
    };
    let mut longest = Vec::new();
    let mut distances = vec![u64::MAX; count];
    let mut queue = BinaryHeap::new();
    for start in 0..count {
        if speeds[start] == 0.0 {
            continue;
        }
        distances.fill(u64::MAX);
        distances[start] = 0;
        let row = &mut routes.next_step[start * count..(start + 1) * count];
        row[start] = start as u16;
        queue.push(Reverse((0, start)));
        while let Some(Reverse((cost, current))) = queue.pop() {
            // A better route may have been found since this entry was queued.
            if cost != distances[current] {
                continue;
            }
            for &(next, step_cost) in &neighbors[current] {
                let candidate = cost + step_cost;
                if candidate < distances[next] {
                    distances[next] = candidate;
                    row[next] = if current == start {
                        next as u16
                    } else {
                        row[current]
                    };
                    queue.push(Reverse((candidate, next)));
                }
            }
        }
        if let Some((destination, &cost)) = distances
            .iter()
            .enumerate()
            .filter(|&(tile, &cost)| tile != start && cost != u64::MAX)
            .max_by_key(|&(_, cost)| cost)
        {
            longest.push((cost, start, destination));
        }
    }
    tracing::info!(
        seconds = started.elapsed().as_secs_f64(),
        walkable_tiles = speeds.iter().filter(|&&speed| speed > 0.0).count(),
        table_bytes = routes.next_step.len() * size_of::<u16>(),
        "Precomputed terrain routes"
    );

    // Pick randomly from the longest routes, rather than five arbitrary short trips.
    longest.sort_unstable_by(|a, b| b.cmp(a));
    longest.truncate(64);
    let directory = std::env::var_os("RIOT_BOT_PATHS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("bot-paths"));
    std::fs::create_dir_all(&directory)?;
    for number in 1..=PREVIEW_COUNT.min(longest.len()) {
        let index = (uuid::Uuid::new_v4().as_u128() % longest.len() as u128) as usize;
        let (cost, start, destination) = longest.swap_remove(index);
        let path = routes
            .path(
                (start % width, start / width),
                (destination % width, destination / width),
            )
            .expect("Computed route must be reachable");
        let mut preview = map.clone();
        for &(x, y) in &path {
            preview[y][x] = b'p';
        }
        let mut text = Vec::with_capacity(count + height);
        for row in preview {
            text.extend_from_slice(&row);
            text.push(b'\n');
        }
        let file = directory.join(format!("path-{number}.txt"));
        std::fs::write(&file, text)?;
        tracing::info!(file = %file.display(), ?start, ?destination,
            tiles = path.len(), travel_seconds = cost as f64 / 1_000_000_000.0,
            "Saved long route preview");
    }
    let _ = ROUTES.set(routes);
    Ok(ROUTES.get().unwrap())
}
