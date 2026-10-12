// Player setup and ground pickups. Humans and bots start with the same gameplay components.
use super::{Game, Player, timestamp_millis};
use crate::{
    components::{
        AimAngle, Ammo, Collider, Health, MovementInput, ObjectId, PLAYER_RADIUS, PreviousPosition,
        RespawnCount, Score, Shooting, WeaponPickup,
    },
    constants::{FIRE_INTERVAL, MAP_HEIGHT, MAP_WIDTH, MAX_HEALTH},
    protocol::ServerMessage,
    types::{CharacterType, PlayerId, Position, WeaponType},
};
use std::time::Duration;
use tokio::{sync::mpsc, time};
use uuid::Uuid;

impl Game {
    // Allocate IDs and create the shared player components for a human or bot.
    pub(super) fn spawn_player(
        &mut self,
        username: String,
        character: CharacterType,
        sender_for_client: Option<mpsc::Sender<ServerMessage>>,
        last_processed_request_id: String,
    ) -> (PlayerId, hecs::Entity, Position) {
        let player_id = self.next_player_id;
        self.next_player_id += 1;
        let object_id = self.next_entity_id;
        self.next_entity_id += 1;
        let position = random_spawn_position();
        let weapon = WeaponType::Pistol;
        let entity = self.world.spawn((
            player_id,
            username,
            character,
            position,
            PreviousPosition(position),
            Health(MAX_HEALTH),
            Score::default(),
            weapon,
            Ammo(crate::constants::ammo_capacity(weapon) as f32),
            MovementInput::default(),
            AimAngle(0.0),
            ObjectId(object_id),
            Collider(PLAYER_RADIUS),
            Shooting::default(),
            RespawnCount(0),
        ));
        // A bot has a brain instead of a client connection; everything else is shared.
        if sender_for_client.is_none() {
            self.world
                .insert_one(entity, crate::bot::Bot::default())
                .unwrap();
        }
        self.players.insert(
            player_id,
            Player {
                entity,
                last_processed_request_id,
                sender_for_client,
            },
        );
        (player_id, entity, position)
    }

    // Spawn the requested opponents without creating fake connections.
    pub(super) fn spawn_bots(&mut self) {
        for number in 1..=self.bot_settings.count {
            self.spawn_player(
                format!("Hitman {number}"),
                CharacterType::Villain,
                None,
                String::new(),
            );
        }
    }

    // Try to place one random weapon on slower terrain, respecting the pickup cap.
    pub(super) fn spawn_weapon_pickup(&mut self) {
        if self.world.query::<&WeaponPickup>().iter().count()
            >= crate::constants::MAX_WEAPON_PICKUPS
        {
            return;
        }
        // Bound the search so a crowded map cannot stall the simulation.
        for _ in 0..100 {
            let position = random_spawn_position();
            if !crate::terrain::can_stand(position, crate::constants::WEAPON_PICKUP_RADIUS) {
                continue;
            }
            // Uniform candidate positions, accepted inversely to walking speed.
            // Water (70) gets all candidates; normal ground (200) gets 35%.
            let sample = u32::from_le_bytes(Uuid::new_v4().as_bytes()[..4].try_into().unwrap());
            if sample as f64 / u32::MAX as f64 > 70.0 / crate::terrain::speed(position) as f64 {
                continue;
            }
            let occupied = self
                .world
                .query::<(&Position, Option<&PlayerId>, Option<&WeaponPickup>)>()
                .iter()
                .any(|(other, player, pickup)| {
                    (player.is_some() || pickup.is_some())
                        && (other.x - position.x).powi(2) + (other.y - position.y).powi(2)
                            < 100.0_f32.powi(2)
                });
            if occupied {
                continue;
            }
            let object_id = self.next_entity_id;
            self.next_entity_id += 1;
            self.world.spawn((
                ObjectId(object_id),
                position,
                WeaponPickup {
                    weapon_type: random_weapon(),
                    expires_at: time::Instant::now()
                        + Duration::from_secs(crate::constants::WEAPON_LIFETIME_SECONDS),
                    expires_at_millis: timestamp_millis()
                        + crate::constants::WEAPON_LIFETIME_SECONDS * 1000,
                },
            ));
            break;
        }
    }

    // Expire old guns, then equip the first participant who crosses each pickup.
    pub(super) fn update_weapon_pickups(&mut self) {
        let now = time::Instant::now();
        let pickups: Vec<_> = self
            .world
            .query_mut::<(hecs::Entity, &Position, &WeaponPickup)>()
            .into_iter()
            .map(|(entity, position, pickup)| {
                (entity, *position, pickup.weapon_type, pickup.expires_at)
            })
            .collect();
        for (pickup, position, weapon, expires_at) in pickups {
            if expires_at <= now {
                let _ = self.world.despawn(pickup);
                continue;
            }
            // Swept pickup detection also catches a player who crosses it between ticks.
            // Player ID breaks ties when several participants arrive together.
            let winner = self
                .world
                .query::<(hecs::Entity, &PlayerId, &Position, &PreviousPosition)>()
                .iter()
                .filter(|(_, id, current, previous)| {
                    self.players.get(id).is_some_and(|p| p.is_active())
                        && crate::collision::circle_time(
                            previous.0,
                            **current,
                            position,
                            PLAYER_RADIUS + crate::constants::WEAPON_PICKUP_RADIUS,
                        )
                        .is_some()
                })
                .min_by_key(|(_, id, _, _)| **id)
                .map(|(entity, _, _, _)| entity);
            if let Some(player) = winner {
                if let Ok((equipped, ammo, shooting)) =
                    self.world
                        .query_one_mut::<(&mut WeaponType, &mut Ammo, &mut Shooting)>(player)
                {
                    *equipped = weapon;
                    ammo.0 = crate::constants::ammo_capacity(weapon) as f32;
                    shooting.cooldown = FIRE_INTERVAL;
                    shooting.pending_shot = false;
                }
                let _ = self.world.despawn(pickup);
            }
        }
    }
}

// Choose among the four implemented guns using their intended spawn weights.
fn random_weapon() -> WeaponType {
    // The last UUID byte is random (outside its fixed version/variant bits).
    // Rejection avoids modulo bias when reducing 256 possible values to 100.
    loop {
        let roll = Uuid::new_v4().as_bytes()[15];
        if roll >= 200 {
            continue;
        }
        return match roll % 100 {
            0..=39 => WeaponType::Pistol,
            40..=69 => WeaponType::Ak47,
            70..=89 => WeaponType::DesertEagle,
            _ => WeaponType::RocketLauncher,
        };
    }
}

// Sample the arena until the whole player circle fits outside walls.
pub(super) fn random_spawn_position() -> Position {
    loop {
        let random = Uuid::new_v4().as_u128();
        let x = (random as u32) as f64 / u32::MAX as f64;
        let y = ((random >> 32) as u32) as f64 / u32::MAX as f64;
        let position = Position {
            x: PLAYER_RADIUS + x as f32 * (MAP_WIDTH - 2.0 * PLAYER_RADIUS),
            y: PLAYER_RADIUS + y as f32 * (MAP_HEIGHT - 2.0 * PLAYER_RADIUS),
        };
        if crate::terrain::can_stand(position, PLAYER_RADIUS) {
            return position;
        }
    }
}
