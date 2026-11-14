use clap::Parser;
use mellowmesh_relay::{create_router, RelayState};
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(
    name = "mellowmesh-relay",
    version,
    about = "MellowMesh relay: reach your local hub from anywhere, no port forwarding"
)]
struct Args {
    /// Port to listen on.
    #[arg(short, long, default_value_t = 9443)]
    port: u16,

    /// Bind address. Defaults to all interfaces — a relay is meant to be
    /// reachable. Put it behind TLS (reverse proxy) in production.
    #[arg(long, default_value = "0.0.0.0")]
    bind: String,

    /// JSON file that persists hub-id → link-key bindings across restarts.
    /// Without it, bindings live in memory only and any hub id can be
    /// claimed afresh after the relay restarts. Falls back to the
    /// MELLOWMESH_RELAY_STATE environment variable.
    #[arg(long)]
    state_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();
    let addr: SocketAddr = format!("{}:{}", args.bind, args.port).parse()?;

    let state_file = args
        .state_file
        .or_else(|| std::env::var_os("MELLOWMESH_RELAY_STATE").map(PathBuf::from))
        .filter(|p| !p.as_os_str().is_empty());
    let state = match state_file {
        Some(path) => {
            let state = RelayState::with_state_file(path.clone())?;
            tracing::info!(
                "Loaded {} hub binding(s) from {}",
                state.known_hub_count(),
                path.display()
            );
            state
        }
        None => {
            tracing::warn!(
                "No --state-file / MELLOWMESH_RELAY_STATE set: hub bindings are kept in memory \
                 only and are lost when the relay restarts"
            );
            RelayState::default()
        }
    };
    let app = create_router(state);

    tracing::info!("MellowMesh relay listening on {}", addr);
    tracing::warn!(
        "Traffic is plain HTTP — terminate TLS in front of this relay before exposing it to the internet"
    );
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
