use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    components::{AimAngle, Bullet, Health, MovementInput, PLAYER_RADIUS, Score, Shooting},
    constants::{MAP_HEIGHT, MAP_WIDTH, MAX_HEALTH, SNAPSHOT_RATE, TICK_DURATION},
    protocol::{ClientMessage, ServerMessage},
    types::{EntityId, GameId, PlayerId, Position, WeaponType},
};
use tokio::{
    sync::{mpsc, watch},
    time,
};
use uuid::Uuid;

// Keep the loop here; the larger ECS systems have their own files below.
mod combat;
mod snapshot;
mod spawning;
use spawning::random_spawn_position;

// The manager is a directory of games; each game task owns its own world.
pub struct GameManager {
    game_senders: HashMap<GameId, GameHandle>,
}

impl GameManager {
    // Start with an empty directory; worlds are created only when a game is requested.
    pub fn new() -> Self {
        Self {
            game_senders: HashMap::new(),
        }
    }

    // Create the channels, register the handle, and hand the world to its own task.
    pub fn new_game(
        &mut self,
        game_id: GameId,
        manager: Arc<Mutex<GameManager>>,
        bot_settings: crate::bot::Settings,
    ) -> GameHandle {
        let (sender, receiver) = mpsc::channel(64);
        let (snapshot_sender, snapshots) = watch::channel(None);
        let game = Game {
            id: game_id.clone(),
            bot_settings,
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

    // Clone channel handles so callers can send without keeping the manager locked.
    pub fn get_sender_for_gameid(&self, game_id: &str) -> Option<GameHandle> {
        self.game_senders.get(game_id).cloned()
    }

    // Forget the finished game so its ID can no longer be joined.
    pub fn remove_game(&mut self, game_id: &str) {
        self.game_senders.remove(game_id);
    }
}

// Only run() and the methods it calls touch this state, so the world needs no mutex.
struct Game {
    id: GameId,
    bot_settings: crate::bot::Settings,
    receiver: mpsc::Receiver<GameMessage>,
    snapshot_sender: watch::Sender<Option<bytes::Bytes>>,
    last_snapshot_timestamp: u64,
    // Components hold gameplay state. Player records below hold connection details.
    world: hecs::World,
    // Humans and bots both live here; bots have no client sender.
    players: HashMap<PlayerId, Player>,
    next_player_id: PlayerId,
    next_entity_id: EntityId,
    round_number: u32,
    intermission: bool,
    // A monotonic deadline is unaffected by changes to the host clock.
    round_deadline: time::Instant,
}

impl Game {
    // Own this game until it has been without connected humans for 30 seconds.
    async fn run(mut self, manager: Arc<Mutex<GameManager>>) {
        crate::train::spawn(&mut self.world, self.next_entity_id);
        self.next_entity_id += crate::constants::TRAIN_CARRIAGES;
        for _ in 0..crate::constants::INITIAL_WEAPON_PICKUPS {
            self.spawn_weapon_pickup();
        }
        self.spawn_bots();
        tracing::info!("Created game {}", self.id);
        // Bots do not keep an empty game alive. Count only connected humans.
        let mut empty_since = Some(time::Instant::now());
        let mut cleanup = time::interval_at(
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
        // Input, simulation, snapshots, and housekeeping have independent schedules.
        loop {
            if time::Instant::now() >= self.round_deadline {
                self.advance_round();
                if !self.intermission {
                    // Start the next round with fresh timers, not missed intermission ticks.
                    simulation.reset();
                    pickups.reset();
                }
            }
            tokio::select! {
                // Wake at the round boundary even if no client sends anything.
                _ = time::sleep_until(self.round_deadline) => {},
                message = self.receiver.recv() => {
                    let Some(message) = message else { break };
                    self.handle_message(message).await;
                    if self.human_count() > 0 {
                        empty_since = None;
                    }
                }
                _ = simulation.tick(), if !self.intermission => {
                    self.simulate_tick();
                },
                _ = snapshots.tick() => self.send_player_snapshots(),
                _ = pickups.tick(), if !self.intermission => self.spawn_weapon_pickup(),
                _ = cleanup.tick() => {
                    self.cleanup_players();
                    if self.human_count() == 0 {
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
        // Dropping this task drops its world. Removing the handle stops future joins.
        manager.lock().unwrap().remove_game(&self.id);
    }

    // Remove closed human connections and log the remaining participants.
    fn cleanup_players(&mut self) {
        // Remove disconnected participants and their gameplay entities.
        self.players.retain(|_, player| {
            if !player.is_active() {
                let _ = self.world.despawn(player.entity);
                false
            } else {
                true
            }
        });
        tracing::info!("Game {}: {} players", self.id, self.players.len());
        for (player_id, player) in &self.players {
            if let Ok((position, health)) = self
                .world
                .query_one_mut::<(&Position, &Health)>(player.entity)
            {
                tracing::info!(
                    "Player {player_id}: ({:.1}, {:.1}), health {:.1}",
                    position.x,
                    position.y,
                    health.0
                );
            }
        }
    }

    // Order matters: transport and input movement happen before firing and collisions.
    fn simulate_tick(&mut self) {
        // Capture positions before both train transport and walking for bullet momentum.
        for (_, position, previous) in self.world.query_mut::<(
            &PlayerId,
            &Position,
            &mut crate::components::PreviousPosition,
        )>() {
            previous.0 = *position;
        }
        if let Some(position) = crate::train::tick(&mut self.world) {
            for player in self.players.values() {
                player.send_event(ServerMessage::TrainWhistle {
                    game_id: self.id.clone(),
                    position,
                });
            }
        }
        // Copy target positions before bot queries borrow the world mutably.
        let targets: Vec<_> = self
            .players
            .iter()
            .filter(|(_, player)| {
                player.is_active() && (self.bot_settings.difficulty == 1 || player.is_human())
            })
            .filter_map(|(id, player)| {
                self.world
                    .get::<&Position>(player.entity)
                    .ok()
                    .map(|position| (*id, *position))
            })
            .collect();
        crate::bot::tick(&mut self.world, &targets, self.bot_settings);
        // Both humans and bots now have intent; apply the common gameplay systems.
        self.move_players();
        self.update_weapon_pickups();
        crate::train::replenish(&mut self.world);
        self.fire_bullets();
        self.move_bullets();
    }

    // Only connection-owned messages reach here. Bots write input components directly.
    async fn handle_message(&mut self, message: GameMessage) {
        // Exit: check the connection before removing its player and notifying viewers.
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
            if !player.belongs_to(&message.sender_to_client) {
                return;
            }
            let player = self.players.remove(player_id).unwrap();
            let _ = self.world.despawn(player.entity);
            tracing::info!("Player {player_id} left game {}", self.id);
            self.send_player_snapshots();
            return;
        }
        // Input updates intent only. The next tick decides movement and shots.
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
            if !player.belongs_to(&message.sender_to_client) {
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
                // Preserve a press/release pair that arrives between simulation ticks.
                fire.pending_shot |= *shooting && !fire.held;
                fire.held = *shooting;
                player.last_processed_request_id = client_request_id.clone();
            }
            return;
        }
        // Join: create the ECS player, then queue Welcome before publishing a snapshot.
        if let ClientMessage::JoinGame {
            username,
            character_type,
            client_request_id,
            ..
        } = message.client_message
        {
            let (player_id, entity, position) = self.spawn_player(
                username.clone(),
                character_type,
                Some(message.sender_to_client.clone()),
                client_request_id,
            );
            tracing::info!(
                "Player {username} ({player_id}) joined game {} at ({:.1}, {:.1})",
                self.id,
                position.x,
                position.y
            );
            let welcome = ServerMessage::Welcome {
                server_request_id: Uuid::new_v4().to_string(),
                timestamp_millis: timestamp_millis(),
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
        // Clear held controls at both boundaries, so input cannot carry across rounds.
        for (input, shooting) in self
            .world
            .query_mut::<(&mut MovementInput, &mut Shooting)>()
        {
            *input = MovementInput::default();
            *shooting = Shooting::default();
        }
        if !self.intermission {
            self.round_number = self.round_number.saturating_add(1);
            // Collect entity handles first; despawning while a query is borrowed is invalid.
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
                // The client uses this counter to avoid interpolating across a teleport.
                if let Some(respawn) = respawn {
                    respawn.0 = respawn.0.wrapping_add(1);
                }
            }
            for _ in 0..crate::constants::INITIAL_WEAPON_PICKUPS {
                self.spawn_weapon_pickup();
            }
        }
        for bot in self.world.query_mut::<&mut crate::bot::Bot>() {
            *bot = crate::bot::Bot::default();
        }
        self.send_player_snapshots();
    }

    // Apply the latest intent using terrain speed and circular wall collision.
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
            // Normalize diagonals so pressing two movement keys is not faster.
            let length = ((x * x + y * y) as f32).sqrt();
            let distance = input.speed(*position) * TICK_DURATION;
            let previous = *position;
            // Check axes separately to slide along walls instead of getting stuck.
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
}

// PlayerId looks up this record; entity points to the player's ECS components.
struct Player {
    entity: hecs::Entity,
    last_processed_request_id: String,
    // None means a bot, not a disconnected human.
    sender_for_client: Option<mpsc::Sender<ServerMessage>>,
}

impl Player {
    // Distinguish bot records from humans without relying on their names or sprites.
    fn is_human(&self) -> bool {
        self.sender_for_client.is_some()
    }

    // Player IDs are public, so authorize commands by the actual reply channel.
    fn belongs_to(&self, sender: &mpsc::Sender<ServerMessage>) -> bool {
        self.sender_for_client
            .as_ref()
            .is_some_and(|connection| connection.same_channel(sender))
    }

    // Bots are always active; humans are active while their reply channel is open.
    fn is_active(&self) -> bool {
        self.sender_for_client
            .as_ref()
            .is_none_or(|sender| !sender.is_closed())
    }

    // Queue a discrete event without letting a slow client stall the game.
    fn send_event(&self, message: ServerMessage) {
        if let Some(sender) = &self.sender_for_client {
            let _ = sender.try_send(message);
        }
    }
}

// The reply channel also identifies which connection sent the command.
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
    // Count connections, not bots, when deciding whether the room is empty.
    fn human_count(&self) -> usize {
        self.players
            .values()
            .filter(|player| player.is_human() && player.is_active())
            .count()
    }
}

// Wire timestamps use wall-clock milliseconds; round deadlines use monotonic Instant.
fn timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
