use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    constants::{
        BULLET_LIFETIME, BULLET_RADIUS, BULLET_SPEED, FIRE_INTERVAL, MAP_HEIGHT, MAP_WIDTH,
        MAX_HEALTH, SNAPSHOT_RATE, TICK_DURATION,
    },
    protocol::{ClientMessage, ServerMessage},
    types::{CharacterType, EntityId, GameId, Object, ObjectState, PlayerId, Position, WeaponType},
};
use tokio::{sync::mpsc, time};
use uuid::Uuid;

use crate::components::{
    AimAngle, Bullet, Collider, Health, Lifetime, MovementInput, ObjectId, PLAYER_RADIUS, Score,
    Shooting, Velocity,
};

pub struct GameManager {
    game_senders: HashMap<GameId, mpsc::Sender<GameMessage>>,
}

impl GameManager {
    pub fn new() -> Self {
        Self {
            game_senders: HashMap::new(),
        }
    }

    pub fn new_game(
        &mut self,
        game_id: GameId,
        manager: Arc<Mutex<GameManager>>,
    ) -> mpsc::Sender<GameMessage> {
        let (sender, receiver) = mpsc::channel(64);
        let game = Game {
            id: game_id.clone(),
            receiver,
            world: hecs::World::new(),
            players: HashMap::new(),
            next_player_id: 1,
            next_entity_id: 1,
        };
        self.game_senders.insert(game_id, sender.clone());
        tokio::spawn(game.run(manager));
        sender
    }

    pub fn get_sender_for_gameid(&self, game_id: &str) -> Option<mpsc::Sender<GameMessage>> {
        self.game_senders.get(game_id).cloned()
    }

    pub fn remove_game(&mut self, game_id: &str) {
        self.game_senders.remove(game_id);
    }
}

struct Game {
    id: GameId,
    receiver: mpsc::Receiver<GameMessage>,
    world: hecs::World,
    players: HashMap<PlayerId, Player>,
    next_player_id: PlayerId,
    next_entity_id: EntityId,
}

