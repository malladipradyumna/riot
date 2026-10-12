use crate::{
    protocol::{ClientMessage, ServerMessage},
    types, wire,
};
use bytes::Bytes;
use prost::Message;

// Translate protobuf messages here so gameplay code can use our own Rust types.
pub fn decode_client(data: &[u8]) -> Result<ClientMessage, &'static str> {
    use wire::client_envelope::Payload;
    // Reject malformed frames and missing oneof payloads before they reach gameplay.
    let message = wire::ClientEnvelope::decode(data).map_err(|_| "CORRUPT_MSG")?;
    Ok(match message.payload.ok_or("CORRUPT_MSG")? {
        // A client may choose human cosmetics, but cannot request the bot-only sprite.
        Payload::JoinGame(p) => ClientMessage::JoinGame {
            client_request_id: p.client_request_id,
            username: p.username,
            bot_settings: crate::bot::Settings::new(
                p.bot_count,
                p.bot_difficulty,
                p.game_id.is_none(),
            )?,
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
                wire::CharacterType::Villain => return Err("CORRUPT_MSG"),
            },
        },
        Payload::ExitGame(p) => ClientMessage::ExitGame {
            game_id: p.game_id,
            player_id: p.player_id,
        },
        Payload::PlayerInput(p) => ClientMessage::PlayerInput {
            client_request_id: p.client_request_id,
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

// Map gameplay enums to stable protobuf values explicitly.
fn weapon(value: types::WeaponType) -> i32 {
    (match value {
        types::WeaponType::Pistol => wire::WeaponType::Pistol,
        types::WeaponType::DesertEagle => wire::WeaponType::DesertEagle,
        types::WeaponType::Ak47 => wire::WeaponType::Ak47,
        types::WeaponType::RocketLauncher => wire::WeaponType::RocketLauncher,
    }) as i32
}
// Copy a gameplay point into the protobuf representation.
fn position(value: types::Position) -> wire::Position {
    wire::Position {
        x: value.x,
        y: value.y,
    }
}
// Convert one snapshot object into its matching protobuf oneof.
fn object(value: &types::ObjectState) -> wire::ObjectState {
    use wire::object_state::Object;
    // Keep the stable outer ID/position alongside the object-specific payload.
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
                types::CharacterType::Villain => wire::CharacterType::Villain as i32,
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
    };
    wire::ObjectState {
        entity_id: value.entity_id,
        position: Some(position(value.position)),
        object: Some(object),
    }
}

// Put a server payload in the envelope and return shared immutable bytes.
fn encode(payload: wire::server_envelope::Payload) -> Bytes {
    Bytes::from(
        wire::ServerEnvelope {
            payload: Some(payload),
        }
        .encode_to_vec(),
    )
}
// Encode a short application error for a client that cannot proceed.
pub fn error(code: &str) -> Bytes {
    encode(wire::server_envelope::Payload::Error(wire::Error {
        code: code.into(),
    }))
}
// Serialize the shared state frame once for all clients in a game.
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

// Serialize a discrete reply or event, separate from the snapshot channel.
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
    })
}
