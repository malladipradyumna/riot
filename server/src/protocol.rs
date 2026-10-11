use crate::types::{CharacterType, GameId, PlayerId, Position, WeaponType};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum ClientMessage {
    JoinGame {
        client_request_id: String,
        timestamp_millis: u64,
        username: String,
        #[serde(default)]
        character_type: CharacterType,
        game_id: Option<GameId>,
    },
    ExitGame {
        client_request_id: String,
        timestamp_millis: u64,
        username: String,
        game_id: GameId,
        player_id: PlayerId,
    },
    PlayerInput {
        client_request_id: String,
        timestamp_millis: u64,
        player_id: PlayerId,
        game_id: GameId,
        up: bool,
        down: bool,
        left: bool,
        right: bool,
        shooting: bool,
        #[serde(default)]
        sprinting: bool,
        aim_angle: f32,
    },
}

#[derive(Serialize, Deserialize, Debug)]
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
    Welcome {
        server_request_id: String,
        timestamp_millis: u64,
        player_id: PlayerId,
        game_id: GameId,
    },
    PlayerDied {
        server_request_id: String,
        timestamp_millis: u64,
        player_id: PlayerId,
        game_id: GameId,
        last_processed_client_request_id: String,
        dead_player_id: PlayerId,
    },
    PlayerLeft {
        server_request_id: String,
        timestamp_millis: u64,
        player_id: PlayerId,
        game_id: GameId,
        last_processed_client_request_id: String,
        abandoned_player_id: PlayerId,
    },
    Pong,
}