impl Game {
    async fn run(mut self, manager: Arc<Mutex<GameManager>>) {
        let mut game_metrics = crate::metrics::get().game();
        crate::train::spawn(&mut self.world, self.next_entity_id);
        self.next_entity_id += crate::constants::TRAIN_CARRIAGES;
        tracing::info!("Created game {}", self.id);
        let mut empty_since = Some(time::Instant::now());
        let mut timer = time::interval_at(
            time::Instant::now() + Duration::from_secs(10),
            Duration::from_secs(10),
        );
        let mut simulation = time::interval(Duration::from_secs_f32(TICK_DURATION));
        simulation.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        let mut snapshots = time::interval(Duration::from_secs_f64(1.0 / SNAPSHOT_RATE as f64));
        snapshots.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                message = self.receiver.recv() => {
                    let Some(message) = message else { break };
                    self.handle_message(message).await;
                    if !self.players.is_empty() {
                        empty_since = None;
                    }
                }
                scheduled = simulation.tick() => {
                    crate::metrics::get().simulation_delay.observe(scheduled.elapsed().as_secs_f64());
                    let started = std::time::Instant::now();
                    crate::train::tick(&mut self.world);
                    self.move_players();
                    crate::train::replenish(&mut self.world);
                    self.fire_bullets();
                    self.move_bullets();
                    crate::metrics::get().simulation_duration.observe(started.elapsed().as_secs_f64());
                },
                _ = snapshots.tick() => self.send_player_snapshots().await,
                _ = timer.tick() => {
                    // Remove disconnected participants and their gameplay entities.
                    self.players.retain(|_, player| {
                        if player.sender_for_client.is_closed() {
                            let _ = self.world.despawn(player.entity);
                            false
                        } else {
                            true
                        }
                    });
                    tracing::info!("Game {}: {} players", self.id, self.players.len());
                    for (player_id, player) in &self.players {
                        if let Ok((position, health)) = self.world.query_one_mut::<(&Position, &Health)>(player.entity) {
                            tracing::info!("Player {player_id}: ({:.1}, {:.1}), health {:.1}", position.x, position.y, health.0);
                        }
                    }
                    if self.players.is_empty() {
                        let since = empty_since.get_or_insert_with(time::Instant::now);
                        if since.elapsed() >= Duration::from_secs(30) {
                            tracing::info!("Game {} empty for 30 seconds; shutting down", self.id);
                            break;
                        }
                    } else {
                        empty_since = None;
                    }
                }
            }
            game_metrics.update_players(self.players.len());
        }
        manager.lock().unwrap().remove_game(&self.id);
    }

    async fn handle_message(&mut self, message: GameMessage) {
        if let ClientMessage::ExitGame {
            player_id, game_id, ..
        } = &message.client_message
        {
            if game_id != &self.id {
                return;
            }
            let Some(player) = self.players.get(player_id) else {
                return;
            };
            if !player
                .sender_for_client
                .same_channel(&message.sender_to_client)
            {
                return;
            }
            let player = self.players.remove(player_id).unwrap();
            let _ = self.world.despawn(player.entity);
            tracing::info!("Player {player_id} left game {}", self.id);
            self.send_player_snapshots().await;
            return;
        }
        if let ClientMessage::PlayerInput {
            player_id,
            game_id,
            client_request_id,
            up,
            down,
            left,
            right,
            aim_angle,
            shooting,
            ..
        } = &message.client_message
        {
            if game_id != &self.id || !aim_angle.is_finite() {
                return;
            }
            let Some(player) = self.players.get_mut(player_id) else {
                return;
            };
            // A connection may only move its own player.
            if !player
                .sender_for_client
                .same_channel(&message.sender_to_client)
            {
                return;
            }
            if let Ok((input, aim, fire)) =
                self.world
                    .query_one_mut::<(&mut MovementInput, &mut AimAngle, &mut Shooting)>(
                        player.entity,
                    )
            {
                *input = MovementInput {
                    up: *up,
                    down: *down,
                    left: *left,
                    right: *right,
                };
                aim.0 = *aim_angle;
                fire.pending_shot |= *shooting && !fire.held;
                fire.held = *shooting;
                player.last_processed_request_id = client_request_id.clone();
                crate::metrics::get().inputs_processed.inc();
            }
            return;
        }
        if let ClientMessage::JoinGame {
            username,
            character_type,
            client_request_id,
            ..
        } = message.client_message
        {
            let player_id = self.next_player_id;
            self.next_player_id += 1;
            let position = random_spawn_position();
            let object_id = self.next_entity_id;
            self.next_entity_id += 1;
            let weapon = random_weapon();
            let entity = self.world.spawn((
                player_id,
                username.clone(),
                character_type,
                position,
                Health(MAX_HEALTH),
                Score::default(),
                weapon,
                crate::components::Ammo(crate::constants::ammo_capacity(weapon) as f32),
                MovementInput::default(),
                AimAngle(0.0),
                ObjectId(object_id),
                Collider(PLAYER_RADIUS),
                Shooting::default(),
            ));
            self.players.insert(
                player_id,
                Player {
                    entity,
                    last_processed_request_id: client_request_id,
                    sender_for_client: message.sender_to_client.clone(),
                },
            );
            tracing::info!(
                "Player {username} ({player_id}) joined game {} at ({:.1}, {:.1})",
                self.id,
                position.x,
                position.y
            );
            let welcome = ServerMessage::Welcome {
                server_request_id: Uuid::new_v4().to_string(),
                timestamp_millis: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
                player_id,
                game_id: self.id.clone(),
            };
            if message.sender_to_client.send(welcome).await.is_err() {
                self.players.remove(&player_id);
                let _ = self.world.despawn(entity);
                return;
            }
            self.send_player_snapshots().await;
        }
    }

    async fn send_player_snapshots(&mut self) {
        let snapshot_timer = crate::metrics::get().snapshot_duration.start_timer();
        // Copy component data into owned objects before awaiting any sends.
        let platforms = crate::train::platforms(&self.world);
        let mut objects = Vec::with_capacity(self.players.len());
        for (player_id, player) in &self.players {
            if player.sender_for_client.is_closed() {
                continue;
            }
            let respawn_count = self
                .world
                .get::<&crate::components::RespawnCount>(player.entity)
                .map_or(0, |count| count.0);
            let ammo = self
                .world
                .get::<&crate::components::Ammo>(player.entity)
                .map_or(0, |ammo| ammo.0.floor() as u32);
            if let Ok((username, character, position, health, score, aim, weapon, object_id)) =
                self.world.query_one_mut::<(
                    &String,
                    &CharacterType,
                    &Position,
                    &Health,
                    &Score,
                    &AimAngle,
                    &WeaponType,
                    &ObjectId,
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
                        speed: crate::terrain::speed(*position),
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
        objects.sort_by_key(|object| object.entity_id);
        let timestamp_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let acknowledgements = self
            .players
            .iter()
            .map(|(id, player)| crate::wire::InputAck {
                player_id: *id,
                last_processed_client_request_id: player.last_processed_request_id.clone(),
            })
            .collect();
        let encoded =
            crate::codec::encode_snapshot(&self.id, timestamp_millis, &objects, acknowledgements);
        let objects = Arc::new(objects);
        crate::metrics::get().snapshots.inc();
        for (player_id, player) in &self.players {
            let snapshot = ServerMessage::StateSnapshot {
                server_request_id: Uuid::new_v4().to_string(),
                timestamp_millis,
                player_id: *player_id,
                game_id: self.id.clone(),
                last_processed_client_request_id: player.last_processed_request_id.clone(),
                objects: Arc::clone(&objects),
                encoded: encoded.clone(),
            };
            let _ = player.sender_for_client.send(snapshot).await;
        }
        snapshot_timer.observe_duration();
    }

    fn move_players(&mut self) {
        for (player_id, position, input) in self
            .world
            .query_mut::<(&PlayerId, &mut Position, &MovementInput)>()
        {
            let x = i32::from(input.right) - i32::from(input.left);
            let y = i32::from(input.down) - i32::from(input.up);
            if x == 0 && y == 0 {
                continue;
            }
            let length = ((x * x + y * y) as f32).sqrt();
            let distance = crate::terrain::speed(*position) * TICK_DURATION;
            let previous = *position;
            let next_x = (position.x + x as f32 / length * distance)
                .clamp(PLAYER_RADIUS, MAP_WIDTH - PLAYER_RADIUS);
            if crate::terrain::can_stand(
                Position {
                    x: next_x,
                    y: position.y,
                },
                PLAYER_RADIUS,
            ) {
                position.x = next_x;
            }
            let next_y = (position.y + y as f32 / length * distance)
                .clamp(PLAYER_RADIUS, MAP_HEIGHT - PLAYER_RADIUS);
            if crate::terrain::can_stand(
                Position {
                    x: position.x,
                    y: next_y,
                },
                PLAYER_RADIUS,
            ) {
                position.y = next_y;
            }
            if *position != previous {
                tracing::debug!(
                    "Game {} player {player_id}: ({:.1}, {:.1})",
                    self.id,
                    position.x,
                    position.y
                );
            }
        }
    }

    fn fire_bullets(&mut self) {
        let mut shots = Vec::new();
        for (owner, position, aim, weapon, shooting, ammo) in self.world.query_mut::<(
            &PlayerId,
            &Position,
            &AimAngle,
            &WeaponType,
            &mut Shooting,
            &mut crate::components::Ammo,
        )>() {
            shooting.cooldown = (shooting.cooldown - TICK_DURATION).max(0.0);
            if (!shooting.held && !shooting.pending_shot) || shooting.cooldown > f32::EPSILON {
                continue;
            }
            let Some(player) = self.players.get(owner) else {
                continue;
            };
            if player.sender_for_client.is_closed() {
                continue;
            }
            if ammo.0 < 1.0 {
                shooting.pending_shot = false;
                continue;
            }
            ammo.0 -= 1.0;
            shooting.cooldown = FIRE_INTERVAL;
            shooting.pending_shot = false;
            let offset = PLAYER_RADIUS + BULLET_RADIUS + 1.0;
            shots.push((
                *owner,
                *weapon,
                aim.0,
                Position {
                    x: position.x + aim.0.cos() * offset,
                    y: position.y + aim.0.sin() * offset,
                },
                Velocity {
                    x: aim.0.cos() * BULLET_SPEED,
                    y: aim.0.sin() * BULLET_SPEED,
                },
            ));
        }
        // Finish borrowing player components before spawning new entities.
        for (owner, weapon_type, aim_angle, position, velocity) in shots {
            let object_id = self.next_entity_id;
            self.next_entity_id += 1;
            self.world.spawn((
                ObjectId(object_id),
                position,
                velocity,
                Bullet {
                    owner,
                    weapon_type,
                    aim_angle,
                },
                Collider(BULLET_RADIUS),
                Lifetime(BULLET_LIFETIME),
            ));
            for player in self.players.values() {
                // Audio events must not block simulation if a client is slow.
                let _ = player.sender_for_client.try_send(ServerMessage::GunFired {
                    server_request_id: Uuid::new_v4().to_string(),
                    timestamp_millis: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    player_id: owner,
                    game_id: self.id.clone(),
                    weapon_type,
                    position,
                    aim_angle,
                });
            }
        }
    }

    fn move_bullets(&mut self) {
        // Copy colliders before borrowing bullet positions mutably.
        let colliders: Vec<_> = self
            .world
            .query_mut::<(
                hecs::Entity,
                &Position,
                &Collider,
                Option<&PlayerId>,
                Option<&Velocity>,
            )>()
            .into_iter()
            .map(|(entity, position, radius, player_id, velocity)| {
                let delta = velocity.map_or(Position { x: 0.0, y: 0.0 }, |v| Position {
                    x: v.x * TICK_DURATION,
                    y: v.y * TICK_DURATION,
                });
                (entity, *position, radius.0, player_id.copied(), delta)
            })
            .collect();
        let mut removals = Vec::new();
        let mut hits = Vec::new();
        for (entity, position, velocity, bullet, lifetime) in self.world.query_mut::<(
            hecs::Entity,
            &mut Position,
            &Velocity,
            &Bullet,
            &mut Lifetime,
        )>() {
            let previous = *position;
            position.x += velocity.x * TICK_DURATION;
            position.y += velocity.y * TICK_DURATION;
            lifetime.0 -= TICK_DURATION;
            let wall_hit = crate::terrain::segment_blocked(previous, *position, BULLET_RADIUS)
                || position.x <= BULLET_RADIUS
                || position.x >= MAP_WIDTH - BULLET_RADIUS
                || position.y <= BULLET_RADIUS
                || position.y >= MAP_HEIGHT - BULLET_RADIUS
                || previous.x <= BULLET_RADIUS
                || previous.x >= MAP_WIDTH - BULLET_RADIUS
                || previous.y <= BULLET_RADIUS
                || previous.y >= MAP_HEIGHT - BULLET_RADIUS;
            let hit = colliders
                .iter()
                .filter(|(other, center, radius, player_id, delta)| {
                    if *other == entity || *player_id == Some(bullet.owner) {
                        return false;
                    }
                    // Relative segment also accounts for two bullets moving toward each other.
                    segment_hits_circle(
                        Position {
                            x: previous.x - center.x,
                            y: previous.y - center.y,
                        },
                        Position {
                            x: position.x - center.x - delta.x,
                            y: position.y - center.y - delta.y,
                        },
                        BULLET_RADIUS + radius,
                    )
                })
                .min_by(|a, b| {
                    let distance = |center: Position| {
                        (previous.x - center.x).powi(2) + (previous.y - center.y).powi(2)
                    };
                    distance(a.1).total_cmp(&distance(b.1))
                });
            if !wall_hit && lifetime.0 > 0.0 {
                if let Some((_, center, _, Some(victim), _)) = hit {
                    hits.push((bullet.owner, *victim, bullet.weapon_type, *center));
                }
            }
            if wall_hit || hit.is_some() || lifetime.0 <= 0.0 {
                removals.push(entity);
            }
        }
        for entity in removals {
            let _ = self.world.despawn(entity);
        }
        // Apply damage after the bullet query releases its world borrow.
        for (killer, victim, weapon, hit_position) in hits {
            // Hits were collected before damage was applied. A respawn invalidates
            // other hits against the victim's previous position in this tick.
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

    fn damage_player(&mut self, killer: PlayerId, victim: PlayerId, weapon: WeaponType) {
        let damage = crate::constants::weapon_damage(weapon);
        let Some(player) = self.players.get(&victim) else {
            return;
        };
        let entity = player.entity;
        let died = if let Ok(mut health) = self.world.get::<&mut Health>(player.entity) {
            health.0 -= damage;
            if health.0 <= 0.0 {
                health.0 = MAX_HEALTH;
                true
            } else {
                false
            }
        } else {
            false
        };
        if !died {
            return;
        }
        if let Ok(mut score) = self.world.get::<&mut Score>(entity) {
            score.deaths = score.deaths.saturating_add(1);
        }
        if let Some(player) = self.players.get(&killer) {
            if let Ok(mut score) = self.world.get::<&mut Score>(player.entity) {
                score.kills = score.kills.saturating_add(1);
            }
        }
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
        let count = self
            .world
            .get::<&crate::components::RespawnCount>(entity)
            .map_or(1, |count| count.0.wrapping_add(1));
        let _ = self
            .world
            .insert_one(entity, crate::components::RespawnCount(count));
        if let Ok(mut input) = self.world.get::<&mut MovementInput>(entity) {
            *input = MovementInput::default();
        }
        if let Ok(mut fire) = self.world.get::<&mut Shooting>(entity) {
            *fire = Shooting::default();
        }
        if let Ok(weapon) = self.world.get::<&WeaponType>(entity) {
            if let Ok(mut ammo) = self.world.get::<&mut crate::components::Ammo>(entity) {
                ammo.0 = crate::constants::ammo_capacity(*weapon) as f32;
            }
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
            let _ = player
                .sender_for_client
                .try_send(ServerMessage::PlayerKilled {
                    game_id: self.id.clone(),
                    killer_id: killer,
                    victim_id: victim,
                    killer_name: killer_name.clone(),
                    victim_name: victim_name.clone(),
                    weapon_type: weapon,
                });
        }
    }
}

fn segment_hits_circle(start: Position, end: Position, radius: f32) -> bool {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length_squared = dx * dx + dy * dy;
    let fraction = if length_squared > 0.0 {
        (-(start.x * dx + start.y * dy) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let x = start.x + dx * fraction;
    let y = start.y + dy * fraction;
    x * x + y * y <= radius * radius
}

fn random_weapon() -> WeaponType {
    [
        WeaponType::Pistol,
        WeaponType::Ak47,
        WeaponType::RocketLauncher,
        WeaponType::DesertEagle,
    ][(Uuid::new_v4().as_u128() % 4) as usize]
}

fn random_spawn_position() -> Position {
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

struct Player {
    entity: hecs::Entity,
    last_processed_request_id: String,
    sender_for_client: mpsc::Sender<ServerMessage>,
}

pub struct GameMessage {
    pub client_message: ClientMessage,
    pub sender_to_client: mpsc::Sender<ServerMessage>,
}
