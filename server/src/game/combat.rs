// Bullets and damage live here. The game loop decides when these systems run.
use super::{Game, spawning::random_spawn_position, timestamp_millis};
use crate::{
    components::{
        AimAngle, Ammo, Bullet, Collider, Health, Lifetime, MovementInput, ObjectId, PLAYER_RADIUS,
        PreviousPosition, RespawnCount, Score, Shooting, Velocity,
    },
    constants::{
        BULLET_LIFETIME, BULLET_RADIUS, BULLET_SPEED_MULTIPLIER, FIRE_INTERVAL, MAX_HEALTH,
        TICK_DURATION,
    },
    protocol::ServerMessage,
    types::{PlayerId, Position, WeaponType},
};
use std::collections::HashSet;
use uuid::Uuid;

impl Game {
    // Spend ammo and turn eligible firing requests into bullet entities.
    pub(super) fn fire_bullets(&mut self) {
        // Gather shots while borrowing player components; spawn them after the query ends.
        let mut shots = Vec::new();
        for (owner, position, previous, aim, weapon, shooting, ammo, input) in
            self.world.query_mut::<(
                &PlayerId,
                &Position,
                &PreviousPosition,
                &AimAngle,
                &WeaponType,
                &mut Shooting,
                &mut Ammo,
                &MovementInput,
            )>()
        {
            // Held fire repeats; pending_shot remembers one quick press. Both respect cooldown.
            shooting.cooldown = (shooting.cooldown - TICK_DURATION).max(0.0);
            if (!shooting.held && !shooting.pending_shot) || shooting.cooldown > f32::EPSILON {
                continue;
            }
            let Some(player) = self.players.get(owner) else {
                continue;
            };
            if !player.is_active() {
                continue;
            }
            if ammo.0 < 1.0 {
                shooting.pending_shot = false;
                continue;
            }
            ammo.0 -= 1.0;
            shooting.cooldown = FIRE_INTERVAL;
            shooting.pending_shot = false;
            // Sample only when a bot actually fires; human aim stays unchanged.
            let shot_angle = aim.0
                + if player.is_human() {
                    0.0
                } else {
                    crate::bot::aim_error(self.bot_settings.difficulty)
                };
            let offset = PLAYER_RADIUS + BULLET_RADIUS + 1.0;
            let muzzle = Position {
                x: position.x + shot_angle.cos() * offset,
                y: position.y + shot_angle.sin() * offset,
            };
            // Do not spawn a projectile past a wall between the body and muzzle.
            let muzzle = crate::terrain::segment_hit_fraction(*position, muzzle, BULLET_RADIUS)
                .map_or(muzzle, |t| Position {
                    x: position.x + (muzzle.x - position.x) * t,
                    y: position.y + (muzzle.y - position.y) * t,
                });
            let bullet_speed = input.speed(*position) * BULLET_SPEED_MULTIPLIER;
            shots.push((
                *owner,
                *weapon,
                shot_angle,
                muzzle,
                Velocity {
                    // Actual tick displacement includes train transport and walking,
                    // and excludes movement blocked by walls.
                    x: shot_angle.cos() * bullet_speed
                        + (position.x - previous.0.x) / TICK_DURATION,
                    y: shot_angle.sin() * bullet_speed
                        + (position.y - previous.0.y) / TICK_DURATION,
                },
            ));
        }
        // Finish borrowing player components before spawning new entities.
        for (owner, weapon_type, aim_angle, position, velocity) in shots {
            // Travel direction includes inherited velocity, so it can differ from muzzle aim.
            let bullet_angle = velocity.y.atan2(velocity.x);
            let object_id = self.next_entity_id;
            self.next_entity_id += 1;
            self.world.spawn((
                ObjectId(object_id),
                position,
                velocity,
                Bullet {
                    owner,
                    weapon_type,
                    aim_angle: bullet_angle,
                },
                Collider(BULLET_RADIUS),
                Lifetime(BULLET_LIFETIME),
            ));
            for player in self.players.values() {
                // Audio events must not block simulation if a client is slow.
                player.send_event(ServerMessage::GunFired {
                    server_request_id: Uuid::new_v4().to_string(),
                    timestamp_millis: timestamp_millis(),
                    player_id: owner,
                    game_id: self.id.clone(),
                    weapon_type,
                    position,
                    aim_angle,
                });
            }
        }
    }

