use serde::{Deserialize, Serialize};

pub type PlayerId = u64;
pub type GameId = String;
pub type EntityId = u64;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CharacterType {
    #[default]
    Blue,
    Brown,
    Green,
    Black,
    Gray,
    Olive,
    Cyan,
    Survivor,
    Zombie,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponType {
    Pistol,
    DesertEagle,
    Ak47,
    RocketLauncher,
    Flamethrower,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Object {
    Train {
        carriage_index: u32,
        corner_count: u32,
        aim_angle: f32,
        width: f32,
        height: f32,
        speed: f32,
    },
    Bullet {
        owner: PlayerId,
        weapon_type: WeaponType,
        aim_angle: f32,
    },
    Weapon {
        weapon_type: WeaponType,
        expires_at_millis: u64,
        lifetime_millis: u64,
    },
    Player {
        player_id: PlayerId,
        character_type: CharacterType,
        username: String,
        health: f32,
        ammo: u32,
        ammo_capacity: u32,
        speed: f32,
        riding_train: bool,
        respawn_count: u32,
        kills: u32,
        deaths: u32,
        aim_angle: f32,
        weapon_type: WeaponType,
    },
    Boundary {
        width: f32,
        height: f32,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ObjectState {
    pub entity_id: EntityId,
    pub position: Position,
    pub object: Object,
}
