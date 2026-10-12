use std::{
    cmp::Reverse,
    collections::{BinaryHeap, VecDeque},
    io,
    path::PathBuf,
    sync::OnceLock,
    time::Instant,
};

use crate::{
    components::{AimAngle, Ammo, MovementInput, Shooting, WeaponPickup},
    constants::{
        BOT_DECISION_SECONDS, BULLET_LIFETIME, BULLET_RADIUS, BULLET_SPEED_MULTIPLIER,
        TICK_DURATION,
    },
    terrain,
    types::{PlayerId, Position},
};

// Tile IDs occupy the other values; this sentinel means wall or unreachable.
const NO_PATH: u16 = u16::MAX;
const PREVIEW_COUNT: usize = 5;
const WAYPOINT_RADIUS: f32 = 4.0;
const STEERING_DEAD_ZONE: f32 = 2.0;
const STUCK_REPLAN_SECONDS: f32 = 0.5;
const FIGHTING_DISTANCE: f32 = 100.0;
static ROUTES: OnceLock<Routes> = OnceLock::new();

// Room settings are fixed at creation; later joins cannot change the opponents.
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub count: u32,
    pub difficulty: u32,
}

impl Settings {
    // Older clients use three normal bots. Reject oversized rooms before spawning a task.
    pub fn new(
        count: Option<u32>,
        difficulty: Option<u32>,
        creating: bool,
    ) -> Result<Self, &'static str> {
        let settings = Self {
            count: count.unwrap_or(3),
            difficulty: difficulty.unwrap_or(1),
        };
        if creating && (settings.count > 50 || !(1..=3).contains(&settings.difficulty)) {
            return Err("INVALID_BOT_SETTINGS");
        }
        Ok(settings)
    }
}

// Draw one zero-mean Gaussian angular error per fired shot, in radians.
// Box–Muller uses two uniform samples from UUID bytes without fixed version bits.
pub(crate) fn aim_error(difficulty: u32) -> f32 {
    let std_degrees: f64 = match difficulty {
        1 => 30.0,
        2 => 15.0,
        _ => return 0.0,
    };
    let random = uuid::Uuid::new_v4();
    let bytes = random.as_bytes();
    // The half-step keeps u1 strictly between zero and one, so ln() stays finite.
    let uniform = |slice: &[u8]| {
        (u32::from_le_bytes(slice.try_into().unwrap()) as f64 + 0.5) / (u32::MAX as f64 + 1.0)
    };
    let u1 = uniform(&bytes[..4]);
    let u2 = uniform(&bytes[12..]);
    let standard_normal = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
    (standard_normal * std_degrees.to_radians()) as f32
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Player(PlayerId),
    Gun(hecs::Entity),
}

#[derive(Default)]
pub struct Bot {
    // An empty magazine changes the goal from an opponent to a ground gun.
    target: Option<Target>,
    destination_tile: Option<(usize, usize)>,
    waypoints: VecDeque<Position>,
    // Choose targets less often than steering; movement still updates every tick.
    decision_in: f32,
    last_position: Option<Position>,
    stuck_seconds: f32,
}

// Convert a world position to the map column and row used by the route table.
fn tile(position: Position) -> (usize, usize) {
    (
        (position.x / terrain::TILE_SIZE) as usize,
        (position.y / terrain::TILE_SIZE) as usize,
    )
}

// Convert a route tile back to a steering point in world coordinates.
fn center((x, y): (usize, usize)) -> Position {
    Position {
        x: (x as f32 + 0.5) * terrain::TILE_SIZE,
        y: (y as f32 + 0.5) * terrain::TILE_SIZE,
    }
}

// Measure straight-line distance for steering and local aiming.
fn distance(a: Position, b: Position) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}

// Sum terrain crossing times along a precomputed route.
fn travel_time(path: &[(usize, usize)]) -> f32 {
    path.windows(2)
        .map(|step| {
            terrain::TILE_SIZE / 2.0
                * (1.0 / terrain::speed(center(step[0])) + 1.0 / terrain::speed(center(step[1])))
        })
        .sum()
}

