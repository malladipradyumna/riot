use crate::{
    protocol::{ClientMessage, ServerMessage},
    types, wire,
};
use bytes::Bytes;
use prost::Message;

// Translate protobuf messages here so gameplay code can use our own Rust types.
pub fn decode_client(data: &[u8]) -> Result<ClientMessage, &'static str> {
    use wire::client_envelope::Payload;
    let message = wire::ClientEnvelope::decode(data).map_err(|_| "CORRUPT_MSG")?;
    Ok(match message.payload.ok_or("CORRUPT_MSG")? {
        Payload::JoinGame(p) => ClientMessage::JoinGame {
            client_request_id: p.client_request_id,
            timestamp_millis: p.timestamp_millis,
            username: p.username,
            game_id: p.game_id,
            character_type: match wire::CharacterType::try_from(p.character_type)
                .map_err(|_| "CORRUPT_MSG")?
            {
                wire::CharacterType::Blue => types::CharacterType::Blue,
                wire::CharacterType::Brown => types::CharacterType::Brown,
                wire::CharacterType::Black => types::CharacterType::Black,
                wire::CharacterType::Gray => types::CharacterType::Gray,
                wire::CharacterType::Olive => types::CharacterType::Olive,
                wire::CharacterType::Cyan => types::CharacterType::Cyan,
                wire::CharacterType::Survivor => types::CharacterType::Survivor,
                wire::CharacterType::Zombie => types::CharacterType::Zombie,
                wire::CharacterType::Green => types::CharacterType::Green,
            },
        },
        Payload::ExitGame(p) => ClientMessage::ExitGame {
            client_request_id: p.client_request_id,
            timestamp_millis: p.timestamp_millis,
            username: p.username,
            game_id: p.game_id,
            player_id: p.player_id,
        },
        Payload::PlayerInput(p) => ClientMessage::PlayerInput {
            client_request_id: p.client_request_id,
            timestamp_millis: p.timestamp_millis,
            game_id: p.game_id,
            player_id: p.player_id,
            up: p.up,
            down: p.down,
            left: p.left,
            right: p.right,
            shooting: p.shooting,
            sprinting: p.sprinting,
            aim_angle: p.aim_angle,
        },
    })
}

fn weapon(value: types::WeaponType) -> i32 {
    (match value {
        types::WeaponType::Pistol => wire::WeaponType::Pistol,
        types::WeaponType::DesertEagle => wire::WeaponType::DesertEagle,
        types::WeaponType::Ak47 => wire::WeaponType::Ak47,
        types::WeaponType::RocketLauncher => wire::WeaponType::RocketLauncher,
        types::WeaponType::Flamethrower => wire::WeaponType::Flamethrower,
    }) as i32
}
fn position(value: types::Position) -> wire::Position {
    wire::Position {
        x: value.x,
        y: value.y,
    }
}
fn object(value: &types::ObjectState) -> wire::ObjectState {
    use wire::object_state::Object;
    let object = match &value.object {
        types::Object::Train {
            carriage_index,
            corner_count,
            aim_angle,
            width,
            height,
            speed,
        } => Object::Train(wire::Train {
            carriage_index: *carriage_index,
            corner_count: *corner_count,
            aim_angle: *aim_angle,
            width: *width,
            height: *height,
            speed: *speed,
        }),
        types::Object::Player {
            player_id,
            character_type,
            username,
            health,
            ammo,
            ammo_capacity,
            speed,
            riding_train,
            respawn_count,
            kills,
            deaths,
            aim_angle,
            weapon_type,
        } => Object::Player(wire::Player {
            player_id: *player_id,
            character_type: match character_type {
                types::CharacterType::Blue => wire::CharacterType::Blue as i32,
                types::CharacterType::Brown => wire::CharacterType::Brown as i32,
                types::CharacterType::Black => wire::CharacterType::Black as i32,
                types::CharacterType::Gray => wire::CharacterType::Gray as i32,
                types::CharacterType::Olive => wire::CharacterType::Olive as i32,
                types::CharacterType::Cyan => wire::CharacterType::Cyan as i32,
                types::CharacterType::Survivor => wire::CharacterType::Survivor as i32,
                types::CharacterType::Zombie => wire::CharacterType::Zombie as i32,
                types::CharacterType::Green => wire::CharacterType::Green as i32,
            },
            username: username.clone(),
            health: *health,
            ammo: *ammo,
            ammo_capacity: *ammo_capacity,
            speed: *speed,
            riding_train: *riding_train,
            respawn_count: *respawn_count,
            kills: *kills,
            deaths: *deaths,
            aim_angle: *aim_angle,
            weapon_type: weapon(*weapon_type),
        }),
        types::Object::Bullet {
            owner,
            weapon_type,
            aim_angle,
        } => Object::Bullet(wire::Bullet {
            owner: *owner,
            weapon_type: weapon(*weapon_type),
            aim_angle: *aim_angle,
        }),
        types::Object::Weapon {
            weapon_type,
            expires_at_millis,
            lifetime_millis,
        } => Object::Weapon(wire::Weapon {
            weapon_type: weapon(*weapon_type),
            expires_at_millis: *expires_at_millis,
            lifetime_millis: *lifetime_millis,
        }),
        types::Object::Boundary { width, height } => Object::Boundary(wire::Boundary {
            width: *width,
            height: *height,
        }),
    };
    wire::ObjectState {
        entity_id: value.entity_id,
        position: Some(position(value.position)),
        object: Some(object),
    }
}

