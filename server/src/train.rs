use crate::{
    components::{ObjectId, PLAYER_RADIUS},
    constants::{TICK_DURATION, TRAIN_HEIGHT, TRAIN_SPEED, TRAIN_WIDTH},
    types::{EntityId, PlayerId, Position},
};

pub const LEFT: f32 = 625.0;
pub const RIGHT: f32 = 4225.0;
pub const TOP: f32 = 325.0;
pub const BOTTOM: f32 = 1525.0;

pub struct Train {
    pub distance: f32,
    pub angle: f32,
    pub carriage_index: u32,
    pub corner_count: u32,
}

// Distance wraps around one clockwise lap; carriage spacing uses the same route.
pub fn route(distance: f32) -> (Position, f32) {
    let width = RIGHT - LEFT;
    let height = BOTTOM - TOP;
    let distance = distance.rem_euclid(2.0 * (width + height));
    if distance < width {
        (
            Position {
                x: LEFT + distance,
                y: TOP,
            },
            0.0,
        )
    } else if distance < width + height {
        (
            Position {
                x: RIGHT,
                y: TOP + distance - width,
            },
            std::f32::consts::FRAC_PI_2,
        )
    } else if distance < 2.0 * width + height {
        (
            Position {
                x: RIGHT - (distance - width - height),
                y: BOTTOM,
            },
            std::f32::consts::PI,
        )
    } else {
        (
            Position {
                x: LEFT,
                y: BOTTOM - (distance - 2.0 * width - height),
            },
            -std::f32::consts::FRAC_PI_2,
        )
    }
}
pub fn spawn(world: &mut hecs::World, id: EntityId) {
    for index in 0..crate::constants::TRAIN_CARRIAGES {
        let distance = -(index as f32) * crate::constants::TRAIN_CARRIAGE_SPACING;
        let (position, angle) = route(distance);
        world.spawn((
            ObjectId(id + index),
            position,
            Train {
                distance,
                angle,
                carriage_index: index as u32,
                corner_count: 0,
            },
        ));
    }
}

fn local(point: Position, center: Position, angle: f32) -> (f32, f32) {
    let dx = point.x - center.x;
    let dy = point.y - center.y;
    (
        dx * angle.cos() + dy * angle.sin(),
        -dx * angle.sin() + dy * angle.cos(),
    )
}
pub fn aboard(point: Position, center: Position, angle: f32) -> bool {
    let (x, y) = local(point, center, angle);
    x.abs() <= TRAIN_WIDTH / 2.0 - PLAYER_RADIUS && y.abs() <= TRAIN_HEIGHT / 2.0 - PLAYER_RADIUS
}
pub fn platforms(world: &hecs::World) -> Vec<(Position, f32)> {
    world
        .query::<(&Position, &Train)>()
        .iter()
        .map(|(position, train)| (*position, train.angle))
        .collect()
}
pub fn tick(world: &mut hecs::World) -> Option<Position> {
    let mut whistle = None;
    let mut movement = Vec::new();
    for (position, train) in world.query_mut::<(&mut Position, &mut Train)>() {
        let old = *position;
        let angle = train.angle;
        train.distance = (train.distance + TRAIN_SPEED * TICK_DURATION)
            .rem_euclid(2.0 * ((RIGHT - LEFT) + (BOTTOM - TOP)));
        (*position, train.angle) = route(train.distance);
        if train.carriage_index == 0 && angle != train.angle {
            train.corner_count = train.corner_count.wrapping_add(1);
            whistle = Some(*position);
        }
        movement.push((old, angle, *position, train.angle));
    }
    for (_, position) in world.query_mut::<(&PlayerId, &mut Position)>() {
        for (old, angle, next, next_angle) in &movement {
            if aboard(*position, *old, *angle) {
                let (x, y) = local(*position, *old, *angle);
                let target = Position {
                    x: next.x + x * next_angle.cos() - y * next_angle.sin(),
                    y: next.y + x * next_angle.sin() + y * next_angle.cos(),
                };
                if crate::terrain::can_stand(target, PLAYER_RADIUS) {
                    *position = target;
                }
                break;
            }
        }
    }
    whistle
}

// Called once per simulation tick, after movement, never once per carriage.
pub fn replenish(world: &mut hecs::World) {
    let carriages = platforms(world);
    for (position, health, ammo, weapon) in world.query_mut::<(
        &Position,
        &mut crate::components::Health,
        &mut crate::components::Ammo,
        &crate::types::WeaponType,
    )>() {
        if carriages
            .iter()
            .any(|(center, angle)| aboard(*position, *center, *angle))
        {
            health.0 = (health.0 + crate::constants::TRAIN_HEAL_PER_SECOND * TICK_DURATION)
                .min(crate::constants::MAX_HEALTH);
            ammo.0 = (ammo.0 + crate::constants::TRAIN_AMMO_PER_SECOND * TICK_DURATION)
                .min(crate::constants::ammo_capacity(*weapon) as f32);
        }
    }
}
