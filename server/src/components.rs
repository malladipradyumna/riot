use crate::types::{EntityId, PlayerId, WeaponType};

pub const PLAYER_RADIUS: f32 = 16.0;

pub struct Health(pub f32);

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Score {
    pub kills: u32,
    pub deaths: u32,
}

pub struct AimAngle(pub f32);

pub struct ObjectId(pub EntityId);
pub struct Collider(pub f32);

#[derive(Default)]
pub struct Shooting {
    pub held: bool,
    pub pending_shot: bool,
    pub cooldown: f32,
}

#[derive(Default)]
pub struct MovementInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

// Bullets use Position, Velocity, and Lifetime.
// Their hecs Entity handles identify them independently of players.
pub struct Velocity {
    pub x: f32,
    pub y: f32,
}

pub struct Lifetime(pub f32);

pub struct Bullet {
    pub owner: PlayerId,
    pub weapon_type: WeaponType,
    pub aim_angle: f32,
}

pub struct RespawnCount(pub u32);

pub struct Ammo(pub f32);
