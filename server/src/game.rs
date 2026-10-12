use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    constants::{
        BULLET_LIFETIME, BULLET_RADIUS, BULLET_SPEED_MULTIPLIER, FIRE_INTERVAL, MAP_HEIGHT,
        MAP_WIDTH, MAX_HEALTH, SNAPSHOT_RATE, TICK_DURATION,
    },
    protocol::{ClientMessage, ServerMessage},
    types::{CharacterType, EntityId, GameId, Object, ObjectState, PlayerId, Position, WeaponType},
};
use tokio::{
    sync::{mpsc, watch},
    time,
};
use uuid::Uuid;

use crate::components::{
    AimAngle, Bullet, Collider, Health, Lifetime, MovementInput, ObjectId, PLAYER_RADIUS, Score,
    Shooting, Velocity,
};

// The manager is a directory of games; each game task owns its own world.
pub struct GameManager {
    game_senders: HashMap<GameId, GameHandle>,
}

impl GameManager {
    pub fn new() -> Self {
        Self {
            game_senders: HashMap::new(),
        }
    }

    pub fn new_game(&mut self, game_id: GameId, manager: Arc<Mutex<GameManager>>) -> GameHandle {
        let (sender, receiver) = mpsc::channel(64);
        let (snapshot_sender, snapshots) = watch::channel(None);
        let game = Game {
            id: game_id.clone(),
            receiver,
            snapshot_sender,
            last_snapshot_timestamp: 0,
            world: hecs::World::new(),
            players: HashMap::new(),
            next_player_id: 1,
            next_entity_id: 1,
            round_number: 1,
            intermission: false,
            round_deadline: time::Instant::now()
                + Duration::from_secs(crate::constants::ROUND_SECONDS),
        };
        let handle = GameHandle { sender, snapshots };
        self.game_senders.insert(game_id, handle.clone());
        tokio::spawn(game.run(manager));
        handle
    }

    pub fn get_sender_for_gameid(&self, game_id: &str) -> Option<GameHandle> {
        self.game_senders.get(game_id).cloned()
    }

    pub fn remove_game(&mut self, game_id: &str) {
        self.game_senders.remove(game_id);
    }
}

// Only run() and the methods it calls touch this state, so the world needs no mutex.
struct Game {
    id: GameId,
    receiver: mpsc::Receiver<GameMessage>,
    snapshot_sender: watch::Sender<Option<bytes::Bytes>>,
    last_snapshot_timestamp: u64,
    world: hecs::World,
    players: HashMap<PlayerId, Player>,
    next_player_id: PlayerId,
    next_entity_id: EntityId,
    round_number: u32,
    intermission: bool,
    round_deadline: time::Instant,
}