fn encode(payload: wire::server_envelope::Payload) -> Bytes {
    Bytes::from(
        wire::ServerEnvelope {
            payload: Some(payload),
        }
        .encode_to_vec(),
    )
}
pub fn error(code: &str) -> Bytes {
    encode(wire::server_envelope::Payload::Error(wire::Error {
        code: code.into(),
    }))
}
pub fn encode_snapshot(
    game_id: &str,
    timestamp_millis: u64,
    objects: &[types::ObjectState],
    acknowledgements: Vec<wire::InputAck>,
    round: wire::RoundState,
) -> Bytes {
    encode(wire::server_envelope::Payload::StateSnapshot(
        wire::StateSnapshot {
            game_id: game_id.into(),
            timestamp_millis,
            objects: objects.iter().map(object).collect(),
            acknowledgements,
            round: Some(round),
        },
    ))
}

pub fn encode_server(message: &ServerMessage) -> Bytes {
    use wire::server_envelope::Payload;
    encode(match message {
        ServerMessage::PlayerDamaged {
            game_id,
            player_id,
            damage,
            health,
        } => Payload::PlayerDamaged(wire::PlayerDamaged {
            game_id: game_id.clone(),
            player_id: *player_id,
            damage: *damage,
            health: *health,
        }),
        ServerMessage::TrainWhistle {
            game_id,
            position: pos,
        } => Payload::TrainWhistle(wire::TrainWhistle {
            game_id: game_id.clone(),
            position: Some(position(*pos)),
        }),
        ServerMessage::Welcome {
            server_request_id,
            timestamp_millis,
            player_id,
            game_id,
        } => Payload::Welcome(wire::Welcome {
            server_request_id: server_request_id.clone(),
            timestamp_millis: *timestamp_millis,
            player_id: *player_id,
            game_id: game_id.clone(),
        }),
        ServerMessage::GunFired {
            server_request_id,
            timestamp_millis,
            player_id,
            game_id,
            weapon_type,
            position: pos,
            aim_angle,
        } => Payload::GunFired(wire::GunFired {
            server_request_id: server_request_id.clone(),
            timestamp_millis: *timestamp_millis,
            player_id: *player_id,
            game_id: game_id.clone(),
            weapon_type: weapon(*weapon_type),
            position: Some(position(*pos)),
            aim_angle: *aim_angle,
        }),
        ServerMessage::PlayerKilled {
            game_id,
            killer_id,
            victim_id,
            killer_name,
            victim_name,
            weapon_type,
            position,
        } => Payload::PlayerKilled(wire::PlayerKilled {
            game_id: game_id.clone(),
            killer_id: *killer_id,
            victim_id: *victim_id,
            killer_name: killer_name.clone(),
            victim_name: victim_name.clone(),
            weapon_type: weapon(*weapon_type),
            position: Some(wire::Position {
                x: position.x,
                y: position.y,
            }),
        }),
        ServerMessage::PlayerDied {
            server_request_id,
            timestamp_millis,
            player_id,
            game_id,
            last_processed_client_request_id,
            dead_player_id,
        } => Payload::PlayerDied(wire::PlayerDied {
            server_request_id: server_request_id.clone(),
            timestamp_millis: *timestamp_millis,
            player_id: *player_id,
            game_id: game_id.clone(),
            last_processed_client_request_id: last_processed_client_request_id.clone(),
            dead_player_id: *dead_player_id,
        }),
        ServerMessage::PlayerLeft {
            server_request_id,
            timestamp_millis,
            player_id,
            game_id,
            last_processed_client_request_id,
            abandoned_player_id,
        } => Payload::PlayerLeft(wire::PlayerLeft {
            server_request_id: server_request_id.clone(),
            timestamp_millis: *timestamp_millis,
            player_id: *player_id,
            game_id: game_id.clone(),
            last_processed_client_request_id: last_processed_client_request_id.clone(),
            abandoned_player_id: *abandoned_player_id,
        }),
        ServerMessage::Pong => Payload::Pong(wire::Pong {}),
    })
}
