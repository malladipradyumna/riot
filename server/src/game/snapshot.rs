// Copy ECS state into one shared snapshot. Encoding happens once, not per client.
use super::{Game, timestamp_millis};
use crate::{
    components::{
        AimAngle, Ammo, Bullet, Health, MovementInput, ObjectId, RespawnCount, Score, WeaponPickup,
    },
    types::{CharacterType, Object, ObjectState, Position, WeaponType},
};
use tokio::time;

impl Game {
    // Publish players, bots, bullets, trains, pickups, and round state as one frame.
    pub(super) fn send_player_snapshots(&mut self) {
        // Build one owned snapshot; the watch channel retains only the latest encoded frame.
        let platforms = crate::train::platforms(&self.world);
        let mut objects = Vec::with_capacity(self.players.len());
        for (player_id, player) in &self.players {
            if !player.is_active() {
                continue;
            }
            let respawn_count = self
                .world
                .get::<&RespawnCount>(player.entity)
                .map_or(0, |count| count.0);
            let ammo = self
                .world
                .get::<&Ammo>(player.entity)
                .map_or(0, |ammo| ammo.0.floor() as u32);
            if let Ok((
                username,
                character,
                position,
                health,
                score,
                aim,
                weapon,
                object_id,
                input,
            )) = self.world.query_one_mut::<(
                &String,
                &CharacterType,
                &Position,
                &Health,
                &Score,
                &AimAngle,
                &WeaponType,
                &ObjectId,
                &MovementInput,
            )>(player.entity)
            {
                objects.push(ObjectState {
                    entity_id: object_id.0,
                    position: *position,
                    object: Object::Player {
                        player_id: *player_id,
                        character_type: *character,
                        username: username.clone(),
                        health: health.0,
                        ammo,
                        ammo_capacity: crate::constants::ammo_capacity(*weapon),
                        speed: input.speed(*position),
                        respawn_count,
                        kills: score.kills,
                        deaths: score.deaths,
                        riding_train: platforms.iter().any(|(center, angle)| {
                            crate::train::aboard(*position, *center, *angle)
                        }),
                        aim_angle: aim.0,
                        weapon_type: *weapon,
                    },
                });
            }
        }
        for (object_id, position, bullet) in
            self.world.query_mut::<(&ObjectId, &Position, &Bullet)>()
        {
            objects.push(ObjectState {
                entity_id: object_id.0,
                position: *position,
                object: Object::Bullet {
                    owner: bullet.owner,
                    weapon_type: bullet.weapon_type,
                    aim_angle: bullet.aim_angle,
                },
            });
        }
        for (id, position, train) in self
            .world
            .query_mut::<(&ObjectId, &Position, &crate::train::Train)>()
        {
            objects.push(ObjectState {
                entity_id: id.0,
                position: *position,
                object: Object::Train {
                    aim_angle: train.angle,
                    carriage_index: train.carriage_index,
                    corner_count: train.corner_count,
                    width: crate::constants::TRAIN_WIDTH,
                    height: crate::constants::TRAIN_HEIGHT,
                    speed: crate::constants::TRAIN_SPEED,
                },
            });
        }
        for (id, position, pickup) in self
            .world
            .query_mut::<(&ObjectId, &Position, &WeaponPickup)>()
        {
            if pickup.expires_at > time::Instant::now() {
                objects.push(ObjectState {
                    entity_id: id.0,
                    position: *position,
                    object: Object::Weapon {
                        weapon_type: pickup.weapon_type,
                        expires_at_millis: pickup.expires_at_millis,
                        lifetime_millis: crate::constants::WEAPON_LIFETIME_SECONDS * 1000,
                    },
                });
            }
        }
        objects.sort_by_key(|object| object.entity_id);
        let timestamp_millis = timestamp_millis();
        // Equal millisecond timestamps would make the browser discard a newer frame.
        let timestamp_millis = timestamp_millis.max(self.last_snapshot_timestamp + 1);
        self.last_snapshot_timestamp = timestamp_millis;
        // Bots have no client requests to acknowledge. The frame still includes them.
        let acknowledgements = self
            .players
            .iter()
            .filter(|(_, player)| player.is_human())
            .map(|(id, player)| crate::wire::InputAck {
                player_id: *id,
                last_processed_client_request_id: player.last_processed_request_id.clone(),
            })
            .collect();
        // Translate the remaining monotonic duration into a browser-friendly wall-clock deadline.
        let round = crate::wire::RoundState {
            number: self.round_number,
            intermission: self.intermission,
            ends_at_millis: timestamp_millis
                + self
                    .round_deadline
                    .saturating_duration_since(time::Instant::now())
                    .as_millis() as u64,
        };
        let encoded = crate::codec::encode_snapshot(
            &self.id,
            timestamp_millis,
            &objects,
            acknowledgements,
            round,
        );
        self.snapshot_sender.send_replace(Some(encoded));
    }
}
