use crate::types::{EntityId, PlayerId, WeaponType};

pub const PLAYER_RADIUS: f32 = 16.0;

// Authoritative HP, shared by humans and bots.
pub struct Health(pub f32);

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Score {
    pub kills: u32,
    pub deaths: u32,
}

// Radians measured from +X; mouse aim and bot aim use the same convention.
pub struct AimAngle(pub f32);

// Stable snapshot ID. It is separate from hecs handles and logical PlayerId.
pub struct ObjectId(pub EntityId);
// Circular collision radius in world units.
pub struct Collider(pub f32);

#[derive(Default)]
pub struct Shooting {
    // Latest fire-button state; releasing stops repeated shots.
    pub held: bool,
    // Remember a quick press even if the release arrives before the next tick.
    pub pending_shot: bool,
    // Seconds until another shot is allowed, independent of button state.
    pub cooldown: f32,
}

#[derive(Default)]
pub struct MovementInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub sprinting: bool,
}

// Velocity is in world units per second. Bullet launch adds the shooter's movement.
pub struct Velocity {
    pub x: f32,
    pub y: f32,
}

// Remaining seconds before a projectile expires.
pub struct Lifetime(pub f32);

// Projectile ownership is used for damage credit and immunity to its own shooter.
pub struct Bullet {
    pub owner: PlayerId,
    pub weapon_type: WeaponType,
    pub aim_angle: f32,
}

// Increment on teleport so the browser does not interpolate through the map.
pub struct RespawnCount(pub u32);

// Fractional ammo lets train resupply accumulate smoothly; firing spends whole rounds.
pub struct Ammo(pub f32);

// Tick-start position, captured before transport and walking.
pub struct PreviousPosition(pub crate::types::Position);

// Server expiry uses Instant; the browser gets wall-clock milliseconds for its timer.
pub struct WeaponPickup {
    pub weapon_type: WeaponType,
    pub expires_at: tokio::time::Instant,
    pub expires_at_millis: u64,
}

impl MovementInput {
    // Apply held sprint to terrain walking speed; trains and bullets do not use this.
    pub fn speed(&self, position: crate::types::Position) -> f32 {
        crate::terrain::speed(position)
            * if self.sprinting {
                crate::constants::SPRINT_SPEED_MULTIPLIER
            } else {
                1.0
            }
    }
}