// Called by the game task. Bots only set the same components that human input sets.
pub fn tick(world: &mut hecs::World, targets: &[(PlayerId, Position)], settings: Settings) {
    let routes = ROUTES
        .get()
        .expect("Routes must be initialized before creating games");
    let now = tokio::time::Instant::now();
    let guns: Vec<_> = world
        .query::<(hecs::Entity, &Position, &WeaponPickup)>()
        .iter()
        .filter(|(_, _, pickup)| pickup.expires_at > now)
        .map(|(entity, position, _)| (entity, *position))
        .collect();
    for (player_id, position, ammo, bot, input, aim, shooting) in world.query_mut::<(
        &PlayerId,
        &Position,
        &Ammo,
        &mut Bot,
        &mut MovementInput,
        &mut AimAngle,
        &mut Shooting,
    )>() {
        if targets.iter().all(|(id, _)| id == player_id) {
            *bot = Bot::default();
            *input = MovementInput::default();
            shooting.held = false;
            shooting.pending_shot = false;
            continue;
        }
        let refilling = ammo.0 < 1.0;
        bot.decision_in -= TICK_DURATION;
        let target_position = |target| match target {
            Target::Player(id) => targets
                .iter()
                .find(|(other, _)| *other == id)
                .map(|(_, p)| *p),
            Target::Gun(id) => guns.iter().find(|(other, _)| *other == id).map(|(_, p)| *p),
        };
        // Refill completion or an empty magazine should switch goals immediately.
        let goal_mismatch = match bot.target {
            Some(Target::Gun(_)) => !refilling,
            Some(Target::Player(_)) => refilling,
            None => false,
        };
        let target_gone = bot
            .target
            .is_some_and(|target| target_position(target).is_none());
        if bot.decision_in <= 0.0 || goal_mismatch || target_gone {
            let candidates: Vec<_> = if refilling {
                guns.iter().map(|(id, p)| (Target::Gun(*id), *p)).collect()
            } else {
                targets
                    .iter()
                    .filter(|(id, _)| id != player_id)
                    .map(|(id, p)| (Target::Player(*id), *p))
                    .collect()
            };
            // Nearest means reachable travel time, not a straight line through walls.
            let nearest = candidates
                .into_iter()
                .filter_map(|(target, destination)| {
                    let path = routes.path(tile(*position), tile(destination))?;
                    let cost = travel_time(&path)
                        + distance(*position, center(tile(*position))) / terrain::speed(*position)
                        + distance(destination, center(tile(destination)))
                            / terrain::speed(destination);
                    Some((target, cost))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let next_target = nearest.map(|(target, _)| target);
            if next_target != bot.target {
                bot.waypoints.clear();
                bot.destination_tile = None;
            }
            bot.target = next_target;
            bot.decision_in = BOT_DECISION_SECONDS;
        }
        // No reachable goal: stop and retry on the next decision, not every frame.
        let Some(destination) = bot.target.and_then(target_position) else {
            *input = MovementInput::default();
            shooting.held = false;
            shooting.pending_shot = false;
            continue;
        };
        // Wall sliding or train transport can knock us off the plan; retry if stuck.
        if let Some(previous) = bot.last_position {
            if distance(previous, *position) < 0.1
                && (input.up || input.down || input.left || input.right)
            {
                bot.stuck_seconds += TICK_DURATION;
            } else {
                bot.stuck_seconds = 0.0;
            }
        }
        bot.last_position = Some(*position);
        let destination_tile = tile(destination);
        if bot.destination_tile != Some(destination_tile)
            || bot.stuck_seconds > STUCK_REPLAN_SECONDS
            || bot
                .waypoints
                .front()
                .is_some_and(|next| distance(*position, *next) > terrain::TILE_SIZE * 2.0)
        {
            let mut path = routes
                .path(tile(*position), destination_tile)
                .unwrap_or_default();
            // Skip the current tile's center only when the player's whole body
            // can reach the next center safely. This avoids walking backward
            // every time a moving target enters a new tile.
            if path.len() > 1
                && terrain::segment_hit_fraction(
                    *position,
                    center(path[1]),
                    crate::components::PLAYER_RADIUS,
                )
                .is_none()
            {
                path.remove(0);
            }
            bot.waypoints = path.into_iter().map(center).collect();
            bot.destination_tile = Some(destination_tile);
            bot.stuck_seconds = 0.0;
        }
        while bot
            .waypoints
            .front()
            .is_some_and(|next| distance(*position, *next) <= WAYPOINT_RADIUS)
        {
            bot.waypoints.pop_front();
        }
        let waypoint = bot.waypoints.front().copied().unwrap_or(destination);
        let dx = waypoint.x - position.x;
        let dy = waypoint.y - position.y;
        *input = MovementInput {
            up: dy < -STEERING_DEAD_ZONE,
            down: dy > STEERING_DEAD_ZONE,
            left: dx < -STEERING_DEAD_ZONE,
            right: dx > STEERING_DEAD_ZONE,
            sprinting: settings.difficulty == 3,
        };
        let is_player = matches!(bot.target, Some(Target::Player(_)));
        let clear = terrain::segment_hit_fraction(*position, destination, BULLET_RADIUS).is_none();
        if is_player {
            aim.0 = (destination.y - position.y).atan2(destination.x - position.x);
            // Leave some space instead of walking into the opponent while firing.
            if clear && distance(*position, destination) < FIGHTING_DISTANCE {
                *input = MovementInput::default();
            }
        } else if dx != 0.0 || dy != 0.0 {
            aim.0 = dy.atan2(dx);
        }
        // Do not waste shots at a target beyond the bullet lifetime.
        let speed_multiplier = if settings.difficulty == 3 {
            crate::constants::SPRINT_SPEED_MULTIPLIER
        } else {
            1.0
        };
        let reach = terrain::speed(*position)
            * speed_multiplier
            * BULLET_SPEED_MULTIPLIER
            * BULLET_LIFETIME;
        shooting.held =
            is_player && !refilling && clear && distance(*position, destination) < reach;
        shooting.pending_shot = false;
    }
}

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

// Build the shared next-step table once and write five long-route previews.
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
    // Run one Dijkstra search per walkable start and retain only its first steps.
    for start in 0..count {
        if speeds[start] == 0.0 {
            continue;
        }
        distances.fill(u64::MAX);
        distances[start] = 0;
        let row = &mut routes.next_step[start * count..(start + 1) * count];
        row[start] = start as u16;
        queue.push(Reverse((0, start)));
        // Reverse turns Rust's max-heap into a queue that visits the cheapest tile first.
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
