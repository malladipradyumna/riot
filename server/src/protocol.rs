// Application messages used by game logic. codec.rs handles the protobuf boundary.
use crate::types::{CharacterType, GameId, PlayerId, Position, WeaponType};

#[derive(Debug)]
// Client clocks and unused wire metadata are deliberately absent from simulation messages.
pub enum ClientMessage {
    // None creates a room; Some joins the specified existing room.
    JoinGame {
        client_request_id: String,
        username: String,
        character_type: CharacterType,
        game_id: Option<GameId>,
        bot_settings: crate::bot::Settings,
    },
    ExitGame {
        game_id: GameId,
        player_id: PlayerId,
    },
    // Intent and request identity only; positions and damage are server-owned.
    PlayerInput {
        client_request_id: String,
        player_id: PlayerId,
        game_id: GameId,
        up: bool,
        down: bool,
        left: bool,
        right: bool,
        shooting: bool,
        sprinting: bool,
        aim_angle: f32,
    },
}

#[derive(Debug)]
// Discrete messages use mpsc. Full state frames are encoded separately for watch.
pub enum ServerMessage {
    PlayerDamaged {
        game_id: GameId,
        player_id: PlayerId,
        damage: f32,
        health: f32,
    },
    TrainWhistle {
        game_id: GameId,
        position: Position,
    },
    // The victim's old position locates death audio after their immediate respawn.
    PlayerKilled {
        game_id: GameId,
        killer_id: PlayerId,
        victim_id: PlayerId,
        killer_name: String,
        victim_name: String,
        weapon_type: WeaponType,
        position: Position,
    },
    GunFired {
        server_request_id: String,
        timestamp_millis: u64,
        player_id: PlayerId,
        game_id: GameId,
        weapon_type: WeaponType,
        position: Position,
        aim_angle: f32,
    },
    // Sent before the connection is allowed to forward snapshots.
    Welcome {
        server_request_id: String,
        timestamp_millis: u64,
        player_id: PlayerId,
        game_id: GameId,
    },
}