    // Advance bullets and resolve their earliest wall, player, or bullet contact.
    pub(super) fn move_bullets(&mut self) {
        // Own the collision inputs before mutating positions or despawning entities.
        let colliders: Vec<_> = self
            .world
            .query_mut::<(
                hecs::Entity,
                &Position,
                &Collider,
                Option<&PlayerId>,
                Option<&Velocity>,
                Option<&PreviousPosition>,
                Option<&crate::train::Train>,
            )>()
            .into_iter()
            .filter(|(_, _, _, player, _, _, train)| {
                train.is_none()
                    && player.is_none_or(|id| self.players.get(id).is_some_and(|p| p.is_active()))
            })
            .map(
                |(entity, position, radius, player, velocity, previous, _)| {
                    let start = previous.map_or(*position, |p| p.0);
                    let delta = if let Some(v) = velocity {
                        Position {
                            x: v.x * TICK_DURATION,
                            y: v.y * TICK_DURATION,
                        }
                    } else {
                        Position {
                            x: position.x - start.x,
                            y: position.y - start.y,
                        }
                    };
                    (entity, start, radius.0, player.copied(), delta, *position)
                },
            )
            .collect();
        let mut contacts = Vec::new();
        for (entity, position, velocity, bullet, lifetime) in self.world.query_mut::<(
            hecs::Entity,
            &mut Position,
            &Velocity,
            &Bullet,
            &mut Lifetime,
        )>() {
            let start = *position;
            let end = Position {
                x: start.x + velocity.x * TICK_DURATION,
                y: start.y + velocity.y * TICK_DURATION,
            };
            *position = end;
            if let Some(t) = crate::terrain::segment_hit_fraction(start, end, BULLET_RADIUS) {
                contacts.push((t, entity, None, None));
            }
            if lifetime.0 <= TICK_DURATION {
                contacts.push((
                    (lifetime.0 / TICK_DURATION).clamp(0.0, 1.0),
                    entity,
                    None,
                    None,
                ));
            }
            lifetime.0 -= TICK_DURATION;
            for (other, center, radius, player, delta, current) in &colliders {
                if *other == entity || *player == Some(bullet.owner) {
                    continue;
                }
                // Process each bullet pair once; both bullets disappear at contact.
                if player.is_none() && entity.to_bits().get() > other.to_bits().get() {
                    continue;
                }
                // Sweep in relative space so fast bullets cannot skip a moving player.
                let relative_start = Position {
                    x: start.x - center.x,
                    y: start.y - center.y,
                };
                let relative_end = Position {
                    x: end.x - center.x - delta.x,
                    y: end.y - center.y - delta.y,
                };
                if let Some(t) = crate::collision::circle_time(
                    relative_start,
                    relative_end,
                    Position { x: 0.0, y: 0.0 },
                    BULLET_RADIUS + radius,
                ) {
                    let damage =
                        player.map(|victim| (bullet.owner, victim, bullet.weapon_type, *current));
                    contacts.push((t, entity, Some((*other, player.is_none())), damage));
                }
            }
        }
        // Resolve globally in time order. A destroyed bullet cannot block another later hit.
        contacts.sort_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then_with(|| a.2.is_some().cmp(&b.2.is_some()))
                .then_with(|| a.1.to_bits().cmp(&b.1.to_bits()))
                .then_with(|| a.2.map(|v| v.0.to_bits()).cmp(&b.2.map(|v| v.0.to_bits())))
        });
        let mut removed = HashSet::new();
        let mut hits = Vec::new();
        for (_, entity, target, damage) in contacts {
            if removed.contains(&entity)
                || target.is_some_and(|(other, is_bullet)| is_bullet && removed.contains(&other))
            {
                continue;
            }
            removed.insert(entity);
            if let Some((other, true)) = target {
                removed.insert(other);
            }
            if let Some(hit) = damage {
                hits.push(hit);
            }
        }
        for entity in removed {
            let _ = self.world.despawn(entity);
        }
        for (killer, victim, weapon, hit_position) in hits {
            // A respawn invalidates any remaining contacts with the old body this tick.
            let still_at_hit = self
                .players
                .get(&victim)
                .and_then(|player| {
                    self.world
                        .get::<&Position>(player.entity)
                        .ok()
                        .map(|position| *position == hit_position)
                })
                .unwrap_or(false);
            if still_at_hit {
                self.damage_player(killer, victim, weapon);
            }
        }
    }

    // Apply a hit, update kills/deaths, and immediately respawn a dead participant.
    pub(super) fn damage_player(&mut self, killer: PlayerId, victim: PlayerId, weapon: WeaponType) {
        // Health, scores, and respawning are authoritative for humans and bots alike.
        let damage = crate::constants::weapon_damage(weapon);
        let Some(player) = self.players.get(&victim) else {
            return;
        };
        let entity = player.entity;
        let mut remaining_health = MAX_HEALTH;
        let died = if let Ok(mut health) = self.world.get::<&mut Health>(player.entity) {
            health.0 -= damage;
            remaining_health = health.0.max(0.0);
            if health.0 <= 0.0 {
                health.0 = MAX_HEALTH;
                true
            } else {
                false
            }
        } else {
            false
        };
        player.send_event(ServerMessage::PlayerDamaged {
            game_id: self.id.clone(),
            player_id: victim,
            damage,
            health: remaining_health,
        });
        if !died {
            return;
        }
        if let Ok(mut score) = self.world.get::<&mut Score>(entity) {
            score.deaths = score.deaths.saturating_add(1);
        }
        if let Some(player) = self.players.get(&killer)
            && let Ok(mut score) = self.world.get::<&mut Score>(player.entity)
        {
            score.kills = score.kills.saturating_add(1);
        }
        // Keep the old position for death audio, then respawn away from the last fight.
        let previous = *self.world.get::<&Position>(entity).unwrap();
        let next = loop {
            let candidate = random_spawn_position();
            if (candidate.x - previous.x).powi(2) + (candidate.y - previous.y).powi(2)
                >= 300.0_f32.powi(2)
            {
                break candidate;
            }
        };
        *self.world.get::<&mut Position>(entity).unwrap() = next;
        self.world.get::<&mut PreviousPosition>(entity).unwrap().0 = next;
        let count = self
            .world
            .get::<&RespawnCount>(entity)
            .map_or(1, |count| count.0.wrapping_add(1));
        let _ = self.world.insert_one(entity, RespawnCount(count));
        if let Ok(mut input) = self.world.get::<&mut MovementInput>(entity) {
            *input = MovementInput::default();
        }
        if let Ok(mut fire) = self.world.get::<&mut Shooting>(entity) {
            *fire = Shooting::default();
        }
        if let Ok((weapon, ammo)) = self.world.query_one_mut::<(&WeaponType, &mut Ammo)>(entity) {
            ammo.0 = crate::constants::ammo_capacity(*weapon) as f32;
        }
        if let Ok(mut bot) = self.world.get::<&mut crate::bot::Bot>(entity) {
            *bot = crate::bot::Bot::default();
        }
        let name = |id: PlayerId| {
            self.players
                .get(&id)
                .and_then(|player| {
                    self.world
                        .get::<&String>(player.entity)
                        .ok()
                        .map(|name| name.to_string())
                })
                .unwrap_or_else(|| format!("Player {id}"))
        };
        let killer_name = name(killer);
        let victim_name = name(victim);
        tracing::info!("{killer_name} killed {victim_name}");
        for player in self.players.values() {
            player.send_event(ServerMessage::PlayerKilled {
                game_id: self.id.clone(),
                killer_id: killer,
                victim_id: victim,
                killer_name: killer_name.clone(),
                victim_name: victim_name.clone(),
                weapon_type: weapon,
                position: previous,
            });
        }
    }
}
