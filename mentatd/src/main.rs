pub mod config;
pub mod pool;
pub mod protocol;
pub mod server;

use crate::config::Config;
use crate::pool::create_pool;
use crate::server::{create_router, AppState};
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = load_config()?;

    init_logging(&config.logging.level, &config.logging.format);

    info!("Starting mentatd server");
    info!("Configuration:");
    info!("  Server: {}:{}", config.server.host, config.server.port);
    info!("  Database: {}", mask_connection_string(&config.database.connection_string));
    info!("  Pool size: {}", config.database.pool_size);

    let pool = create_pool(
        &config.database.connection_string,
        config.database.pool_size,
    )
    .map_err(|e| {
        error!("Failed to create connection pool: {}", e);
        anyhow::anyhow!("Database connection failed: {}", e)
    })?;

    info!("Testing database connection...");
    let client = pool.get().await.map_err(|e| {
        error!("Failed to get database connection: {}", e);
        anyhow::anyhow!("Database connection test failed: {}", e)
    })?;

    let version: String = client
        .query_one("SELECT version()", &[])
        .await
        .map_err(|e| {
            error!("Failed to query database: {}", e);
            anyhow::anyhow!("Database query failed: {}", e)
        })?
        .get(0);

    info!("Connected to PostgreSQL: {}", version.lines().next().unwrap_or("unknown"));

    let state = AppState::new(pool, config.clone());
    let app = create_router(state);

    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid server address: {}", e))?;

    info!("Server listening on http://{}", addr);
    info!("Health check: http://{}/health", addr);
    info!("Ready to accept Datomic client connections");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn load_config() -> anyhow::Result<Config> {
    let config_path = std::env::var("MENTATD_CONFIG")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("mentatd.toml"));

    if config_path.exists() {
        info!("Loading config from {:?}", config_path);
        Config::from_file(&config_path).map_err(|e| anyhow::anyhow!("Config error: {}", e))
    } else {
        info!("No config file found, using environment variables and defaults");
        Ok(Config::from_env())
    }
}

fn init_logging(level: &str, format: &str) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false);

    match format {
        "json" => subscriber.json().init(),
        "pretty" => subscriber.pretty().init(),
        _ => subscriber.compact().init(),
    }
}

fn mask_connection_string(s: &str) -> String {
    if let Some(at_pos) = s.rfind('@') {
        if let Some(colon_pos) = s[..at_pos].rfind(':') {
            let mut masked = String::from(&s[..colon_pos + 1]);
            masked.push_str("****");
            masked.push_str(&s[at_pos..]);
            return masked;
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_connection_string() {
        let input = "postgresql://user:password@localhost:5432/db";
        let output = mask_connection_string(input);
        assert!(!output.contains("password"));
        assert!(output.contains("****"));
        assert!(output.contains("localhost"));
    }
}
