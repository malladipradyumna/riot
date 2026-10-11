use std::sync::OnceLock;

use axum::{
    Router,
    http::{StatusCode, header},
    routing::get as route_get,
};
use prometheus::{Histogram, HistogramOpts, IntCounter, IntGauge, Registry, TextEncoder};

static METRICS: OnceLock<Metrics> = OnceLock::new();

pub struct Metrics {
    registry: Registry,
    pub connections: IntGauge,
    pub games: IntGauge,
    pub players: IntGauge,
    pub messages_received: IntCounter,
    pub messages_sent: IntCounter,
    pub bytes_received: IntCounter,
    pub bytes_sent: IntCounter,
    pub inputs_processed: IntCounter,
    pub protocol_errors: IntCounter,
    pub connection_errors: IntCounter,
    pub snapshots: IntCounter,
    pub snapshot_deliveries: IntCounter,
    pub simulation_duration: Histogram,
    pub simulation_delay: Histogram,
    pub snapshot_duration: Histogram,
    pub workers: IntGauge,
}

impl Metrics {
    fn new() -> Self {
        let registry = Registry::new();
        let gauge = |name, help| {
            let metric = IntGauge::new(name, help).unwrap();
            registry.register(Box::new(metric.clone())).unwrap();
            metric
        };
        let counter = |name, help| {
            let metric = IntCounter::new(name, help).unwrap();
            registry.register(Box::new(metric.clone())).unwrap();
            metric
        };
        let histogram = |name, help| {
            let metric = Histogram::with_opts(HistogramOpts::new(name, help).buckets(vec![
                0.0001, 0.0005, 0.001, 0.005, 0.01, 0.0333, 0.0667, 0.1, 0.25, 0.5, 1.0,
            ]))
            .unwrap();
            registry.register(Box::new(metric.clone())).unwrap();
            metric
        };
        #[cfg(target_os = "linux")]
        registry
            .register(Box::new(
                prometheus::process_collector::ProcessCollector::for_self(),
            ))
            .unwrap();
        Self {
            connections: gauge("riot_connections", "Open WebSocket connections"),
            games: gauge(
                "riot_games",
                "Running game tasks, including empty games awaiting expiry",
            ),
            players: gauge("riot_players", "Players registered in running game tasks"),
            messages_received: counter(
                "riot_messages_received_total",
                "Received application WebSocket messages, including invalid messages",
            ),
            messages_sent: counter(
                "riot_messages_sent_total",
                "Application messages successfully written to a WebSocket",
            ),
            bytes_received: counter(
                "riot_bytes_received_total",
                "Received application payload bytes, excluding protocol overhead",
            ),
            bytes_sent: counter(
                "riot_bytes_sent_total",
                "Application payload bytes successfully written, excluding protocol overhead",
            ),
            inputs_processed: counter(
                "riot_inputs_processed_total",
                "Authorized player inputs applied to game components",
            ),
            protocol_errors: counter(
                "riot_protocol_errors_total",
                "Invalid application messages or rejected requests",
            ),
            connection_errors: counter(
                "riot_connection_errors_total",
                "Failed handshakes or WebSocket connections",
            ),
            snapshots: counter(
                "riot_snapshots_total",
                "Shared snapshots built, including join and exit updates",
            ),
            snapshot_deliveries: counter(
                "riot_snapshot_deliveries_total",
                "Snapshots successfully written to client WebSockets",
            ),
            simulation_duration: histogram(
                "riot_simulation_duration_seconds",
                "Simulation tick execution duration; count is completed ticks",
            ),
            simulation_delay: histogram(
                "riot_simulation_delay_seconds",
                "Delay between scheduled simulation tick and execution",
            ),
            snapshot_duration: histogram(
                "riot_snapshot_duration_seconds",
                "Snapshot build, encoding, and latest-frame watch publication duration",
            ),
            workers: gauge("riot_tokio_workers", "Configured Tokio worker threads"),
            registry,
        }
    }

    pub fn connection(&self) -> ConnectionGuard {
        self.connections.inc();
        ConnectionGuard(self.connections.clone())
    }

    pub fn game(&self) -> GameGuard {
        self.games.inc();
        GameGuard {
            players: 0,
            players_gauge: self.players.clone(),
            games_gauge: self.games.clone(),
        }
    }

    fn render(&self) -> String {
        TextEncoder::new()
            .encode_to_string(&self.registry.gather())
            .unwrap()
    }
}

pub fn get() -> &'static Metrics {
    METRICS.get_or_init(Metrics::new)
}

pub struct ConnectionGuard(IntGauge);
impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.dec();
    }
}

pub struct GameGuard {
    players: i64,
    players_gauge: IntGauge,
    games_gauge: IntGauge,
}
impl GameGuard {
    pub fn update_players(&mut self, players: usize) {
        let next = players as i64;
        self.players_gauge.add(next - self.players);
        self.players = next;
    }
}
impl Drop for GameGuard {
    fn drop(&mut self) {
        self.players_gauge.sub(self.players);
        self.games_gauge.dec();
    }
}

pub fn router() -> Router {
    Router::new()
        .route("/healthz", route_get(|| async { "ok\n" }))
        .route(
            "/metrics",
            route_get(|| async {
                (
                    StatusCode::OK,
                    [(
                        header::CONTENT_TYPE,
                        "text/plain; version=0.0.4; charset=utf-8",
                    )],
                    get().render(),
                )
            }),
        )
}
