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
    #[serde(default)]
    pub circuit_breaker: CircuitBreakerConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    /// Optional API key for request authentication.
    /// When set, all requests to `/` and `/stream/query` must include
    /// an `Authorization: Bearer <key>` header matching this value.
    /// Health and metrics endpoints are exempt.
    #[serde(default)]
    pub api_key: Option<String>,
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

#[derive(Debug, Clone, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Error threshold before the circuit breaker opens. Default 50.
    #[serde(default = "default_cb_threshold")]
    pub error_threshold: u64,
    /// Time window in seconds for the error counter. Default 60.
    #[serde(default = "default_cb_window_secs")]
    pub window_secs: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            error_threshold: default_cb_threshold(),
            window_secs: default_cb_window_secs(),
        }
    }
}

fn default_cb_threshold() -> u64 {
    50
}

fn default_cb_window_secs() -> u64 {
    60
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
    100  // Increased from 10 to 100 for better concurrency (Phase 0 optimization)
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
                api_key: std::env::var("MENTATD_API_KEY").ok(),
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
            circuit_breaker: CircuitBreakerConfig {
                error_threshold: std::env::var("MENTATD_CB_THRESHOLD")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(default_cb_threshold),
                window_secs: std::env::var("MENTATD_CB_WINDOW_SECS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(default_cb_window_secs),
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
        assert_eq!(config.database.pool_size, 100);
        assert!(config.cache.enabled);
        assert_eq!(config.cache.capacity, 1000);
        assert_eq!(config.cache.ttl_secs, 300);
    }

    #[test]
    fn test_default_server_timeout() {
        let config = Config::from_env();
        assert_eq!(config.server.timeout, 30);
    }

    #[test]
    fn test_default_database_max_lifetime() {
        let config = Config::from_env();
        assert_eq!(config.database.max_lifetime_secs, 1800);
    }

    #[test]
    fn test_default_logging() {
        let config = Config::from_env();
        assert_eq!(config.logging.level, "info");
        assert_eq!(config.logging.format, "compact");
    }

    #[test]
    fn test_default_database_url() {
        let config = Config::from_env();
        // Unless DATABASE_URL is set, should use default
        if std::env::var("DATABASE_URL").is_err() {
            assert_eq!(
                config.database.connection_string,
                "postgresql://localhost/mentat"
            );
        }
    }

    #[test]
    fn test_cache_config_default() {
        let cache = CacheConfig::default();
        assert!(cache.enabled);
        assert_eq!(cache.capacity, 1000);
        assert_eq!(cache.ttl_secs, 300);
    }

    #[test]
    fn test_config_from_toml_string() {
        let toml_str = r#"
            [server]
            host = "0.0.0.0"
            port = 9090
            timeout = 60

            [database]
            connection_string = "postgresql://user:pass@host/db"
            pool_size = 20
            max_lifetime_secs = 3600

            [logging]
            level = "debug"
            format = "json"

            [cache]
            enabled = false
            capacity = 500
            ttl_secs = 120
        "#;

        let config: Config = toml::from_str(toml_str).expect("should parse");
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 9090);
        assert_eq!(config.server.timeout, 60);
        assert_eq!(
            config.database.connection_string,
            "postgresql://user:pass@host/db"
        );
        assert_eq!(config.database.pool_size, 20);
        assert_eq!(config.database.max_lifetime_secs, 3600);
        assert_eq!(config.logging.level, "debug");
        assert_eq!(config.logging.format, "json");
        assert!(!config.cache.enabled);
        assert_eq!(config.cache.capacity, 500);
        assert_eq!(config.cache.ttl_secs, 120);
    }

    #[test]
    fn test_config_from_toml_minimal() {
        // Only required fields; everything else uses defaults
        let toml_str = r#"
            [server]

            [database]
            connection_string = "postgresql://localhost/test"

            [logging]
        "#;

        let config: Config = toml::from_str(toml_str).expect("should parse");
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.database.pool_size, 100);
        // Cache should use Default impl
        assert!(config.cache.enabled);
        assert_eq!(config.cache.capacity, 1000);
    }

    #[test]
    fn test_config_from_file_nonexistent() {
        let result = Config::from_file("/nonexistent/path/config.toml");
        assert!(result.is_err());
    }

    #[test]
    fn test_config_from_invalid_toml() {
        let bad_toml = "this is not valid toml {{{{";
        let result: Result<Config, _> = toml::from_str(bad_toml);
        assert!(result.is_err());
    }
}
