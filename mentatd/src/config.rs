use serde::Deserialize;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to parse config file: {0}")]
    Parse(#[from] toml::de::Error),
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub logging: LoggingConfig,
    #[serde(default)]
    pub cache: CacheConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub connection_string: String,
    #[serde(default = "default_pool_size")]
    pub pool_size: usize,
    #[serde(default = "default_max_lifetime")]
    pub max_lifetime_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_log_format")]
    pub format: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "default_cache_enabled")]
    pub enabled: bool,
    #[serde(default = "default_cache_capacity")]
    pub capacity: usize,
    #[serde(default = "default_cache_ttl_secs")]
    pub ttl_secs: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: default_cache_enabled(),
            capacity: default_cache_capacity(),
            ttl_secs: default_cache_ttl_secs(),
        }
    }
}

fn default_cache_enabled() -> bool {
    true
}

fn default_cache_capacity() -> usize {
    1000
}

fn default_cache_ttl_secs() -> u64 {
    300
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8080
}

fn default_timeout() -> u64 {
    30
}

fn default_pool_size() -> usize {
    10
}

fn default_max_lifetime() -> u64 {
    1800
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "compact".to_string()
}

impl Config {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path)?;
        let config = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn from_env() -> Self {
        Self {
            server: ServerConfig {
                host: std::env::var("MENTATD_HOST").unwrap_or_else(|_| default_host()),
                port: std::env::var("MENTATD_PORT")
                    .ok()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or_else(default_port),
                timeout: std::env::var("MENTATD_TIMEOUT")
                    .ok()
                    .and_then(|t| t.parse().ok())
                    .unwrap_or_else(default_timeout),
            },
            database: DatabaseConfig {
                connection_string: std::env::var("DATABASE_URL")
                    .unwrap_or_else(|_| "postgresql://localhost/mentat".to_string()),
                pool_size: std::env::var("DATABASE_POOL_SIZE")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(default_pool_size),
                max_lifetime_secs: std::env::var("DATABASE_MAX_LIFETIME")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(default_max_lifetime),
            },
            logging: LoggingConfig {
                level: std::env::var("RUST_LOG").unwrap_or_else(|_| default_log_level()),
                format: std::env::var("LOG_FORMAT").unwrap_or_else(|_| default_log_format()),
            },
            cache: CacheConfig {
                enabled: std::env::var("MENTATD_CACHE_ENABLED")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(default_cache_enabled),
                capacity: std::env::var("MENTATD_CACHE_CAPACITY")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(default_cache_capacity),
                ttl_secs: std::env::var("MENTATD_CACHE_TTL")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(default_cache_ttl_secs),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::from_env();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.database.pool_size, 10);
        assert!(config.cache.enabled);
        assert_eq!(config.cache.capacity, 1000);
        assert_eq!(config.cache.ttl_secs, 300);
    }
}