impl Game {
    async fn run(mut self, manager: Arc<Mutex<GameManager>>) {
        crate::train::spawn(&mut self.world, self.next_entity_id);
        self.next_entity_id += crate::constants::TRAIN_CARRIAGES;
        for _ in 0..crate::constants::INITIAL_WEAPON_PICKUPS {
            self.spawn_weapon_pickup();
        }
        tracing::info!("Created game {}", self.id);
        let mut empty_since = Some(time::Instant::now());
        let mut timer = time::interval_at(
            time::Instant::now() + Duration::from_secs(10),
            Duration::from_secs(10),
        );
        let mut simulation = time::interval(Duration::from_secs_f32(TICK_DURATION));
        // Skip missed ticks rather than piling up simulation work after a stall.
        simulation.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        let mut snapshots = time::interval(Duration::from_secs_f64(1.0 / SNAPSHOT_RATE as f64));
        snapshots.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        let mut pickups = time::interval(Duration::from_secs_f64(
            crate::constants::WEAPON_SPAWN_SECONDS,
        ));
        pickups.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        loop {
            if time::Instant::now() >= self.round_deadline {
                self.advance_round();
                if !self.intermission {
                    simulation.reset();
                    pickups.reset();
                }
            }
            tokio::select! {
                _ = time::sleep_until(self.round_deadline) => {},
                message = self.receiver.recv() => {
                    let Some(message) = message else { break };
                    self.handle_message(message).await;
                    if !self.players.is_empty() {
                        empty_since = None;
                    }
                }
                _ = simulation.tick(), if !self.intermission => {
                    // Capture positions before both train transport and walking for bullet momentum.
                    for (_, position, previous) in self.world.query_mut::<(&PlayerId, &Position, &mut crate::components::PreviousPosition)>() { previous.0 = *position; }
                    if let Some(position) = crate::train::tick(&mut self.world) {
                        for player in self.players.values() {
                            let _ = player.sender_for_client.try_send(ServerMessage::TrainWhistle { game_id: self.id.clone(), position });
                        }
                    }
                    self.move_players();
                    self.update_weapon_pickups();
                    crate::train::replenish(&mut self.world);
                    self.fire_bullets();
                    self.move_bullets();
                },
                _ = snapshots.tick() => self.send_player_snapshots(),
                _ = pickups.tick(), if !self.intermission => self.spawn_weapon_pickup(),
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
            self.send_player_snapshots();
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
            sprinting,
            ..
        } = &message.client_message
        {
            if self.intermission || game_id != &self.id || !aim_angle.is_finite() {
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
                    sprinting: *sprinting,
                };
                aim.0 = *aim_angle;
                fire.pending_shot |= *shooting && !fire.held;
                fire.held = *shooting;
                player.last_processed_request_id = client_request_id.clone();
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
            let weapon = WeaponType::Pistol;
            let entity = self.world.spawn((
                player_id,
                username.clone(),
                character_type,
                position,
                crate::components::PreviousPosition(position),
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
            self.send_player_snapshots();
        }
    }

    // Keep connections and weapons across rounds, but start scores and health afresh.
    fn advance_round(&mut self) {
        self.intermission = !self.intermission;
        self.round_deadline = time::Instant::now()
            + Duration::from_secs(if self.intermission {
                crate::constants::ROUND_BREAK_SECONDS
            } else {
                crate::constants::ROUND_SECONDS
            });
        for (input, shooting) in self
            .world
            .query_mut::<(&mut MovementInput, &mut Shooting)>()
        {
            *input = MovementInput::default();
            *shooting = Shooting::default();
        }
        if !self.intermission {
            self.round_number = self.round_number.saturating_add(1);
            let transient: Vec<_> = self
                .world
                .query::<(
                    hecs::Entity,
                    Option<&Bullet>,
                    Option<&crate::components::WeaponPickup>,
                )>()
                .iter()
                .filter_map(|(entity, bullet, pickup)| {
                    (bullet.is_some() || pickup.is_some()).then_some(entity)
                })
                .collect();
            for entity in transient {
                let _ = self.world.despawn(entity);
            }
            for (position, previous, health, score, weapon, ammo, respawn) in
                self.world.query_mut::<(
                    &mut Position,
                    &mut crate::components::PreviousPosition,
                    &mut Health,
                    &mut Score,
                    &WeaponType,
                    &mut crate::components::Ammo,
                    Option<&mut crate::components::RespawnCount>,
                )>()
            {
                *position = random_spawn_position();
                previous.0 = *position;
                health.0 = MAX_HEALTH;
                *score = Score::default();
                ammo.0 = crate::constants::ammo_capacity(*weapon) as f32;
                if let Some(respawn) = respawn {
                    respawn.0 = respawn.0.wrapping_add(1);
                }
            }
            for _ in 0..crate::constants::INITIAL_WEAPON_PICKUPS {
                self.spawn_weapon_pickup();
            }
        }
        self.send_player_snapshots();
    }

    fn send_player_snapshots(&mut self) {
        // Build one owned snapshot; the watch channel retains only the latest encoded frame.
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
        for (id, position, pickup) in
            self.world
                .query_mut::<(&ObjectId, &Position, &crate::components::WeaponPickup)>()
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
        let timestamp_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let timestamp_millis = timestamp_millis.max(self.last_snapshot_timestamp + 1);
        self.last_snapshot_timestamp = timestamp_millis;
        let acknowledgements = self
            .players
            .iter()
            .map(|(id, player)| crate::wire::InputAck {
                player_id: *id,
                last_processed_client_request_id: player.last_processed_request_id.clone(),
            })
            .collect();
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
            let distance = input.speed(*position) * TICK_DURATION;
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
        for (owner, position, previous, aim, weapon, shooting, ammo, input) in
            self.world.query_mut::<(
                &PlayerId,
                &Position,
                &crate::components::PreviousPosition,
                &AimAngle,
                &WeaponType,
                &mut Shooting,
                &mut crate::components::Ammo,
                &MovementInput,
            )>()
        {
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
            let muzzle = Position {
                x: position.x + aim.0.cos() * offset,
                y: position.y + aim.0.sin() * offset,
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
                aim.0,
                muzzle,
                Velocity {
                    // Actual tick displacement includes train transport and walking,
                    // and excludes movement blocked by walls.
                    x: aim.0.cos() * bullet_speed + (position.x - previous.0.x) / TICK_DURATION,
                    y: aim.0.sin() * bullet_speed + (position.y - previous.0.y) / TICK_DURATION,
                },
            ));
        }
        // Finish borrowing player components before spawning new entities.
        for (owner, weapon_type, aim_angle, position, velocity) in shots {
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
        // Own the collision inputs before mutating positions or despawning entities.
        let colliders: Vec<_> = self
            .world
            .query_mut::<(
                hecs::Entity,
                &Position,
                &Collider,
                Option<&PlayerId>,
                Option<&Velocity>,
                Option<&crate::components::PreviousPosition>,
                Option<&crate::train::Train>,
            )>()
            .into_iter()
            .filter(|(_, _, _, player, _, _, train)| {
                train.is_none()
                    && player.is_none_or(|id| {
                        self.players
                            .get(id)
                            .is_some_and(|p| !p.sender_for_client.is_closed())
                    })
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
        let mut removed = std::collections::HashSet::new();
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

    fn damage_player(&mut self, killer: PlayerId, victim: PlayerId, weapon: WeaponType) {
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
        let _ = player
            .sender_for_client
            .try_send(ServerMessage::PlayerDamaged {
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
        self.world
            .get::<&mut crate::components::PreviousPosition>(entity)
            .unwrap()
            .0 = next;
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
                    position: previous,
                });
        }
    }
}

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

// PlayerId looks up this record; entity points to the player's ECS components.
struct Player {
    entity: hecs::Entity,
    last_processed_request_id: String,
    sender_for_client: mpsc::Sender<ServerMessage>,
}

pub struct GameMessage {
    pub client_message: ClientMessage,
    pub sender_to_client: mpsc::Sender<ServerMessage>,
}

// Commands queue in mpsc. Snapshots use watch so slow clients get the latest frame.
#[derive(Clone)]
pub struct GameHandle {
    pub sender: mpsc::Sender<GameMessage>,
    pub snapshots: watch::Receiver<Option<bytes::Bytes>>,
}

impl Game {
    fn spawn_weapon_pickup(&mut self) {
        if self
            .world
            .query::<&crate::components::WeaponPickup>()
            .iter()
            .count()
            >= crate::constants::MAX_WEAPON_PICKUPS
        {
            return;
        }
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
                .query::<(
                    &Position,
                    Option<&PlayerId>,
                    Option<&crate::components::WeaponPickup>,
                )>()
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
                crate::components::WeaponPickup {
                    weapon_type: random_weapon(),
                    expires_at: time::Instant::now()
                        + Duration::from_secs(crate::constants::WEAPON_LIFETIME_SECONDS),
                    expires_at_millis: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64
                        + crate::constants::WEAPON_LIFETIME_SECONDS * 1000,
                },
            ));
            break;
        }
    }

    fn update_weapon_pickups(&mut self) {
        let now = time::Instant::now();
        let pickups: Vec<_> = self
            .world
            .query_mut::<(hecs::Entity, &Position, &crate::components::WeaponPickup)>()
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
            let winner = self
                .world
                .query::<(
                    hecs::Entity,
                    &PlayerId,
                    &Position,
                    &crate::components::PreviousPosition,
                )>()
                .iter()
                .filter(|(_, id, current, previous)| {
                    self.players
                        .get(id)
                        .is_some_and(|p| !p.sender_for_client.is_closed())
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
                if let Ok((equipped, ammo, shooting)) = self.world.query_one_mut::<(
                    &mut WeaponType,
                    &mut crate::components::Ammo,
                    &mut Shooting,
                )>(player)
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
