mod config;
mod db;
mod server;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "openkj-ticker",
    about = "OpenKJ karaoke rotation ticker for OBS",
    long_about = "Reads the OpenKJ SQLite database and serves a live rotation \
                  ticker as a web page. Connect OBS Browser Source to the /ticker URL."
)]
struct Args {
    /// Path to the config file (created with defaults if it does not exist).
    #[arg(short, long, default_value = "openkj-ticker.toml")]
    config: PathBuf,

    /// Override the OpenKJ data directory (contains openkj.sqlite and openkj2.ini).
    #[arg(long)]
    data_dir: Option<PathBuf>,

    /// Override the HTTP server port.
    #[arg(short, long)]
    port: Option<u16>,

    /// How many singers to display in the ticker (overrides config).
    #[arg(long)]
    singer_count: Option<usize>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "openkj_ticker=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();

    let mut cfg = config::Config::load_or_create(&args.config)?;

    // CLI flags override config file values.
    if let Some(data_dir) = args.data_dir {
        cfg.data_dir = Some(data_dir);
    }
    if let Some(port) = args.port {
        cfg.server.port = port;
    }
    if let Some(count) = args.singer_count {
        cfg.ticker.singer_count = count;
    }

    // Auto-discover the data directory if not already set.
    if cfg.data_dir.is_none() {
        match config::discover_data_dir() {
            Some(dir) => {
                tracing::info!("Discovered OpenKJ data directory: {}", dir.display());
                cfg.data_dir = Some(dir);
                // Persist the discovered path so the user can see and edit it.
                cfg.save(&args.config)?;
            }
            None => {
                tracing::warn!(
                    "Could not find OpenKJ data directory automatically. \
                    Set data_dir in {} or pass --data-dir.",
                    args.config.display()
                );
            }
        }
    }

    server::run(cfg).await
}
