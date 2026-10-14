use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite::Message};

use crate::{
    game::{GameHandle, GameManager, GameMessage},
    protocol::{ClientMessage, ServerMessage},
};

const ADDRESS: &str = "0.0.0.0:8000";

mod bot;
mod codec;
mod collision;
mod components;
mod constants;
mod game;
mod protocol;
mod terrain;
mod train;
mod types;
mod wire;

// Build the shared Tokio runtime, then run the server until shutdown.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all();
    // Optional runtime tuning; leave Tokio defaults when no override is supplied.
    if let Ok(value) = std::env::var("RIOT_EVENT_INTERVAL") {
        builder.event_interval(value.parse::<std::num::NonZeroU32>()?.get());
    }
    builder.build()?.block_on(run())
}

// Load static data once, start async logging, and accept client connections.
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // The guard flushes the logging thread when run() exits.
    let (writer, _log_guard) = tracing_appender::non_blocking(std::io::stdout());
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(writer)
        .with_ansi(false)
        .init();
    // Validate the map and precompute routes before opening the listener.
    terrain::load();
    bot::initialize()?;
    tracing::info!("Tokio runtime started");
    let address = std::env::var("RIOT_ADDRESS").unwrap_or_else(|_| ADDRESS.to_owned());
    let listener = TcpListener::bind(&address).await?;
    tracing::info!("RIOT WebSocket server listening on {address}");
    let game_manager = Arc::new(Mutex::new(GameManager::new()));

    loop {
        // Stop accepting on Ctrl+C; runtime shutdown drops the client and game tasks.
        let accepted = tokio::select! {
            result = listener.accept() => result,
            _ = tokio::signal::ctrl_c() => { tracing::info!("Server shutting down"); break; }
        };
        let (stream, addr) = match accepted {
            Ok(connection) => connection,
            Err(error) => {
                tracing::warn!("Failed to accept a connection: {error}");
                continue;
            }
        };
        // Share only the game directory with the new connection task.
        let manager = Arc::clone(&game_manager);
        tracing::debug!("WebSocket connection from {addr}");
        tokio::spawn(async move {
            if let Err(error) = handle_client(stream, manager).await {
                tracing::warn!("Connection to {addr} ended with an error: {error}");
            }
        });
    }
    Ok(())
}

// One task owns each socket. The game talks back through channels, not socket borrows.
async fn handle_client(
    stream: TcpStream,
    game_manager: Arc<Mutex<GameManager>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Complete the HTTP upgrade before reading application frames.
    let mut websocket = accept_async(stream).await?;
    tracing::debug!("WebSocket handshake completed");
    let (sender_to_client, mut receiver_from_server) = mpsc::channel::<ServerMessage>(64);
    let mut sender_to_game: Option<GameHandle> = None;
    // A snapshot is only useful after Welcome tells the client which player is theirs.
    let mut welcomed = false;

    loop {
        tokio::select! {
            // Network input is decoded here and then queued to the owning game.
            message = websocket.next() => {
                let Some(message) = message else { break };
                match message? {
                    Message::Binary(data) => {
                        let client_message = match codec::decode_client(&data) {
                            Ok(message) => message,
                            Err(code) => {
                                send_application(&mut websocket, codec::error(code)).await?;
                                continue;
                            }
                        };
                        tracing::trace!(?client_message, "Received client message");
                        if let Err(error) = handle_client_msg(client_message, &game_manager, &mut sender_to_game, &sender_to_client).await {
                            send_application(&mut websocket, codec::error(&error)).await?;
                        }
                    }
                    // Application traffic is protobuf; WebSocket control frames remain standard.
                    Message::Text(_) => {
                        send_application(&mut websocket, codec::error("BINARY_REQUIRED")).await?;
                    },
                    Message::Ping(data) => websocket.send(Message::Pong(data)).await?,
                    Message::Close(frame) => {
                        websocket.close(frame).await?;
                        break;
                    }
                    _ => {}
                }
            }
            // watch replaces old frames rather than queueing them behind a slow client.
            result = async { sender_to_game.as_mut().unwrap().snapshots.changed().await }, if welcomed && sender_to_game.is_some() => {
                if result.is_err() { break; }
                // Drop the watch borrow before awaiting WebSocket I/O.
                let bytes = sender_to_game.as_mut().unwrap().snapshots.borrow_and_update().clone();
                if let Some(bytes) = bytes {
                    send_application(&mut websocket, bytes).await?;
                }
            }
            // mpsc preserves discrete replies and gameplay events instead of replacing them.
            Some(message) = receiver_from_server.recv() => {
                let bytes = codec::encode_server(&message);
                send_application(&mut websocket, bytes).await?;
                if matches!(message, ServerMessage::Welcome { .. }) { welcomed = true; }
            }
        }
    }
    Ok(())
}

// Wrap protobuf bytes in a binary WebSocket frame.
async fn send_application(
    websocket: &mut WebSocketStream<TcpStream>,
    bytes: bytes::Bytes,
) -> Result<(), tokio_tungstenite::tungstenite::Error> {
    websocket.send(Message::Binary(bytes)).await?;
    Ok(())
}

// Find a game once at join, then forward commands through its cached channel handle.
async fn handle_client_msg(
    client_message: ClientMessage,
    game_manager: &Arc<Mutex<GameManager>>,
    sender_to_game: &mut Option<GameHandle>,
    sender_to_client: &mpsc::Sender<ServerMessage>,
) -> Result<(), String> {
    if let ClientMessage::JoinGame {
        game_id,
        bot_settings,
        ..
    } = &client_message
    {
        if sender_to_game.is_some() {
            return Err("ALREADY_JOINED".into());
        }
        // The guard goes out of scope before sending anything asynchronously.
        let sender = {
            let mut manager = game_manager.lock().map_err(|_| "MANAGER_UNAVAILABLE")?;
            match game_id {
                None => manager.new_random_game(Arc::clone(game_manager), *bot_settings),
                Some(id) => manager.get_sender_for_gameid(id).ok_or("GAME_NOT_FOUND")?,
            }
        };
        *sender_to_game = Some(sender);
    }

    // Keep the handle after joining; normal input does not need the manager lock.
    let sender = sender_to_game.as_ref().ok_or("JOIN_GAME_FIRST")?;
    sender
        .sender
        .send(GameMessage {
            client_message,
            sender_to_client: sender_to_client.clone(),
        })
        .await
        .map_err(|_| "GAME_UNAVAILABLE".to_owned())
}
