pub mod cache;
pub mod config;
pub mod db_cache;
pub mod metrics;
pub mod pool;
pub mod protocol;
pub mod server;
pub mod stream;

use crate::config::Config;
use crate::pool::create_pool;
use crate::server::{create_router, AppState};
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    let config = load_config()?;

    init_logging(&config.logging.level, &config.logging.format);

    metrics::register_metrics();

    info!("Starting mentatd server");
    info!("Configuration:");
    info!("  Server: {}:{}", config.server.host, config.server.port);
    info!(
        "  Database: {}",
        mask_connection_string(&config.database.connection_string)
    );
    info!("  Pool size: {}", config.database.pool_size);
    info!(
        "  Query cache: {} (capacity: {}, TTL: {}s)",
        if config.cache.enabled {
            "enabled"
        } else {
            "disabled"
        },
        config.cache.capacity,
        config.cache.ttl_secs
    );
    info!(
        "  API key auth: {}",
        if config.server.api_key.is_some() {
            "enabled"
        } else {
            "disabled (set MENTATD_API_KEY to enable)"
        }
    );

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

    info!(
        "Connected to PostgreSQL: {}",
        version.lines().next().unwrap_or("unknown")
    );

    let state = AppState::new(pool.clone(), config.clone());

    // Spawn background task to clean up expired db snapshots
    let cleanup_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300)); // 5 minutes
        loop {
            interval.tick().await;
            cleanup_state.db_cache().cleanup_expired();
            tracing::debug!("Cleaned up expired db snapshots");
        }
    });

    // Spawn background task to update connection pool metrics (Phase 0 optimization)
    let pool_clone = pool.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5)); // Update every 5 seconds
        loop {
            interval.tick().await;
            crate::pool::update_pool_metrics(&pool_clone);
        }
    });

    let app = create_router(state);

    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid server address: {}", e))?;

    info!("Server listening on http://{}", addr);
    info!("Health check: http://{}/health", addr);
    info!("Metrics: http://{}/metrics", addr);
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
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

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
