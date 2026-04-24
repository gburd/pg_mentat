use deadpool_postgres::{Config as PoolConfig, Pool};
use thiserror::Error;
use tokio_postgres::NoTls;

#[derive(Debug, Error)]
pub enum PoolError {
    #[error("Failed to create connection pool: {0}")]
    Config(String),
    #[error("Failed to get connection from pool: {0}")]
    Connection(#[from] deadpool_postgres::PoolError),
    #[error("Database query error: {0}")]
    Query(#[from] tokio_postgres::Error),
}

pub type DbPool = Pool;

pub fn create_pool(connection_string: &str, max_size: usize) -> Result<DbPool, PoolError> {
    let config = connection_string
        .parse::<tokio_postgres::Config>()
        .map_err(|e| PoolError::Config(e.to_string()))?;

    let mut pool_config = PoolConfig::new();
    pool_config.dbname = config.get_dbname().map(String::from);
    pool_config.host = config.get_hosts().first().and_then(|h| match h {
        tokio_postgres::config::Host::Tcp(hostname) => Some(hostname.clone()),
        #[cfg(unix)]
        tokio_postgres::config::Host::Unix(_) => None,
    });
    pool_config.port = config.get_ports().first().copied();
    pool_config.user = config.get_user().map(String::from);
    pool_config.password = config
        .get_password()
        .map(|p| String::from_utf8(p.to_vec()).unwrap_or_default());

    let mut manager_config = tokio_postgres::Config::new();
    if let Some(dbname) = &pool_config.dbname {
        manager_config.dbname(dbname);
    }
    if let Some(host) = &pool_config.host {
        manager_config.host(host);
    }
    if let Some(port) = pool_config.port {
        manager_config.port(port);
    }
    if let Some(user) = &pool_config.user {
        manager_config.user(user);
    }
    if let Some(password) = &pool_config.password {
        manager_config.password(password);
    }

    let manager = deadpool_postgres::Manager::new(manager_config, NoTls);

    let pool = Pool::builder(manager)
        .max_size(max_size)
        .build()
        .map_err(|e| PoolError::Config(e.to_string()))?;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_config_parsing() {
        let result = create_pool("postgresql://localhost/mentat", 5);
        assert!(result.is_ok());
    }

    #[test]
    fn test_pool_with_host_and_port() {
        let result = create_pool("postgresql://localhost:5432/mentat", 5);
        assert!(result.is_ok());
    }

    #[test]
    fn test_pool_with_user_and_password() {
        let result = create_pool("postgresql://user:pass@localhost/mentat", 5);
        assert!(result.is_ok());
    }

    #[test]
    fn test_pool_invalid_connection_string() {
        let result = create_pool("not-a-valid-connection-string", 5);
        assert!(result.is_err());
    }

    #[test]
    fn test_pool_max_size_one() {
        let result = create_pool("postgresql://localhost/mentat", 1);
        assert!(result.is_ok());
    }

    #[test]
    fn test_pool_large_max_size() {
        let result = create_pool("postgresql://localhost/mentat", 100);
        assert!(result.is_ok());
    }

    #[test]
    fn test_pool_status_fields() {
        let pool = create_pool("postgresql://localhost/mentat", 5).unwrap();
        let status = pool.status();
        assert_eq!(status.max_size, 5);
        assert_eq!(status.size, 0); // no connections yet
    }

    #[test]
    fn test_pool_error_display() {
        let err = PoolError::Config("test error".to_string());
        assert_eq!(err.to_string(), "Failed to create connection pool: test error");
    }
}
