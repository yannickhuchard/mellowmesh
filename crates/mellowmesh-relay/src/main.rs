use clap::Parser;
use mellowmesh_relay::{create_router, RelayState};
use std::net::SocketAddr;
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
    let app = create_router(RelayState::default());

    tracing::info!("MellowMesh relay listening on {}", addr);
    tracing::warn!(
        "Traffic is plain HTTP — terminate TLS in front of this relay before exposing it to the internet"
    );
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
