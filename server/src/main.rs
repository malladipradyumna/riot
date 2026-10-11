use std::sync::{Arc, Mutex};

use crate::protocol::{ClientMessage, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite::Message};
use uuid::Uuid;

use crate::game::{GameManager, GameMessage};

const ADDRESS: &str = "0.0.0.0:8000";

mod codec;
mod components;
mod constants;
mod game;
mod metrics;
mod protocol;
mod terrain;
mod train;
mod types;
mod wire;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all();
    if let Ok(value) = std::env::var("RIOT_EVENT_INTERVAL") {
        builder.event_interval(value.parse::<std::num::NonZeroU32>()?.get());
    }
    builder.build()?.block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (writer, _log_guard) = tracing_appender::non_blocking(std::io::stdout());
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(writer)
        .with_ansi(false)
        .init();
    terrain::load();
    tracing::info!(
        workers = tokio::runtime::Handle::current().metrics().num_workers(),
        "Tokio runtime started"
    );
    let address = std::env::var("RIOT_ADDRESS").unwrap_or_else(|_| ADDRESS.to_owned());
    let listener = TcpListener::bind(&address).await?;
    metrics::get()
        .workers
        .set(tokio::runtime::Handle::current().metrics().num_workers() as i64);
    let metrics_address =
        std::env::var("RIOT_METRICS_ADDRESS").unwrap_or_else(|_| "127.0.0.1:9000".into());
    let metrics_listener = TcpListener::bind(&metrics_address).await?;
    let mut metrics_task =
        tokio::spawn(async move { axum::serve(metrics_listener, metrics::router()).await });
    tracing::info!("Metrics and health endpoints listening on {metrics_address}");
    tracing::info!("RIOT WebSocket server listening on {address}");
    let game_manager = Arc::new(Mutex::new(GameManager::new()));

    loop {
        let accepted = tokio::select! {
            result = listener.accept() => result,
            result = &mut metrics_task => {
                result??;
                return Err(std::io::Error::other("Metrics HTTP server stopped").into());
            },
            _ = tokio::signal::ctrl_c() => { tracing::info!("Server shutting down"); break; }
        };
        let (stream, addr) = match accepted {
            Ok(connection) => connection,
            Err(error) => {
                tracing::warn!("Failed to accept a connection: {error}");
                continue;
            }
        };
        let manager = Arc::clone(&game_manager);
        tracing::debug!("WebSocket connection from {addr}");
        tokio::spawn(async move {
            if let Err(error) = handle_client(stream, manager).await {
                metrics::get().connection_errors.inc();
                tracing::warn!("Connection to {addr} ended with an error: {error}");
            }
        });
    }
    metrics_task.abort();
    Ok(())
}

async fn handle_client(
    stream: TcpStream,
    game_manager: Arc<Mutex<GameManager>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut websocket = accept_async(stream).await?;
    let _connection_metric = metrics::get().connection();
    tracing::debug!("WebSocket handshake completed");
    let (sender_to_client, mut receiver_from_server) = mpsc::channel::<ServerMessage>(64);
    let mut sender_to_game: Option<mpsc::Sender<GameMessage>> = None;

    loop {
        tokio::select! {
            message = websocket.next() => {
                let Some(message) = message else { break };
                match message? {
                    Message::Binary(data) => {
                        metrics::get().messages_received.inc();
                        metrics::get().bytes_received.inc_by(data.len() as u64);
                        let client_message = match codec::decode_client(&data) {
                            Ok(message) => message,
                            Err(code) => {
                                metrics::get().protocol_errors.inc();
                                send_application(&mut websocket, codec::error(code)).await?;
                                continue;
                            }
                        };
                        tracing::trace!(?client_message, "Received client message");
                        if let Err(error) = handle_client_msg(client_message, &game_manager, &mut sender_to_game, &sender_to_client).await {
                            metrics::get().protocol_errors.inc();
                            send_application(&mut websocket, codec::error(&error)).await?;
                        }
                    }
                    Message::Text(text) => {
                        metrics::get().messages_received.inc();
                        metrics::get().bytes_received.inc_by(text.len() as u64);
                        metrics::get().protocol_errors.inc();
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
            Some(message) = receiver_from_server.recv() => {
                let bytes = codec::encode_server(&message);
                send_application(&mut websocket, bytes).await?;
                if matches!(message, ServerMessage::StateSnapshot { .. }) {
                    metrics::get().snapshot_deliveries.inc();
                }
            }
        }
    }
    Ok(())
}

async fn send_application(
    websocket: &mut WebSocketStream<TcpStream>,
    bytes: bytes::Bytes,
) -> Result<(), tokio_tungstenite::tungstenite::Error> {
    let length = bytes.len();
    websocket.send(Message::Binary(bytes)).await?;
    metrics::get().messages_sent.inc();
    metrics::get().bytes_sent.inc_by(length as u64);
    Ok(())
}

async fn handle_client_msg(
    client_message: ClientMessage,
    game_manager: &Arc<Mutex<GameManager>>,
    sender_to_game: &mut Option<mpsc::Sender<GameMessage>>,
    sender_to_client: &mpsc::Sender<ServerMessage>,
) -> Result<(), String> {
    if let ClientMessage::JoinGame { game_id, .. } = &client_message {
        if sender_to_game.is_some() {
            return Err("ALREADY_JOINED".into());
        }
        // The guard goes out of scope before sending anything asynchronously.
        let sender = {
            let mut manager = game_manager.lock().map_err(|_| "MANAGER_UNAVAILABLE")?;
            match game_id {
                None => manager.new_game(Uuid::new_v4().to_string(), Arc::clone(game_manager)),
                Some(id) => manager.get_sender_for_gameid(id).ok_or("GAME_NOT_FOUND")?,
            }
        };
        *sender_to_game = Some(sender);
    }

    let sender = sender_to_game.as_ref().ok_or("JOIN_GAME_FIRST")?;
    sender
        .send(GameMessage {
            client_message,
            sender_to_client: sender_to_client.clone(),
        })
        .await
        .map_err(|_| "GAME_UNAVAILABLE".to_owned())
}
