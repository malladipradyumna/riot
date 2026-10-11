pub const TICK_RATE: u32 = 30;
pub const TICK_DURATION: f32 = 1.0 / TICK_RATE as f32;

pub const MAP_WIDTH: f32 = 4800.0;
pub const MAP_HEIGHT: f32 = 1800.0;

pub const MAX_HEALTH: f32 = 100.0;
pub const BULLET_SPEED_MULTIPLIER: f32 = 3.0;
pub const BULLET_RADIUS: f32 = 4.0;
pub const FIRE_INTERVAL: f32 = 0.2;
pub const BULLET_LIFETIME: f32 = 5.0;

pub const SNAPSHOT_RATE: u32 = 15;

// Direct-hit damage; rockets currently use the same projectile behavior (no splash).
pub fn weapon_damage(weapon: crate::types::WeaponType) -> f32 {
    use crate::types::WeaponType::*;
    match weapon {
        Pistol => 25.0,
        Ak47 => 20.0,
        DesertEagle => 50.0,
        RocketLauncher => 100.0,
        Flamethrower => 10.0,
    }
}

pub const TRAIN_SPEED: f32 = 400.0;
pub const TRAIN_WIDTH: f32 = 180.0;
pub const TRAIN_HEIGHT: f32 = 90.0;

pub const TRAIN_CARRIAGES: u64 = 4;
pub const TRAIN_CARRIAGE_SPACING: f32 = TRAIN_WIDTH + 12.0;
pub const TRAIN_HEAL_PER_SECOND: f32 = 3.0;
pub const TRAIN_AMMO_PER_SECOND: f32 = 10.0;

pub fn ammo_capacity(weapon: crate::types::WeaponType) -> u32 {
    use crate::types::WeaponType::*;
    match weapon {
        Pistol => 24,
        Ak47 => 90,
        DesertEagle => 14,
        RocketLauncher => 6,
        Flamethrower => 60,
    }
}

pub const INITIAL_WEAPON_PICKUPS: usize = 32;
pub const MAX_WEAPON_PICKUPS: usize = 64;
pub const WEAPON_SPAWN_SECONDS: f64 = 1.5;
pub const WEAPON_LIFETIME_SECONDS: u64 = 30;
pub const WEAPON_PICKUP_RADIUS: f32 = 24.0;

pub const SPRINT_SPEED_MULTIPLIER: f32 = 1.9;

pub const ROUND_SECONDS: u64 = 300;
pub const ROUND_BREAK_SECONDS: u64 = 10;
