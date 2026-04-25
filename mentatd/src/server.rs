use crate::cache::QueryCache;
use crate::config::Config;
use crate::db_cache::DbValueCache;
use crate::metrics;
use crate::pool::DbPool;
use crate::protocol::{
    parser::{parse_request, ParseError},
    serializer::serialize_response,
    transit_parser::{detect_input_format, parse_transit_json, parse_transit_msgpack, InputFormat},
    transit_serializer::{
        content_type_for_encoding, parse_accept_encoding, serialize_transit_json,
        serialize_transit_msgpack, TransitEncoding,
    },
    Anomaly, AnomalyCategory, FilterPredicate, Operation, Response, ResponseValue,
};
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response as AxumResponse},
    routing::{get, post},
    Router,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("Database pool error: {0}")]
    Pool(#[from] deadpool_postgres::PoolError),
    #[error("Database query error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Transaction conflict: serialization failure after {attempts} attempts")]
    Conflict { attempts: u32 },
    #[error("Unique constraint violation: {0}")]
    UniqueViolation(String),
    #[error("Circuit breaker open: too many errors ({error_count} in the last {window_secs}s)")]
    CircuitBreakerOpen { error_count: u64, window_secs: u64 },
}

impl From<ServerError> for Anomaly {
    fn from(err: ServerError) -> Self {
        match err {
            ServerError::Parse(e) => e.into(),
            ServerError::Pool(_) | ServerError::Database(_) => Self {
                category: AnomalyCategory::Unavailable,
                message: "Database unavailable".to_string(),
                db_error: Some("db.error/unavailable".to_string()),
            },
            ServerError::Internal(msg) => Self {
                category: AnomalyCategory::Fault,
                message: msg,
                db_error: Some("db.error/fault".to_string()),
            },
            ServerError::Conflict { attempts } => Self {
                category: AnomalyCategory::Unavailable,
                message: format!(
                    "Transaction serialization conflict after {} attempts. \
                     Concurrent transactions modified the same data. Retry the transaction.",
                    attempts
                ),
                db_error: Some("db.error/conflict".to_string()),
            },
            ServerError::UniqueViolation(msg) => Self {
                category: AnomalyCategory::Incorrect,
                message: msg,
                db_error: Some("db.error/unique-conflict".to_string()),
            },
            ServerError::CircuitBreakerOpen {
                error_count,
                window_secs,
            } => Self {
                category: AnomalyCategory::Unavailable,
                message: format!(
                    "Service temporarily unavailable: {} errors in the last {}s. \
                     The circuit breaker will reset automatically.",
                    error_count, window_secs
                ),
                db_error: Some("db.error/circuit-breaker-open".to_string()),
            },
        }
    }
}

/// Maximum number of retry attempts for serialization failures.
const MAX_TRANSACT_RETRIES: u32 = 3;

/// Base delay for exponential backoff on serialization failures (milliseconds).
const BASE_RETRY_DELAY_MS: u64 = 10;

/// Check if a `tokio_postgres::Error` is a serialization failure (SQLSTATE 40001).
fn is_serialization_failure(err: &tokio_postgres::Error) -> bool {
    if let Some(db_err) = err.as_db_error() {
        *db_err.code() == tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE
    } else {
        false
    }
}

/// Check if a `tokio_postgres::Error` is a unique constraint violation (SQLSTATE 23505).
fn is_unique_violation(err: &tokio_postgres::Error) -> bool {
    if let Some(db_err) = err.as_db_error() {
        *db_err.code() == tokio_postgres::error::SqlState::UNIQUE_VIOLATION
    } else {
        false
    }
}

/// Check if a `tokio_postgres::Error` is a deadlock (SQLSTATE 40P01).
fn is_deadlock(err: &tokio_postgres::Error) -> bool {
    if let Some(db_err) = err.as_db_error() {
        *db_err.code() == tokio_postgres::error::SqlState::T_R_DEADLOCK_DETECTED
    } else {
        false
    }
}

/// Compute retry delay with exponential backoff and jitter.
///
/// Delay = base_ms * 2^attempt * jitter, where jitter varies +/-25%.
fn retry_delay(attempt: u32) -> Duration {
    let base = BASE_RETRY_DELAY_MS * 2_u64.pow(attempt);
    // Simple jitter: vary by +/- 25% using a deterministic-ish approach
    // based on the attempt number (avoids pulling in rand for this).
    let jitter_factor = match attempt % 4 {
        0 => 0.80,
        1 => 1.10,
        2 => 0.90,
        3 => 1.20,
        _ => 1.0,
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let millis = (base as f64 * jitter_factor) as u64;
    Duration::from_millis(millis)
}

// ============================================================================
// Circuit Breaker
// ============================================================================

/// Circuit breaker that tracks database errors and rejects requests when
/// the error rate is too high. Protects the database from cascading failures
/// when it is overloaded or unresponsive.
///
/// State transitions:
///   Closed (normal) -> Open (rejecting) when errors exceed threshold in window
///   Open -> Closed when the window resets
///
/// Configurable via `[circuit_breaker]` section in config or env vars:
///   MENTATD_CB_THRESHOLD (default 50), MENTATD_CB_WINDOW_SECS (default 60)
pub struct CircuitBreaker {
    error_count: AtomicU64,
    last_reset: Mutex<Instant>,
    /// Error threshold before the circuit breaker opens.
    threshold: u64,
    /// Time window for the error counter (seconds).
    window_secs: u64,
}

impl CircuitBreaker {
    pub fn new(threshold: u64, window_secs: u64) -> Self {
        Self {
            error_count: AtomicU64::new(0),
            last_reset: Mutex::new(Instant::now()),
            threshold,
            window_secs,
        }
    }

    /// Record an error. Returns the current error count.
    pub async fn record_error(&self) -> u64 {
        self.maybe_reset().await;
        self.error_count.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Check if the circuit breaker is open (rejecting requests).
    pub async fn should_reject(&self) -> bool {
        self.maybe_reset().await;
        self.error_count.load(Ordering::Relaxed) > self.threshold
    }

    /// Return the current error count (for metrics/diagnostics).
    pub fn error_count(&self) -> u64 {
        self.error_count.load(Ordering::Relaxed)
    }

    /// Return the configured time window in seconds.
    pub fn window_secs(&self) -> u64 {
        self.window_secs
    }

    /// Reset the error counter if the time window has elapsed.
    async fn maybe_reset(&self) {
        let mut last = self.last_reset.lock().await;
        if last.elapsed() > Duration::from_secs(self.window_secs) {
            self.error_count.store(0, Ordering::Relaxed);
            *last = Instant::now();
        }
    }
}

// ============================================================================
// Application State
// ============================================================================

#[derive(Clone)]
pub struct AppState {
    pool: DbPool,
    #[allow(dead_code)]
    config: Arc<Config>,
    query_cache: Arc<QueryCache>,
    db_cache: Arc<DbValueCache>,
    circuit_breaker: Arc<CircuitBreaker>,
}

impl AppState {
    pub fn new(pool: DbPool, config: Config) -> Self {
        let cache_capacity = if config.cache.enabled {
            config.cache.capacity
        } else {
            0
        };
        let cache_ttl = Duration::from_secs(config.cache.ttl_secs);
        // TTL for database value snapshots: 5 minutes.
        // This matches Datomic's expectation that database values are short-lived
        // immutable references used within a request or batch of queries.
        let db_snapshot_ttl = Duration::from_secs(300);
        Self {
            pool,
            config: Arc::new(config),
            query_cache: Arc::new(QueryCache::new(cache_capacity, cache_ttl)),
            db_cache: Arc::new(DbValueCache::new(db_snapshot_ttl)),
            circuit_breaker: Arc::new(CircuitBreaker::new(
                config.circuit_breaker.error_threshold,
                config.circuit_breaker.window_secs,
            )),
        }
    }

    /// Returns a reference to the database connection pool.
    pub fn pool(&self) -> &DbPool {
        &self.pool
    }

    /// Returns a reference to the db value cache.
    pub fn db_cache(&self) -> &DbValueCache {
        &self.db_cache
    }
}

/// Maximum request body size (16 MiB).
///
/// Prevents denial-of-service via oversized payloads.
const MAX_BODY_SIZE: usize = 16 * 1024 * 1024;

pub fn create_router(state: AppState) -> Router {
    // Authenticated routes: require API key when configured
    let api_routes = Router::new()
        .route("/", post(handle_request))
        .route("/stream/query", post(crate::stream::handle_stream_query));

    // Public routes: health and metrics are always accessible
    let public_routes = Router::new()
        .route("/health", get(health_check))
        .route("/metrics", get(metrics_endpoint));

    let api_routes = if state.config.server.api_key.is_some() {
        api_routes.layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
    } else {
        api_routes
    };

    api_routes
        .merge(public_routes)
        .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY_SIZE))
        .with_state(state)
}

/// Authentication middleware that validates Bearer tokens against the configured API key.
///
/// When `MENTATD_API_KEY` is set, all requests to protected endpoints must include
/// an `Authorization: Bearer <key>` header. Returns 401 Unauthorized if the key
/// is missing or incorrect.
async fn auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<AxumResponse, StatusCode> {
    let expected_key = state
        .config
        .server
        .api_key
        .as_deref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if let Some(token) = auth_header.strip_prefix("Bearer ") {
        // Use constant-time comparison to prevent timing attacks
        if constant_time_eq(token.as_bytes(), expected_key.as_bytes()) {
            Ok(next.run(request).await)
        } else {
            Err(StatusCode::UNAUTHORIZED)
        }
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Constant-time byte comparison to prevent timing side-channel attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "mentatd ready")
}

async fn metrics_endpoint(State(state): State<AppState>) -> impl IntoResponse {
    // Update pool gauges before rendering
    let pool_status = state.pool.status();
    metrics::CONNECTION_POOL_SIZE.set(f64::from(pool_status.size as u32));
    metrics::CONNECTION_POOL_AVAILABLE.set(i64::from(pool_status.available as u32));
    metrics::CONNECTION_POOL_WAITING.set(i64::from(pool_status.waiting as u32));

    // Update cache gauges from dependency-tracked stats
    let cache_stats = state.query_cache.stats();
    metrics::CACHE_SIZE.set(cache_stats.size as i64);
    metrics::CACHE_TRACKED_ENTRIES.set(cache_stats.tracked_entries as i64);
    metrics::CACHE_HIT_RATE.set(cache_stats.hit_rate);
    metrics::CACHE_AVG_DEPS.set(cache_stats.avg_dependency_count);

    let body = metrics::render_metrics();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
}

async fn handle_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<AxumResponse, StatusCode> {
    let request_start = Instant::now();
    metrics::REQUEST_COUNT.inc();

    // Timing: Header processing
    let header_start = Instant::now();
    // Detect input format from Content-Type header
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/edn");
    let input_format = detect_input_format(content_type);
    let header_time = header_start.elapsed();

    // Timing: Parse request
    let parse_start = Instant::now();
    // Parse the request body using the appropriate parser.
    // SECURITY: Log only the format and size at info level; full body at debug
    // to avoid leaking transaction data (which may contain PII) into logs.
    let parse_result = match input_format {
        InputFormat::TransitJson => {
            let body_str = std::str::from_utf8(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
            info!("Received Transit+JSON request ({} bytes)", body.len());
            debug!("Transit+JSON body: {}", body_str);
            parse_transit_json(body_str)
        }
        InputFormat::TransitMsgpack => {
            info!("Received Transit+MessagePack request ({} bytes)", body.len());
            parse_transit_msgpack(&body)
        }
        InputFormat::Edn => {
            let body_str = std::str::from_utf8(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
            info!("Received EDN request ({} bytes)", body.len());
            debug!("EDN body: {}", body_str);
            parse_request(body_str)
        }
    };
    let parse_time = parse_start.elapsed();

    // Timing: Execute operation
    let execute_start = Instant::now();
    let response = match parse_result {
        Ok(request) => match execute_operation(request.op, &state).await {
            Ok(result) => Response::Success { result },
            Err(e) => {
                error!("Operation failed: {}", e);
                metrics::ERROR_COUNT.inc();
                // Record error in circuit breaker (database/pool errors indicate
                // systemic issues; parse errors and conflicts are client-side).
                if matches!(e, ServerError::Pool(_) | ServerError::Database(_) | ServerError::Internal(_)) {
                    state.circuit_breaker.record_error().await;
                }
                Response::Error { anomaly: e.into() }
            }
        },
        Err(e) => {
            error!("Parse failed: {}", e);
            metrics::ERROR_COUNT.inc();
            Response::Error { anomaly: e.into() }
        }
    };
    let execute_time = execute_start.elapsed();

    // Timing: Serialize response
    let serialize_start = Instant::now();
    // Content-type negotiation: check Accept header for Transit formats
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/edn");

    let response_result = if let Some(encoding) = parse_accept_encoding(accept) {
        let content_type = content_type_for_encoding(encoding);
        match encoding {
            TransitEncoding::Json => {
                let transit_response = serialize_transit_json(&response);
                info!("Sending Transit+JSON response");
                Ok((
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, content_type)],
                    transit_response,
                )
                    .into_response())
            }
            TransitEncoding::Msgpack => {
                let transit_response = serialize_transit_msgpack(&response);
                info!("Sending Transit+MessagePack response ({} bytes)", transit_response.len());
                Ok((
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, content_type)],
                    transit_response,
                )
                    .into_response())
            }
        }
    } else {
        // Default: EDN format
        let edn_response = serialize_response(&response);
        info!("Sending EDN response ({} bytes)", edn_response.len());
        debug!("EDN response: {}", edn_response);
        Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/edn")],
            edn_response,
        )
            .into_response())
    };
    let serialize_time = serialize_start.elapsed();

    let total_time = request_start.elapsed();

    // Log timing breakdown
    tracing::info!(
        "Request timing - total: {:?}, header: {:?}, parse: {:?}, execute: {:?}, serialize: {:?}",
        total_time, header_time, parse_time, execute_time, serialize_time
    );

    // Also log as structured data for easier analysis
    tracing::debug!(
        total_ms = total_time.as_millis() as u64,
        header_ms = header_time.as_millis() as u64,
        parse_ms = parse_time.as_millis() as u64,
        execute_ms = execute_time.as_millis() as u64,
        serialize_ms = serialize_time.as_millis() as u64,
        "Request timing breakdown"
    );

    response_result
}

async fn execute_operation(op: Operation, state: &AppState) -> Result<ResponseValue, ServerError> {
    // Circuit breaker: reject requests when error rate is too high.
    // Health checks bypass the circuit breaker so monitoring can still probe.
    if !matches!(op, Operation::Health) && state.circuit_breaker.should_reject().await {
        let count = state.circuit_breaker.error_count();
        warn!(
            error_count = count,
            window_secs = CIRCUIT_BREAKER_WINDOW_SECS,
            "Circuit breaker open: rejecting request"
        );
        return Err(ServerError::CircuitBreakerOpen {
            error_count: count,
            window_secs: CIRCUIT_BREAKER_WINDOW_SECS,
        });
    }

    let op_start = Instant::now();

    let (op_name, result) = match op {
        Operation::Health => ("health", Ok(ResponseValue::String("healthy".to_string()))),

        Operation::ListDatabases => {
            let client = state.pool.get().await?;

            let rows = client
                .query(
                    "SELECT datname FROM pg_database WHERE datistemplate = false",
                    &[],
                )
                .await?;

            let databases: Vec<ResponseValue> = rows
                .iter()
                .map(|row| ResponseValue::String(row.get::<_, String>(0)))
                .collect();

            ("list_databases", Ok(ResponseValue::Vector(databases)))
        }

        Operation::CreateDatabase { db_name } => {
            if !is_valid_db_name(&db_name) {
                return Err(ServerError::Internal("Invalid database name".to_string()));
            }

            let client = state.pool.get().await?;

            // Use quoted identifier to prevent SQL injection.
            // is_valid_db_name already restricts to [a-zA-Z][a-zA-Z0-9_]* but
            // we quote defensively as a second layer of protection.
            let quoted_name = quote_identifier(&db_name);
            client
                .execute(&format!("CREATE DATABASE {}", quoted_name), &[])
                .await?;

            ("create_database", Ok(ResponseValue::Boolean(true)))
        }

        Operation::DeleteDatabase { db_name } => {
            if !is_valid_db_name(&db_name) {
                return Err(ServerError::Internal("Invalid database name".to_string()));
            }

            let client = state.pool.get().await?;

            let quoted_name = quote_identifier(&db_name);
            client
                .execute(&format!("DROP DATABASE {}", quoted_name), &[])
                .await?;

            ("delete_database", Ok(ResponseValue::Boolean(true)))
        }

        Operation::Connect { db_name } => {
            let client = state.pool.get().await?;

            let exists = client
                .query_one(
                    "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)",
                    &[&db_name],
                )
                .await?
                .get::<_, bool>(0);

            if !exists {
                return Err(ServerError::Internal(format!(
                    "Database '{}' not found",
                    db_name
                )));
            }

            let connection_id = Uuid::new_v4().to_string();

            ("connect", Ok(ResponseValue::Map(vec![
                (
                    ResponseValue::Keyword("connection-id".to_string()),
                    ResponseValue::String(connection_id),
                ),
                (
                    ResponseValue::Keyword("db-name".to_string()),
                    ResponseValue::String(db_name),
                ),
                (
                    ResponseValue::Keyword("status".to_string()),
                    ResponseValue::String("connected".to_string()),
                ),
            ])))
        }

        Operation::Db { connection_id } => {
            info!("Creating database value for connection: {}", connection_id);

            let client = state.pool.get().await?;

            // Get current basis-t from PostgreSQL (the latest transaction id)
            let row = client
                .query_one(
                    "SELECT COALESCE(MAX(tx), 0) FROM mentat.transactions",
                    &[],
                )
                .await?;

            let basis_t: i64 = row.get(0);

            // Create an immutable snapshot in the cache, keyed by a unique db_id.
            // This snapshot captures the point-in-time basis-t so that subsequent
            // queries using this db value see a consistent view, even if new
            // transactions are committed in the meantime.
            let db_id = state.db_cache.create_snapshot(basis_t);

            info!(
                "Created db value: db_id={}, basis_t={}, connection_id={}",
                db_id, basis_t, connection_id
            );

            ("db", Ok(ResponseValue::Map(vec![
                (
                    ResponseValue::Keyword("db-id".to_string()),
                    ResponseValue::String(db_id),
                ),
                (
                    ResponseValue::Keyword("basis-t".to_string()),
                    ResponseValue::Integer(basis_t),
                ),
                (
                    ResponseValue::Keyword("db-name".to_string()),
                    ResponseValue::String("mentat".to_string()),
                ),
                (
                    ResponseValue::Keyword("t".to_string()),
                    ResponseValue::Integer(basis_t),
                ),
                (
                    ResponseValue::Keyword("next-t".to_string()),
                    ResponseValue::Integer(basis_t + 1),
                ),
            ])))
        }

        Operation::DbSnapshot => {
            info!("Creating database snapshot");

            let client = state.pool.get().await?;

            // Get current basis-t from PostgreSQL
            let row = client
                .query_one(
                    "SELECT COALESCE(MAX(tx), 0) FROM mentat.transactions",
                    &[],
                )
                .await?;

            let basis_t: i64 = row.get(0);

            // Create snapshot in cache
            let db_id = state.db_cache.create_snapshot(basis_t);

            info!("Created db snapshot: db_id={}, basis_t={}", db_id, basis_t);

            ("db_snapshot", Ok(ResponseValue::DbSnapshot { db_id, basis_t }))
        }

        Operation::Query { query, args, db_id, .. } => {
            info!("Executing query: {} with args: {:?}, db_id: {:?}", query, args, db_id);
            metrics::QUERY_COUNT.inc();
            let query_start = Instant::now();

            // Timing: DB snapshot lookup
            let snapshot_start = Instant::now();
            // If db_id is provided, get the cached basis-t
            let basis_t = if let Some(ref id) = db_id {
                match state.db_cache.get_basis_t(id) {
                    Some(t) => {
                        debug!("Using cached db snapshot: db_id={}, basis_t={}", id, t);
                        Some(t)
                    }
                    None => {
                        warn!("Invalid or expired db_id: {}", id);
                        return Err(ServerError::Internal(format!("Invalid or expired db-id: {}", id)));
                    }
                }
            } else {
                None
            };
            let snapshot_time = snapshot_start.elapsed();

            // Timing: Cache key generation
            let cache_key_start = Instant::now();
            // Convert args to a stable JSON string for cache key
            let args_json_str = serde_json::to_string(&args)
                .map_err(|e| ServerError::Internal(format!("Failed to serialize args: {}", e)))?;

            // Include basis_t in cache key if using a snapshot
            let cache_key_suffix = if let Some(t) = basis_t {
                format!("{}@{}", args_json_str, t)
            } else {
                args_json_str.clone()
            };
            let cache_key_time = cache_key_start.elapsed();

            // Timing: Cache lookup
            let cache_lookup_start = Instant::now();
            // Check cache first -- we cache the raw JSON from PostgreSQL
            if let Some(cached_json) = state.query_cache.get(&query, &cache_key_suffix) {
                let cache_lookup_time = cache_lookup_start.elapsed();
                debug!("Cache hit for query: {}", query);
                metrics::CACHE_HITS.inc();

                // Timing: Parse cached result
                let parse_cached_start = Instant::now();
                let result_json: serde_json::Value = serde_json::from_str(&cached_json)
                    .map_err(|e| {
                        ServerError::Internal(format!("Failed to parse cached result: {}", e))
                    })?;
                let result = parse_query_results(&result_json)?;
                let parse_cached_time = parse_cached_start.elapsed();

                let elapsed = query_start.elapsed().as_secs_f64();
                metrics::QUERY_DURATION.observe(elapsed);
                metrics::observe_operation("query", elapsed);

                tracing::debug!(
                    "Query (cached) timing - total: {:?}, snapshot: {:?}, cache_key: {:?}, cache_lookup: {:?}, parse: {:?}",
                    query_start.elapsed(), snapshot_time, cache_key_time, cache_lookup_time, parse_cached_time
                );

                return Ok(ResponseValue::Vector(result));
            }
            let cache_lookup_time = cache_lookup_start.elapsed();

            debug!("Cache miss for query: {}", query);
            metrics::CACHE_MISSES.inc();

            // Timing: Get connection from pool
            let pool_start = Instant::now();
            let client = state.pool.get().await?;
            let pool_time = pool_start.elapsed();

            // Timing: Prepare query inputs
            let prepare_start = Instant::now();
            // Build query inputs with optional basis-t for temporal filtering
            let args_json = serde_json::to_value(&args)
                .map_err(|e| ServerError::Internal(format!("Failed to serialize args: {}", e)))?;

            let inputs_json = if let Some(t) = basis_t {
                // Add basis-t for snapshot queries
                let mut inputs = serde_json::Map::new();
                inputs.insert("inputs".to_string(), args_json);
                inputs.insert("asOf".to_string(), serde_json::Value::Number(t.into()));
                serde_json::Value::Object(inputs)
            } else {
                // Regular query without snapshot
                args_json
            };
            let prepare_time = prepare_start.elapsed();

            // Timing: Execute PostgreSQL query
            let db_start = Instant::now();
            let row = client
                .query_one("SELECT mentat_query($1, $2::jsonb)", &[&query, &inputs_json])
                .await?;
            let db_time = db_start.elapsed();

            // Timing: Extract and cache result
            let cache_insert_start = Instant::now();
            let result_json: serde_json::Value = row.get(0);

            // Cache the raw JSON result with entity dependency tracking
            let json_str = result_json.to_string();
            let deps = extract_result_entities(&result_json);
            state
                .query_cache
                .insert_with_deps(&query, &cache_key_suffix, json_str, deps);
            let cache_insert_time = cache_insert_start.elapsed();

            // Timing: Parse result
            let parse_result_start = Instant::now();
            let result = parse_query_results(&result_json)?;
            let parse_result_time = parse_result_start.elapsed();

            let elapsed = query_start.elapsed().as_secs_f64();
            metrics::QUERY_DURATION.observe(elapsed);

            tracing::debug!(
                "Query timing - total: {:?}, snapshot: {:?}, cache_key: {:?}, cache_lookup: {:?}, pool: {:?}, prepare: {:?}, db: {:?}, cache_insert: {:?}, parse: {:?}",
                query_start.elapsed(), snapshot_time, cache_key_time, cache_lookup_time, pool_time, prepare_time, db_time, cache_insert_time, parse_result_time
            );

            ("query", Ok(ResponseValue::Vector(result)))
        }

        Operation::Transact {
            connection_id,
            tx_data,
        } => {
            info!(
                "Executing transaction: {} with data: {}",
                connection_id, tx_data
            );
            metrics::TRANSACTION_COUNT.inc();
            let tx_start = Instant::now();

            // Retry loop: handle serialization failures (SQLSTATE 40001) and
            // deadlocks (SQLSTATE 40P01) with exponential backoff. The
            // mentat_transact() PG function uses SERIALIZABLE isolation, which
            // may raise 40001 when concurrent transactions conflict.
            let mut attempt: u32 = 0;
            let (report_str, total_pool_time, total_db_time) = loop {
                // Timing: Get connection from pool
                let pool_start = Instant::now();
                let client = state.pool.get().await?;
                let pool_time = pool_start.elapsed();

                // Timing: Execute PostgreSQL function
                let db_start = Instant::now();
                let query_result = client
                    .query_one("SELECT mentat_transact($1)", &[&tx_data])
                    .await;
                let db_time = db_start.elapsed();

                match query_result {
                    Ok(row) => {
                        let report: String = row.get(0);
                        if attempt > 0 {
                            info!(
                                "Transaction succeeded after {} retries (total attempts: {})",
                                attempt,
                                attempt + 1
                            );
                        }
                        break (report, pool_time, db_time);
                    }
                    Err(ref e) if is_serialization_failure(e) || is_deadlock(e) => {
                        metrics::TRANSACTION_CONFLICTS.inc();

                        if attempt >= MAX_TRANSACT_RETRIES {
                            metrics::TRANSACTION_RETRY_EXHAUSTED.inc();
                            let kind = if is_serialization_failure(e) {
                                "serialization_failure"
                            } else {
                                "deadlock"
                            };
                            error!(
                                "Transaction {} failed after {} attempts ({}: {})",
                                connection_id,
                                attempt + 1,
                                kind,
                                e
                            );
                            return Err(ServerError::Conflict {
                                attempts: attempt + 1,
                            });
                        }

                        let delay = retry_delay(attempt);
                        metrics::TRANSACTION_RETRIES.inc();
                        warn!(
                            "Transaction {} got serialization conflict (attempt {}/{}), retrying in {:?}",
                            connection_id,
                            attempt + 1,
                            MAX_TRANSACT_RETRIES + 1,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        attempt += 1;
                    }
                    Err(ref e) if is_unique_violation(e) => {
                        metrics::TRANSACTION_UNIQUE_VIOLATIONS.inc();
                        let detail = e
                            .as_db_error()
                            .map(|db| {
                                format!(
                                    "{} (detail: {})",
                                    db.message(),
                                    db.detail().unwrap_or("none")
                                )
                            })
                            .unwrap_or_else(|| e.to_string());
                        error!("Transaction unique constraint violation: {}", detail);
                        return Err(ServerError::UniqueViolation(detail));
                    }
                    Err(e) => {
                        // Non-retryable error -- propagate immediately
                        return Err(ServerError::Database(e));
                    }
                }
            };

            // Timing: Parse report
            let parse_report_start = Instant::now();
            let result = parse_tx_report(&report_str)?;
            let parse_report_time = parse_report_start.elapsed();

            let tx_elapsed = tx_start.elapsed().as_secs_f64();
            metrics::TRANSACTION_DURATION.observe(tx_elapsed);

            // Timing: Cache invalidation
            let cache_start = Instant::now();
            // Extract changed entity IDs from the tx report and perform
            // targeted cache invalidation instead of clearing everything.
            let changed_entities = extract_changed_entities(&report_str);
            if changed_entities.is_empty() {
                // Could not determine affected entities -- fall back to full clear.
                state.query_cache.invalidate();
                metrics::CACHE_FULL_INVALIDATIONS.inc();
                debug!("Query cache fully invalidated (no entity info in tx report)");
            } else {
                let removed = state
                    .query_cache
                    .invalidate_entities(&changed_entities);
                metrics::CACHE_TARGETED_INVALIDATIONS.inc();
                debug!(
                    "Query cache: invalidated {} entries for {} changed entities",
                    removed,
                    changed_entities.len()
                );
            }
            let cache_time = cache_start.elapsed();

            tracing::debug!(
                "Transact timing - total: {:?}, pool: {:?}, db: {:?}, parse_report: {:?}, cache: {:?}, retries: {}",
                tx_start.elapsed(), total_pool_time, total_db_time, parse_report_time, cache_time, attempt
            );

            ("transact", Ok(result))
        }

        Operation::Pull { pattern, entity_id } => {
            info!("Executing pull: pattern={}, entity_id={}", pattern, entity_id);

            // Use cache with precise entity dependency: a pull only depends on
            // the single entity being pulled.
            let cache_query = format!("__pull__:{}", pattern);
            let cache_args = entity_id.to_string();

            if let Some(cached_json) = state.query_cache.get(&cache_query, &cache_args) {
                debug!("Cache hit for pull: entity_id={}", entity_id);
                metrics::CACHE_HITS.inc();
                let result_json: serde_json::Value = serde_json::from_str(&cached_json)
                    .map_err(|e| {
                        ServerError::Internal(format!("Failed to parse cached pull: {}", e))
                    })?;
                let result = json_to_response_value(&result_json);
                return Ok(result);
            }
            metrics::CACHE_MISSES.inc();

            let client = state.pool.get().await?;

            let row = client
                .query_one("SELECT mentat_pull($1, $2)", &[&pattern, &entity_id])
                .await?;

            let result_json: serde_json::Value = row.get(0);

            // Cache with single-entity dependency for precise invalidation.
            let json_str = result_json.to_string();
            let mut deps = std::collections::HashSet::new();
            deps.insert(entity_id);
            // Also track any ref entities in the pull result.
            collect_entity_ids_from_json(&result_json, &mut deps);
            state
                .query_cache
                .insert_with_deps(&cache_query, &cache_args, json_str, deps);

            let result = json_to_response_value(&result_json);

            ("pull", Ok(result))
        }

        Operation::BasisT => {
            info!("Executing basis-t");

            let client = state.pool.get().await?;

            let row = client
                .query_one(
                    "SELECT COALESCE(MAX(tx), 0) FROM mentat.transactions",
                    &[],
                )
                .await?;

            let basis_t: i64 = row.get(0);

            ("basis_t", Ok(ResponseValue::Integer(basis_t)))
        }

        Operation::With { tx_data } => {
            info!("Executing speculative transaction (d/with): {}", tx_data);
            metrics::TRANSACTION_COUNT.inc();

            let client = state.pool.get().await?;

            // Use a savepoint to execute the transaction speculatively.
            // BEGIN a transaction, run the transact, capture the report, then ROLLBACK.
            // This gives us the what-if results without committing.
            client.execute("BEGIN", &[]).await?;

            let result = async {
                let row = client
                    .query_one("SELECT mentat_transact($1)", &[&tx_data])
                    .await?;

                let report_str: String = row.get(0);
                let result = parse_tx_report(&report_str)?;
                Ok::<_, ServerError>(result)
            }
            .await;

            // Always rollback -- this is speculative
            client.execute("ROLLBACK", &[]).await?;

            match result {
                Ok(report) => ("with", Ok(report)),
                Err(e) => return Err(e),
            }
        }

        Operation::Filter {
            predicate,
            query,
            args,
        } => {
            info!("Executing filtered query: {} with predicate: {:?}", query, predicate);
            metrics::QUERY_COUNT.inc();

            let client = state.pool.get().await?;

            // Convert args to JSON
            let args_json = serde_json::to_value(&args)
                .map_err(|e| ServerError::Internal(format!("Failed to serialize args: {}", e)))?;
            let inputs_json = serde_json::json!({"inputs": args_json});

            // Implement d/filter by temporarily replacing mentat.datoms with a
            // filtered view within a savepoint. The view has the same columns as
            // the real table but only exposes rows matching the predicate.
            let filter_clause = build_filter_clause(&predicate);

            client.execute("SAVEPOINT filter_view", &[]).await?;

            // Rename the real table and create a filtered view in its place
            client
                .execute("ALTER TABLE mentat.datoms RENAME TO _datoms_real", &[])
                .await?;
            let view_sql = format!(
                "CREATE VIEW mentat.datoms AS SELECT * FROM mentat._datoms_real WHERE {}",
                filter_clause
            );
            client.execute(&view_sql, &[]).await?;

            let result = async {
                let row = client
                    .query_one("SELECT mentat_query($1, $2::jsonb)", &[&query, &inputs_json])
                    .await?;
                let result_json: serde_json::Value = row.get(0);
                parse_query_results(&result_json)
            }
            .await;

            // Rollback the savepoint to restore the original table name and drop
            // the view atomically, regardless of whether the query succeeded.
            let _ = client
                .execute("ROLLBACK TO SAVEPOINT filter_view", &[])
                .await;
            let _ = client
                .execute("RELEASE SAVEPOINT filter_view", &[])
                .await;

            match result {
                Ok(r) => ("filter", Ok(ResponseValue::Vector(r))),
                Err(e) => return Err(e),
            }
        }

        Operation::Datoms { index, components } => {
            info!("Executing datoms: index={:?}, components={:?}", index, components);

            let client = state.pool.get().await?;

            // Build SQL query for typed-column schema
            let query = build_datoms_query(index, components.len());

            let rows = match components.len() {
                0 => client.query(&query, &[]).await?,
                1 => {
                    let p0: i64 = components[0].parse().unwrap_or(0);
                    client.query(&query, &[&p0]).await?
                }
                2 => {
                    let p0: i64 = components[0].parse().unwrap_or(0);
                    let p1: i64 = components[1].parse().unwrap_or(0);
                    client.query(&query, &[&p0, &p1]).await?
                }
                _ => {
                    let p0: i64 = components[0].parse().unwrap_or(0);
                    let p1: i64 = components[1].parse().unwrap_or(0);
                    let p2: i64 = components[2].parse().unwrap_or(0);
                    client.query(&query, &[&p0, &p1, &p2]).await?
                }
            };

            // Row layout from build_datoms_query:
            //   e(0), a(1), value_type_tag(2), v_ref(3), v_bool(4), v_long(5),
            //   v_double(6), v_text(7), v_keyword(8), v_instant(9), v_uuid(10),
            //   v_bytes(11), tx(12), added(13)
            let datoms: Vec<ResponseValue> = rows
                .iter()
                .map(|row| {
                    let e: i64 = row.get(0);
                    let a: i64 = row.get(1);
                    let v_type_tag: i16 = row.get(2);
                    let v = decode_typed_datom_value(row, v_type_tag);
                    let tx: i64 = row.get(12);
                    let added: bool = row.get(13);

                    ResponseValue::Vector(vec![
                        ResponseValue::Integer(e),
                        ResponseValue::Integer(a),
                        v,
                        ResponseValue::Integer(tx),
                        ResponseValue::Boolean(added),
                    ])
                })
                .collect();

            ("datoms", Ok(ResponseValue::Vector(datoms)))
        }

        Operation::AsOf { query, args, t } => {
            info!("Executing as-of query at t={}: {}", t, query);
            metrics::QUERY_COUNT.inc();

            let client = state.pool.get().await?;

            // Convert args to JSON array
            let args_json = serde_json::to_value(&args)
                .map_err(|e| ServerError::Internal(format!("Failed to serialize args: {}", e)))?;

            // Build query inputs with temporal parameter
            let mut inputs = serde_json::Map::new();
            inputs.insert("inputs".to_string(), args_json);
            inputs.insert("asOf".to_string(), serde_json::Value::Number(t.into()));
            let inputs_json = serde_json::Value::Object(inputs);

            let row = client
                .query_one("SELECT mentat_query($1, $2::jsonb)", &[&query, &inputs_json])
                .await?;

            let result_json: serde_json::Value = row.get(0);
            let result = parse_query_results(&result_json)?;

            ("as_of", Ok(ResponseValue::Vector(result)))
        }

        Operation::Since { query, args, t } => {
            info!("Executing since query from t={}: {}", t, query);
            metrics::QUERY_COUNT.inc();

            let client = state.pool.get().await?;

            let args_json = serde_json::to_value(&args)
                .map_err(|e| ServerError::Internal(format!("Failed to serialize args: {}", e)))?;

            let mut inputs = serde_json::Map::new();
            inputs.insert("inputs".to_string(), args_json);
            inputs.insert("since".to_string(), serde_json::Value::Number(t.into()));
            let inputs_json = serde_json::Value::Object(inputs);

            let row = client
                .query_one("SELECT mentat_query($1, $2::jsonb)", &[&query, &inputs_json])
                .await?;

            let result_json: serde_json::Value = row.get(0);
            let result = parse_query_results(&result_json)?;

            ("since", Ok(ResponseValue::Vector(result)))
        }

        Operation::History { query, args } => {
            info!("Executing history query: {}", query);
            metrics::QUERY_COUNT.inc();

            let client = state.pool.get().await?;

            let args_json = serde_json::to_value(&args)
                .map_err(|e| ServerError::Internal(format!("Failed to serialize args: {}", e)))?;

            let mut inputs = serde_json::Map::new();
            inputs.insert("inputs".to_string(), args_json);
            inputs.insert("history".to_string(), serde_json::Value::Bool(true));
            let inputs_json = serde_json::Value::Object(inputs);

            let row = client
                .query_one("SELECT mentat_query($1, $2::jsonb)", &[&query, &inputs_json])
                .await?;

            let result_json: serde_json::Value = row.get(0);
            let result = parse_query_results(&result_json)?;

            ("history", Ok(ResponseValue::Vector(result)))
        }

        Operation::TxRange { start, end } => {
            info!("Executing tx-range: start={:?}, end={:?}", start, end);

            let client = state.pool.get().await?;

            #[allow(clippy::match_same_arms)]
            let query = match (start, end) {
                (Some(_), Some(_)) => {
                    "SELECT tx, tx_instant FROM mentat.transactions WHERE tx BETWEEN $1 AND $2 ORDER BY tx"
                }
                (Some(_), _) => {
                    "SELECT tx, tx_instant FROM mentat.transactions WHERE tx >= $1 ORDER BY tx"
                }
                (_, Some(_)) => {
                    "SELECT tx, tx_instant FROM mentat.transactions WHERE tx <= $1 ORDER BY tx"
                }
                (_, _) => {
                    "SELECT tx, tx_instant FROM mentat.transactions ORDER BY tx"
                }
            };

            let rows = if let (Some(s), Some(e)) = (start, end) {
                client.query(query, &[&s, &e]).await?
            } else if let Some(s) = start {
                client.query(query, &[&s]).await?
            } else if let Some(e) = end {
                client.query(query, &[&e]).await?
            } else {
                client.query(query, &[]).await?
            };

            let transactions: Vec<ResponseValue> = rows
                .iter()
                .map(|row| {
                    let tx: i64 = row.get(0);
                    let tx_instant: Option<String> = row.get(1);
                    ResponseValue::Map(vec![
                        (
                            ResponseValue::Keyword("tx".to_string()),
                            ResponseValue::Integer(tx),
                        ),
                        (
                            ResponseValue::Keyword("tx-instant".to_string()),
                            tx_instant.map_or(ResponseValue::Nil, |s| ResponseValue::String(s)),
                        ),
                    ])
                })
                .collect();

            ("tx_range", Ok(ResponseValue::Vector(transactions)))
        }
    };

    // Record per-operation duration and count
    let elapsed = op_start.elapsed().as_secs_f64();
    metrics::observe_operation(op_name, elapsed);

    result
}

/// Convert a JSON value from `mentat_query()` to a native `ResponseValue`.
fn json_to_response_value(val: &serde_json::Value) -> ResponseValue {
    match val {
        serde_json::Value::String(s) => {
            if let Some(stripped) = s.strip_prefix(':') {
                ResponseValue::Keyword(stripped.to_string())
            } else {
                ResponseValue::String(s.clone())
            }
        }
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                ResponseValue::Integer(i)
            } else if let Some(u) = n.as_u64() {
                // Fallback: treat as i64 if it fits, otherwise as string
                #[allow(clippy::cast_possible_wrap)]
                if u <= i64::MAX as u64 {
                    ResponseValue::Integer(u as i64)
                } else {
                    ResponseValue::String(u.to_string())
                }
            } else if let Some(f) = n.as_f64() {
                ResponseValue::Float(f)
            } else {
                ResponseValue::String(n.to_string())
            }
        }
        serde_json::Value::Bool(b) => ResponseValue::Boolean(*b),
        serde_json::Value::Null => ResponseValue::Nil,
        serde_json::Value::Array(arr) => {
            let items = arr.iter().map(json_to_response_value).collect();
            ResponseValue::Vector(items)
        }
        serde_json::Value::Object(obj) => {
            // Check for type-tagged values from the pg_mentat extension:
            //   {"_t":"inst","v":<micros>}   -> Instant
            //   {"_t":"uuid","v":"<string>"}  -> Uuid
            //   {"_t":"double","v":<number>}  -> Float
            if let Some(serde_json::Value::String(type_tag)) = obj.get("_t") {
                match type_tag.as_str() {
                    "inst" => {
                        if let Some(micros) = obj.get("v").and_then(|v| v.as_i64()) {
                            return ResponseValue::Instant(micros);
                        }
                    }
                    "uuid" => {
                        if let Some(uuid_str) = obj.get("v").and_then(|v| v.as_str()) {
                            return ResponseValue::Uuid(uuid_str.to_string());
                        }
                    }
                    "double" => {
                        if let Some(f) = obj.get("v").and_then(|v| v.as_f64()) {
                            return ResponseValue::Float(f);
                        }
                    }
                    _ => {}
                }
            }
            let entries = obj
                .iter()
                .map(|(k, v)| {
                    (
                        ResponseValue::Keyword(k.clone()),
                        json_to_response_value(v),
                    )
                })
                .collect();
            ResponseValue::Map(entries)
        }
    }
}

/// Parse the JSONB query result from `mentat_query()` into a vector of native EDN vectors.
///
/// The extension returns JSON like:
/// ```json
/// {"columns": ["?name", "?age"], "results": [["Alice", 30], ["Bob", 25]]}
/// ```
///
/// This converts each result row to a native `ResponseValue::Vector`, e.g. `[10001 "Alice"]`.
fn parse_query_results(json: &serde_json::Value) -> Result<Vec<ResponseValue>, ServerError> {
    let results = json
        .get("results")
        .and_then(|r| r.as_array())
        .ok_or_else(|| ServerError::Internal("Missing 'results' in query response".to_string()))?;

    let mut edn_rows = Vec::with_capacity(results.len());
    for row in results {
        let row_arr = row
            .as_array()
            .ok_or_else(|| ServerError::Internal("Expected array for result row".to_string()))?;

        let row_values: Vec<ResponseValue> = row_arr.iter().map(json_to_response_value).collect();
        edn_rows.push(ResponseValue::Vector(row_values));
    }

    Ok(edn_rows)
}

/// Parse the JSON string returned by `mentat_transact()` into a response map.
///
/// The extension returns a Datomic-compatible JSON string like:
/// ```json
/// {
///   "db-before": {"basis-t": 1234},
///   "db-after": {"basis-t": 1235},
///   "tx-data": [[e, a, v, tx, added], ...],
///   "tempids": {"tempid1": 10001}
/// }
/// ```
///
/// This converts it to a `ResponseValue::Map` matching Datomic's transaction report format.
/// For backwards compatibility, also handles the legacy format with "tx-id" fields.
fn parse_tx_report(report_str: &str) -> Result<ResponseValue, ServerError> {
    let report: serde_json::Value = serde_json::from_str(report_str)
        .map_err(|e| ServerError::Internal(format!("Failed to parse tx report: {}", e)))?;

    let mut entries = Vec::new();

    // Datomic-compatible format: db-before, db-after, tx-data, tempids
    if let Some(db_before) = report.get("db-before") {
        entries.push((
            ResponseValue::Keyword("db-before".to_string()),
            json_to_response_value(db_before),
        ));
    }

    if let Some(db_after) = report.get("db-after") {
        entries.push((
            ResponseValue::Keyword("db-after".to_string()),
            json_to_response_value(db_after),
        ));
    }

    if let Some(tx_data) = report.get("tx-data") {
        entries.push((
            ResponseValue::Keyword("tx-data".to_string()),
            json_to_response_value(tx_data),
        ));
    }

    if let Some(tempids) = report.get("tempids") {
        // Datomic tempids map has String keys (the tempid strings), not keywords.
        // json_to_response_value converts all object keys to Keywords, so we
        // handle tempids specially to produce {"tempid-str" entity-id} in EDN.
        let tempids_value = match tempids {
            serde_json::Value::Object(obj) => {
                let map_entries: Vec<(ResponseValue, ResponseValue)> = obj
                    .iter()
                    .map(|(k, v)| {
                        (
                            ResponseValue::String(k.clone()),
                            json_to_response_value(v),
                        )
                    })
                    .collect();
                ResponseValue::Map(map_entries)
            }
            other => json_to_response_value(other),
        };
        entries.push((
            ResponseValue::Keyword("tempids".to_string()),
            tempids_value,
        ));
    }

    // Legacy format fields (backwards compatibility)
    if let Some(tx_id) = report.get("tx-id") {
        entries.push((
            ResponseValue::Keyword("tx-id".to_string()),
            json_to_response_value(tx_id),
        ));
    }

    if let Some(tx_instant) = report.get("tx-instant") {
        entries.push((
            ResponseValue::Keyword("tx-instant".to_string()),
            json_to_response_value(tx_instant),
        ));
    }

    if let Some(datoms) = report.get("datoms-inserted") {
        entries.push((
            ResponseValue::Keyword("datoms-inserted".to_string()),
            json_to_response_value(datoms),
        ));
    }

    Ok(ResponseValue::Map(entries))
}

fn is_valid_db_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name
            .chars()
            .next()
            .map_or(false, |c| c.is_ascii_alphabetic())
}

/// Build a SQL filter clause from a `FilterPredicate`.
///
/// Returns a SQL WHERE clause fragment that can be passed to the query engine
/// to restrict which datoms are visible.
///
/// SECURITY: All string values are escaped using `escape_sql_string` to prevent
/// SQL injection. Integer values are safe because they are typed as `i64`.
/// Custom expressions are rejected to eliminate arbitrary SQL execution.
fn build_filter_clause(predicate: &FilterPredicate) -> String {
    match predicate {
        FilterPredicate::AttrEquals(attr) => {
            // Filter datoms to only those with a specific attribute ident.
            // The attr string may be a keyword like ":person/name" or a quoted form.
            let clean = attr
                .trim_matches(|c| c == ':' || c == '"')
                .to_string();
            // Validate: attribute idents must be alphanumeric with / . - _
            if !is_valid_attribute_ident(&clean) {
                return "FALSE".to_string(); // safe no-op: match nothing
            }
            format!(
                "a = (SELECT entid FROM mentat.idents WHERE ident = '{}' LIMIT 1)",
                escape_sql_string(&format!(":{}", clean))
            )
        }
        FilterPredicate::EntityEquals(eid) => {
            // i64 is safe from injection -- format! produces a numeric literal
            format!("e = {}", eid)
        }
        FilterPredicate::Since(t) => {
            // i64 is safe from injection
            format!("tx > {}", t)
        }
        FilterPredicate::Custom(_expr) => {
            // SECURITY: Custom SQL expressions are rejected. Allowing arbitrary
            // user-supplied SQL fragments is inherently unsafe.
            warn!("Custom filter predicates are disabled for security. Use built-in predicates.");
            "FALSE".to_string()
        }
    }
}

/// Validate an attribute ident string.
///
/// Attribute idents follow the pattern `namespace/name` where each part contains
/// only alphanumeric characters, hyphens, underscores, and dots.
fn is_valid_attribute_ident(ident: &str) -> bool {
    !ident.is_empty()
        && ident.len() <= 256
        && ident
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '-' || c == '_' || c == '.')
}

/// Escape a string value for safe inclusion in a SQL string literal.
///
/// Doubles single quotes to prevent SQL injection via quote-breaking.
fn escape_sql_string(s: &str) -> String {
    s.replace('\'', "''")
}

/// Quote a PostgreSQL identifier (database name, table name, etc.).
///
/// Wraps the identifier in double quotes and escapes any embedded double quotes.
fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Build a SQL query for datoms index access using the typed-column schema.
///
/// Returns a query that selects all typed value columns. The result columns are:
///   e(0), a(1), value_type_tag(2), v_ref(3), v_bool(4), v_long(5),
///   v_double(6), v_text(7), v_keyword(8), v_instant_micros(9), v_uuid_str(10),
///   v_bytes(11), tx(12), added(13)
///
/// For the typed-column schema, value filtering (v component) only works for
/// VAET index (ref lookups via v_ref) since the value column depends on type.
/// Other value-based filters are ignored (only e and a components are used).
fn build_datoms_query(
    index: crate::protocol::DatomsIndex,
    component_count: usize,
) -> String {
    use crate::protocol::DatomsIndex;

    // Select all typed value columns; convert v_instant to epoch microseconds
    // and v_uuid to text so tokio-postgres can read them without extra features.
    let base_query = "\
        SELECT e, a, value_type_tag, \
               v_ref, v_bool, v_long, v_double, v_text, v_keyword, \
               (EXTRACT(EPOCH FROM v_instant) * 1000000)::BIGINT AS v_instant_micros, \
               v_uuid::TEXT AS v_uuid_str, \
               v_bytes, \
               tx, added \
        FROM mentat.datoms";

    let where_clause = match index {
        DatomsIndex::EAVT => {
            // EAVT: components are [e, a] (v filtering not supported for typed columns)
            match component_count {
                0 => " WHERE added = true".to_string(),
                1 => " WHERE e = $1 AND added = true".to_string(),
                _ => " WHERE e = $1 AND a = $2 AND added = true".to_string(),
            }
        }
        DatomsIndex::AEVT => {
            // AEVT: components are [a, e]
            match component_count {
                0 => " WHERE added = true".to_string(),
                1 => " WHERE a = $1 AND added = true".to_string(),
                _ => " WHERE a = $1 AND e = $2 AND added = true".to_string(),
            }
        }
        DatomsIndex::AVET => {
            // AVET: components are [a] (v and e filtering requires type knowledge)
            match component_count {
                0 => " WHERE added = true".to_string(),
                _ => " WHERE a = $1 AND added = true".to_string(),
            }
        }
        DatomsIndex::VAET => {
            // VAET: components are [v_ref, a, e] (ref-only index)
            match component_count {
                0 => " WHERE value_type_tag = 0 AND added = true".to_string(),
                1 => " WHERE v_ref = $1 AND value_type_tag = 0 AND added = true".to_string(),
                2 => " WHERE v_ref = $1 AND a = $2 AND value_type_tag = 0 AND added = true".to_string(),
                _ => " WHERE v_ref = $1 AND a = $2 AND e = $3 AND value_type_tag = 0 AND added = true".to_string(),
            }
        }
    };

    let order_clause = match index {
        DatomsIndex::EAVT => " ORDER BY e, a, value_type_tag, tx",
        DatomsIndex::AEVT => " ORDER BY a, e, value_type_tag, tx",
        DatomsIndex::AVET => " ORDER BY a, value_type_tag, e, tx",
        DatomsIndex::VAET => " ORDER BY v_ref, a, e, tx",
    };

    format!("{}{}{}", base_query, where_clause, order_clause)
}

/// Decode a datom value from a typed-column row into a ResponseValue.
///
/// The row columns at positions 3-11 are the typed value columns:
///   3: v_ref (BIGINT), 4: v_bool (BOOL), 5: v_long (BIGINT),
///   6: v_double (FLOAT8), 7: v_text (TEXT), 8: v_keyword (TEXT),
///   9: v_instant_micros (BIGINT, from EXTRACT), 10: v_uuid_str (TEXT),
///   11: v_bytes (BYTEA)
fn decode_typed_datom_value(row: &tokio_postgres::Row, v_type_tag: i16) -> ResponseValue {
    match v_type_tag {
        0 => {
            // REF -> v_ref at column 3
            let v: Option<i64> = row.get(3);
            v.map_or(ResponseValue::Nil, ResponseValue::Integer)
        }
        1 => {
            // BOOLEAN -> v_bool at column 4
            let v: Option<bool> = row.get(4);
            v.map_or(ResponseValue::Nil, ResponseValue::Boolean)
        }
        2 => {
            // LONG -> v_long at column 5
            let v: Option<i64> = row.get(5);
            v.map_or(ResponseValue::Nil, ResponseValue::Integer)
        }
        3 => {
            // DOUBLE -> v_double at column 6
            let v: Option<f64> = row.get(6);
            v.map_or(ResponseValue::Nil, ResponseValue::Float)
        }
        4 => {
            // INSTANT -> v_instant_micros at column 9 (epoch microseconds via SQL)
            let v: Option<i64> = row.get(9);
            v.map_or(ResponseValue::Nil, ResponseValue::Instant)
        }
        7 => {
            // STRING -> v_text at column 7
            let v: Option<String> = row.get(7);
            v.map_or(ResponseValue::Nil, ResponseValue::String)
        }
        8 => {
            // KEYWORD -> v_keyword at column 8
            let v: Option<String> = row.get(8);
            v.map_or(ResponseValue::Nil, ResponseValue::Keyword)
        }
        10 => {
            // UUID -> v_uuid_str at column 10 (text via SQL cast)
            let v: Option<String> = row.get(10);
            v.map_or(ResponseValue::Nil, ResponseValue::Uuid)
        }
        11 => {
            // BYTES -> v_bytes at column 11
            let v: Option<Vec<u8>> = row.get(11);
            v.map_or(ResponseValue::Nil, |b| ResponseValue::String(format!("0x{}", hex::encode(&b))))
        }
        _ => ResponseValue::Nil,
    }
}

/// Decode a datom value from raw BYTEA bytes and type tag into a ResponseValue.
///
/// This is the legacy decoder for the old single-column `v BYTEA` schema.
/// Kept for backward compatibility with tests and any external tools that
/// may still produce BYTEA-encoded datom values.
#[cfg(test)]
fn decode_datom_value(v_bytes: &[u8], v_type_tag: i16) -> ResponseValue {
    match v_type_tag {
        0 => {
            if v_bytes.len() == 8 {
                let bytes: [u8; 8] = v_bytes.try_into().unwrap_or([0; 8]);
                ResponseValue::Integer(i64::from_le_bytes(bytes))
            } else {
                ResponseValue::Nil
            }
        }
        1 => {
            if v_bytes.len() == 1 {
                ResponseValue::Boolean(v_bytes[0] != 0)
            } else {
                ResponseValue::Nil
            }
        }
        2 => {
            if v_bytes.len() == 8 {
                let bytes: [u8; 8] = v_bytes.try_into().unwrap_or([0; 8]);
                ResponseValue::Integer(i64::from_le_bytes(bytes))
            } else {
                ResponseValue::Nil
            }
        }
        3 => {
            if v_bytes.len() == 8 {
                let bytes: [u8; 8] = v_bytes.try_into().unwrap_or([0; 8]);
                ResponseValue::Float(f64::from_le_bytes(bytes))
            } else {
                ResponseValue::Nil
            }
        }
        4 => {
            if v_bytes.len() == 8 {
                let bytes: [u8; 8] = v_bytes.try_into().unwrap_or([0; 8]);
                ResponseValue::Instant(i64::from_le_bytes(bytes))
            } else {
                ResponseValue::Nil
            }
        }
        7 => String::from_utf8(v_bytes.to_vec())
            .map(ResponseValue::String)
            .unwrap_or(ResponseValue::Nil),
        8 => String::from_utf8(v_bytes.to_vec())
            .map(ResponseValue::Keyword)
            .unwrap_or(ResponseValue::Nil),
        10 => {
            if v_bytes.len() == 16 {
                let uuid_bytes: [u8; 16] = v_bytes.try_into().unwrap_or([0; 16]);
                let uuid = uuid::Uuid::from_bytes(uuid_bytes);
                ResponseValue::Uuid(uuid.to_string())
            } else {
                ResponseValue::Nil
            }
        }
        11 => ResponseValue::String(format!("0x{}", hex::encode(v_bytes))),
        _ => ResponseValue::Nil,
    }
}

/// Extract entity IDs from a Datomic-compatible transaction report JSON string.
///
/// The report may have a `"tx-data"` field containing an array of datom arrays
/// where the first element of each datom is the entity ID.  Returns an empty
/// `Vec` if the report does not contain parseable tx-data.
fn extract_changed_entities(report_str: &str) -> Vec<i64> {
    let report: serde_json::Value = match serde_json::from_str(report_str) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let tx_data = match report.get("tx-data").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return Vec::new(),
    };

    let mut entities = std::collections::HashSet::new();
    for datom in tx_data {
        if let Some(arr) = datom.as_array() {
            if let Some(e) = arr.first().and_then(|v| v.as_i64()) {
                entities.insert(e);
            }
        }
    }

    entities.into_iter().collect()
}

/// Extract entity IDs that appear in a query result JSON value.
///
/// The extension returns results as `{"results": [[col1, col2, ...], ...]}`.
/// We scan each result column for integer values, which are likely entity IDs
/// or attribute IDs.  This is a conservative over-approximation: we track all
/// integer values as potential entity dependencies, which may cause some
/// unnecessary invalidations but never misses a true dependency.
fn extract_result_entities(result_json: &serde_json::Value) -> std::collections::HashSet<i64> {
    let mut entities = std::collections::HashSet::new();

    if let Some(results) = result_json.get("results").and_then(|r| r.as_array()) {
        for row in results {
            if let Some(cols) = row.as_array() {
                for val in cols {
                    if let Some(id) = val.as_i64() {
                        entities.insert(id);
                    }
                }
            }
        }
    }

    entities
}

/// Recursively collect all integer values from a JSON value.
///
/// Used to extract entity IDs from pull results, which may contain nested
/// maps with `:db/id` fields and ref attribute values.
fn collect_entity_ids_from_json(value: &serde_json::Value, out: &mut std::collections::HashSet<i64>) {
    match value {
        serde_json::Value::Number(n) => {
            if let Some(id) = n.as_i64() {
                out.insert(id);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                collect_entity_ids_from_json(v, out);
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values() {
                collect_entity_ids_from_json(v, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_db_name() {
        assert!(is_valid_db_name("test_db"));
        assert!(is_valid_db_name("mydb123"));
        assert!(!is_valid_db_name("123db"));
        assert!(!is_valid_db_name("my-db"));
        assert!(!is_valid_db_name(""));
    }

    #[test]
    fn test_parse_query_results_basic() {
        let json = serde_json::json!({
            "columns": ["?name", "?age"],
            "results": [
                ["Alice", 30],
                ["Bob", 25]
            ]
        });
        let result = parse_query_results(&json);
        assert!(result.is_ok());
        let rows = result.unwrap_or_default();
        assert_eq!(rows.len(), 2);

        // Verify first row is a Vector with native types
        match &rows[0] {
            ResponseValue::Vector(vals) => {
                assert_eq!(vals.len(), 2);
                match &vals[0] {
                    ResponseValue::String(s) => assert_eq!(s, "Alice"),
                    other => panic!("Expected String, got {:?}", other),
                }
                match &vals[1] {
                    ResponseValue::Integer(i) => assert_eq!(*i, 30),
                    other => panic!("Expected Integer, got {:?}", other),
                }
            }
            other => panic!("Expected Vector, got {:?}", other),
        }

        // Verify second row
        match &rows[1] {
            ResponseValue::Vector(vals) => {
                assert_eq!(vals.len(), 2);
                match &vals[0] {
                    ResponseValue::String(s) => assert_eq!(s, "Bob"),
                    other => panic!("Expected String, got {:?}", other),
                }
                match &vals[1] {
                    ResponseValue::Integer(i) => assert_eq!(*i, 25),
                    other => panic!("Expected Integer, got {:?}", other),
                }
            }
            other => panic!("Expected Vector, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_query_results_serialized_format() {
        use crate::protocol::serializer::serialize_response;
        use crate::protocol::Response;

        let json = serde_json::json!({
            "columns": ["?name", "?age"],
            "results": [
                ["Alice", 30],
                ["Bob", 25]
            ]
        });
        let rows = parse_query_results(&json).unwrap_or_default();
        let response = Response::Success {
            result: ResponseValue::Vector(rows),
        };
        let output = serialize_response(&response);
        assert_eq!(output, r#"{:result [["Alice" 30] ["Bob" 25]]}"#);
    }

    #[test]
    fn test_parse_query_results_empty() {
        let json = serde_json::json!({
            "columns": ["?name"],
            "results": []
        });
        let result = parse_query_results(&json);
        assert!(result.is_ok());
        assert!(result.unwrap_or_default().is_empty());
    }

    #[test]
    fn test_parse_query_results_mixed_types() {
        let json = serde_json::json!({
            "columns": ["?v"],
            "results": [
                ["hello"],
                [42],
                [true],
                [null]
            ]
        });
        let result = parse_query_results(&json);
        assert!(result.is_ok());
        let rows = result.unwrap_or_default();

        match &rows[0] {
            ResponseValue::Vector(vals) => match &vals[0] {
                ResponseValue::String(s) => assert_eq!(s, "hello"),
                other => panic!("Expected String, got {:?}", other),
            },
            other => panic!("Expected Vector, got {:?}", other),
        }
        match &rows[1] {
            ResponseValue::Vector(vals) => match &vals[0] {
                ResponseValue::Integer(i) => assert_eq!(*i, 42),
                other => panic!("Expected Integer, got {:?}", other),
            },
            other => panic!("Expected Vector, got {:?}", other),
        }
        match &rows[2] {
            ResponseValue::Vector(vals) => match &vals[0] {
                ResponseValue::Boolean(b) => assert!(*b),
                other => panic!("Expected Boolean, got {:?}", other),
            },
            other => panic!("Expected Vector, got {:?}", other),
        }
        match &rows[3] {
            ResponseValue::Vector(vals) => match &vals[0] {
                ResponseValue::Nil => {}
                other => panic!("Expected Nil, got {:?}", other),
            },
            other => panic!("Expected Vector, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_query_results_mixed_types_serialized() {
        use crate::protocol::serializer::serialize_response;
        use crate::protocol::Response;

        let json = serde_json::json!({
            "columns": ["?v"],
            "results": [
                ["hello"],
                [42],
                [true],
                [null]
            ]
        });
        let rows = parse_query_results(&json).unwrap_or_default();
        let response = Response::Success {
            result: ResponseValue::Vector(rows),
        };
        let output = serialize_response(&response);
        assert_eq!(output, r#"{:result [["hello"] [42] [true] [nil]]}"#);
    }

    #[test]
    fn test_parse_query_results_missing_results_field() {
        let json = serde_json::json!({"columns": ["?x"]});
        let result = parse_query_results(&json);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_tx_report_legacy_format() {
        // Test backwards compatibility with the legacy format
        let report_str =
            r#"{"tx-id":12345,"tx-instant":null,"tempids":{"tempid1":100},"datoms-inserted":3}"#;
        let result = parse_tx_report(report_str);
        assert!(result.is_ok());

        // Verify the map structure
        match result.unwrap_or(ResponseValue::Nil) {
            ResponseValue::Map(entries) => {
                // tx-id should be an integer
                let tx_id_entry = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "tx-id"));
                assert!(tx_id_entry.is_some());
                match &tx_id_entry.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                    ResponseValue::Integer(i) => assert_eq!(*i, 12345),
                    other => panic!("Expected Integer for tx-id, got {:?}", other),
                }

                // tx-instant should be nil
                let tx_instant_entry = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "tx-instant"));
                assert!(tx_instant_entry.is_some());
                match &tx_instant_entry.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                    ResponseValue::Nil => {}
                    other => panic!("Expected Nil for tx-instant, got {:?}", other),
                }

                // datoms-inserted should be an integer
                let datoms_entry = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "datoms-inserted"));
                assert!(datoms_entry.is_some());
                match &datoms_entry.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                    ResponseValue::Integer(i) => assert_eq!(*i, 3),
                    other => panic!("Expected Integer for datoms-inserted, got {:?}", other),
                }

                // tempids should be a map
                let tempids_entry = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "tempids"));
                assert!(tempids_entry.is_some());
                match &tempids_entry.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                    ResponseValue::Map(inner) => {
                        assert_eq!(inner.len(), 1);
                    }
                    other => panic!("Expected Map for tempids, got {:?}", other),
                }
            }
            other => panic!("Expected Map, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_tx_report_legacy_serialized_format() {
        use crate::protocol::serializer::serialize_response;
        use crate::protocol::Response;

        let report_str =
            r#"{"tx-id":12345,"tx-instant":null,"tempids":{},"datoms-inserted":3}"#;
        let result = parse_tx_report(report_str).unwrap_or(ResponseValue::Nil);
        let response = Response::Success { result };
        let output = serialize_response(&response);
        assert!(output.contains(":tx-id 12345"));
        assert!(output.contains(":tx-instant nil"));
        assert!(output.contains(":datoms-inserted 3"));
        assert!(output.contains(":tempids {}"));
    }

    #[test]
    fn test_parse_tx_report_empty_tempids() {
        let report_str = r#"{"tx-id":1,"tx-instant":null,"tempids":{},"datoms-inserted":0}"#;
        let result = parse_tx_report(report_str);
        assert!(result.is_ok());
        match result.unwrap_or(ResponseValue::Nil) {
            ResponseValue::Map(entries) => {
                let tempids_entry = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "tempids"));
                assert!(tempids_entry.is_some());
                match &tempids_entry.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                    ResponseValue::Map(inner) => assert!(inner.is_empty()),
                    other => panic!("Expected empty Map for tempids, got {:?}", other),
                }
            }
            other => panic!("Expected Map, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_tx_report_invalid_json() {
        let result = parse_tx_report("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_tx_report_datomic_format() {
        // Test the new Datomic-compatible transaction report format
        let report_str = r#"{"db-before":{"basis-t":1000},"db-after":{"basis-t":1001},"tx-data":[[1001,10,1714000000000000,1001,true],[5001,100,"Alice",1001,true]],"tempids":{"alice":5001}}"#;
        let result = parse_tx_report(report_str);
        assert!(result.is_ok());

        match result.unwrap_or(ResponseValue::Nil) {
            ResponseValue::Map(entries) => {
                // db-before should be a map with basis-t
                let db_before = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "db-before"));
                assert!(db_before.is_some(), "Missing :db-before");
                match &db_before.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                    ResponseValue::Map(inner) => {
                        assert_eq!(inner.len(), 1);
                        let basis_t = inner.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "basis-t"));
                        assert!(basis_t.is_some());
                        match &basis_t.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                            ResponseValue::Integer(t) => assert_eq!(*t, 1000),
                            other => panic!("Expected Integer for basis-t, got {:?}", other),
                        }
                    }
                    other => panic!("Expected Map for db-before, got {:?}", other),
                }

                // db-after should be a map with basis-t
                let db_after = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "db-after"));
                assert!(db_after.is_some(), "Missing :db-after");
                match &db_after.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                    ResponseValue::Map(inner) => {
                        let basis_t = inner.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "basis-t"));
                        match &basis_t.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                            ResponseValue::Integer(t) => assert_eq!(*t, 1001),
                            other => panic!("Expected Integer for basis-t, got {:?}", other),
                        }
                    }
                    other => panic!("Expected Map for db-after, got {:?}", other),
                }

                // tx-data should be a vector of vectors
                let tx_data = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "tx-data"));
                assert!(tx_data.is_some(), "Missing :tx-data");
                match &tx_data.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                    ResponseValue::Vector(datoms) => {
                        assert_eq!(datoms.len(), 2);
                        // First datom should be [1001, 10, 1714000000000000, 1001, true]
                        match &datoms[0] {
                            ResponseValue::Vector(d) => {
                                assert_eq!(d.len(), 5);
                                match &d[0] {
                                    ResponseValue::Integer(e) => assert_eq!(*e, 1001),
                                    other => panic!("Expected Integer for e, got {:?}", other),
                                }
                                match &d[4] {
                                    ResponseValue::Boolean(added) => assert!(*added),
                                    other => panic!("Expected Boolean for added, got {:?}", other),
                                }
                            }
                            other => panic!("Expected Vector for datom, got {:?}", other),
                        }
                    }
                    other => panic!("Expected Vector for tx-data, got {:?}", other),
                }

                // tempids should be a map with String keys (not Keyword keys)
                let tempids = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "tempids"));
                assert!(tempids.is_some(), "Missing :tempids");
                match &tempids.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                    ResponseValue::Map(inner) => {
                        assert_eq!(inner.len(), 1);
                        // Verify the key is a String, not a Keyword (Datomic tempids use string keys)
                        match &inner[0].0 {
                            ResponseValue::String(k) => assert_eq!(k, "alice"),
                            other => panic!("Expected String key for tempid, got {:?}", other),
                        }
                        match &inner[0].1 {
                            ResponseValue::Integer(v) => assert_eq!(*v, 5001),
                            other => panic!("Expected Integer value for tempid, got {:?}", other),
                        }
                    }
                    other => panic!("Expected Map for tempids, got {:?}", other),
                }
            }
            other => panic!("Expected Map, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_tx_report_datomic_format_serialized() {
        use crate::protocol::serializer::serialize_response;
        use crate::protocol::Response;

        let report_str = r#"{"db-before":{"basis-t":0},"db-after":{"basis-t":1001},"tx-data":[[1001,10,1714000000000000,1001,true]],"tempids":{}}"#;
        let result = parse_tx_report(report_str).unwrap_or(ResponseValue::Nil);
        let response = Response::Success { result };
        let output = serialize_response(&response);
        assert!(output.contains(":db-before {:basis-t 0}"), "Output missing db-before: {}", output);
        assert!(output.contains(":db-after {:basis-t 1001}"), "Output missing db-after: {}", output);
        assert!(output.contains(":tx-data"), "Output missing tx-data: {}", output);
        assert!(output.contains(":tempids {}"), "Output missing tempids: {}", output);
    }

    #[test]
    fn test_json_to_response_value_keyword_detection() {
        let val = serde_json::json!(":db/name");
        match json_to_response_value(&val) {
            ResponseValue::Keyword(k) => assert_eq!(k, "db/name"),
            other => panic!("Expected Keyword, got {:?}", other),
        }
    }

    #[test]
    fn test_json_to_response_value_string() {
        let val = serde_json::json!("hello");
        match json_to_response_value(&val) {
            ResponseValue::String(s) => assert_eq!(s, "hello"),
            other => panic!("Expected String, got {:?}", other),
        }
    }

    #[test]
    fn test_json_to_response_value_nested_array() {
        let val = serde_json::json!([1, "two", [3]]);
        match json_to_response_value(&val) {
            ResponseValue::Vector(items) => {
                assert_eq!(items.len(), 3);
                match &items[2] {
                    ResponseValue::Vector(inner) => {
                        assert_eq!(inner.len(), 1);
                        match &inner[0] {
                            ResponseValue::Integer(i) => assert_eq!(*i, 3),
                            other => panic!("Expected Integer, got {:?}", other),
                        }
                    }
                    other => panic!("Expected Vector, got {:?}", other),
                }
            }
            other => panic!("Expected Vector, got {:?}", other),
        }
    }

    #[test]
    fn test_build_filter_clause_attr_equals() {
        let pred = FilterPredicate::AttrEquals(":person/name".to_string());
        let clause = build_filter_clause(&pred);
        assert!(clause.contains("person/name"));
        assert!(clause.contains("SELECT entid FROM mentat.idents"));
    }

    #[test]
    fn test_build_filter_clause_entity_equals() {
        let pred = FilterPredicate::EntityEquals(42);
        let clause = build_filter_clause(&pred);
        assert_eq!(clause, "e = 42");
    }

    #[test]
    fn test_build_filter_clause_since() {
        let pred = FilterPredicate::Since(1000);
        let clause = build_filter_clause(&pred);
        assert_eq!(clause, "tx > 1000");
    }

    #[test]
    fn test_build_filter_clause_custom_rejected() {
        // Custom predicates are rejected for security -- they always return FALSE
        let pred = FilterPredicate::Custom("a = 10 AND e > 5; DROP TABLE users".to_string());
        let clause = build_filter_clause(&pred);
        assert_eq!(clause, "FALSE");
    }

    // ---- Entity extraction for cache invalidation ----

    #[test]
    fn test_extract_changed_entities_datomic_format() {
        let report = r#"{"db-before":{"basis-t":1000},"db-after":{"basis-t":1001},"tx-data":[[1001,10,1714000000000000,1001,true],[5001,100,"Alice",1001,true],[5002,100,"Bob",1001,true]],"tempids":{}}"#;
        let mut entities = extract_changed_entities(report);
        entities.sort();
        assert_eq!(entities, vec![1001, 5001, 5002]);
    }

    #[test]
    fn test_extract_changed_entities_legacy_format() {
        // Legacy format has no tx-data -- should return empty
        let report = r#"{"tx-id":12345,"tx-instant":null,"tempids":{},"datoms-inserted":3}"#;
        let entities = extract_changed_entities(report);
        assert!(entities.is_empty());
    }

    #[test]
    fn test_extract_changed_entities_invalid_json() {
        let entities = extract_changed_entities("not json at all");
        assert!(entities.is_empty());
    }

    #[test]
    fn test_extract_changed_entities_empty_tx_data() {
        let report = r#"{"tx-data":[],"tempids":{}}"#;
        let entities = extract_changed_entities(report);
        assert!(entities.is_empty());
    }

    #[test]
    fn test_extract_changed_entities_deduplicates() {
        // Same entity appears in multiple datoms
        let report = r#"{"tx-data":[[5001,100,"Alice",1001,true],[5001,101,30,1001,true]]}"#;
        let entities = extract_changed_entities(report);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0], 5001);
    }

    #[test]
    fn test_extract_result_entities_basic() {
        let json = serde_json::json!({
            "columns": ["?e", "?name"],
            "results": [
                [100, "Alice"],
                [200, "Bob"]
            ]
        });
        let entities = extract_result_entities(&json);
        assert!(entities.contains(&100));
        assert!(entities.contains(&200));
    }

    #[test]
    fn test_extract_result_entities_no_integers() {
        let json = serde_json::json!({
            "columns": ["?name"],
            "results": [
                ["Alice"],
                ["Bob"]
            ]
        });
        let entities = extract_result_entities(&json);
        assert!(entities.is_empty());
    }

    #[test]
    fn test_extract_result_entities_missing_results() {
        let json = serde_json::json!({"columns": ["?x"]});
        let entities = extract_result_entities(&json);
        assert!(entities.is_empty());
    }

    // ---- is_valid_db_name comprehensive tests ----

    #[test]
    fn test_valid_db_names() {
        assert!(is_valid_db_name("a"));
        assert!(is_valid_db_name("test_db"));
        assert!(is_valid_db_name("mydb123"));
        assert!(is_valid_db_name("A"));
        assert!(is_valid_db_name("MyDB"));
        assert!(is_valid_db_name("a_b_c_d"));
        assert!(is_valid_db_name("db1_test2_foo3"));
    }

    #[test]
    fn test_invalid_db_names() {
        assert!(!is_valid_db_name(""));
        assert!(!is_valid_db_name("123db")); // starts with digit
        assert!(!is_valid_db_name("_db")); // starts with underscore
        assert!(!is_valid_db_name("my-db")); // contains dash
        assert!(!is_valid_db_name("my db")); // contains space
        assert!(!is_valid_db_name("my.db")); // contains dot
        assert!(!is_valid_db_name("my;db")); // contains semicolon
        assert!(!is_valid_db_name("DROP TABLE")); // SQL injection attempt
    }

    #[test]
    fn test_db_name_max_length() {
        let name_63 = "a".repeat(63);
        assert!(is_valid_db_name(&name_63));

        let name_64 = "a".repeat(64);
        assert!(!is_valid_db_name(&name_64));
    }

    // ---- build_datoms_query comprehensive tests ----
    // These test the typed-column schema version of build_datoms_query, which:
    // - Always includes `WHERE added = true` (even for 0 components)
    // - Uses `value_type_tag` in ORDER BY instead of `v`
    // - Uses typed column names (v_ref) for VAET index

    #[test]
    fn test_build_datoms_query_eavt_all_components() {
        use crate::protocol::DatomsIndex;

        let q0 = build_datoms_query(DatomsIndex::EAVT, 0);
        assert!(q0.contains("FROM mentat.datoms"));
        assert!(q0.contains("WHERE added = true"));
        assert!(q0.contains("ORDER BY e, a, value_type_tag, tx"));

        let q1 = build_datoms_query(DatomsIndex::EAVT, 1);
        assert!(q1.contains("WHERE e = $1 AND added = true"));

        let q2 = build_datoms_query(DatomsIndex::EAVT, 2);
        assert!(q2.contains("WHERE e = $1 AND a = $2 AND added = true"));
    }

    #[test]
    fn test_build_datoms_query_aevt() {
        use crate::protocol::DatomsIndex;

        let q1 = build_datoms_query(DatomsIndex::AEVT, 1);
        assert!(q1.contains("WHERE a = $1 AND added = true"));
        assert!(q1.contains("ORDER BY a, e, value_type_tag, tx"));

        let q2 = build_datoms_query(DatomsIndex::AEVT, 2);
        assert!(q2.contains("WHERE a = $1 AND e = $2 AND added = true"));
    }

    #[test]
    fn test_build_datoms_query_avet() {
        use crate::protocol::DatomsIndex;

        let q1 = build_datoms_query(DatomsIndex::AVET, 1);
        assert!(q1.contains("WHERE a = $1 AND added = true"));
        assert!(q1.contains("ORDER BY a, value_type_tag, e, tx"));
    }

    #[test]
    fn test_build_datoms_query_vaet() {
        use crate::protocol::DatomsIndex;

        let q0 = build_datoms_query(DatomsIndex::VAET, 0);
        assert!(q0.contains("WHERE value_type_tag = 0 AND added = true"));
        assert!(q0.contains("ORDER BY v_ref, a, e, tx"));

        let q1 = build_datoms_query(DatomsIndex::VAET, 1);
        assert!(q1.contains("WHERE v_ref = $1"));

        let q2 = build_datoms_query(DatomsIndex::VAET, 2);
        assert!(q2.contains("WHERE v_ref = $1 AND a = $2"));
    }

    // ---- decode_typed_datom_value tests ----
    // NOTE: decode_typed_datom_value reads from tokio_postgres::Row (typed columns)
    // and cannot be unit-tested without a live database connection. These are
    // covered by integration tests instead.

    // ---- json_to_response_value edge cases ----

    #[test]
    fn test_json_to_response_value_object() {
        let val = serde_json::json!({"key1": "value1", "key2": 42});
        match json_to_response_value(&val) {
            ResponseValue::Map(entries) => {
                assert_eq!(entries.len(), 2);
            }
            other => panic!("Expected Map, got {:?}", other),
        }
    }

    #[test]
    fn test_json_to_response_value_nested_object() {
        let val = serde_json::json!({"outer": {"inner": 1}});
        match json_to_response_value(&val) {
            ResponseValue::Map(entries) => {
                assert_eq!(entries.len(), 1);
                match &entries[0].1 {
                    ResponseValue::Map(inner) => assert_eq!(inner.len(), 1),
                    other => panic!("Expected inner Map, got {:?}", other),
                }
            }
            other => panic!("Expected Map, got {:?}", other),
        }
    }

    #[test]
    fn test_json_to_response_value_large_u64() {
        // u64 value larger than i64::MAX should become a string
        let val = serde_json::json!(u64::MAX);
        match json_to_response_value(&val) {
            ResponseValue::String(s) => {
                assert_eq!(s, u64::MAX.to_string());
            }
            other => panic!("Expected String for large u64, got {:?}", other),
        }
    }

    #[test]
    fn test_json_to_response_value_float() {
        let val = serde_json::json!(3.14);
        match json_to_response_value(&val) {
            ResponseValue::Float(f) => {
                assert!((f - 3.14).abs() < 0.001);
            }
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    #[test]
    fn test_json_to_response_value_empty_array() {
        let val = serde_json::json!([]);
        match json_to_response_value(&val) {
            ResponseValue::Vector(items) => assert!(items.is_empty()),
            other => panic!("Expected empty Vector, got {:?}", other),
        }
    }

    #[test]
    fn test_json_to_response_value_empty_object() {
        let val = serde_json::json!({});
        match json_to_response_value(&val) {
            ResponseValue::Map(entries) => assert!(entries.is_empty()),
            other => panic!("Expected empty Map, got {:?}", other),
        }
    }

    // ---- build_filter_clause comprehensive tests ----

    #[test]
    fn test_build_filter_clause_attr_equals_with_colon() {
        let pred = FilterPredicate::AttrEquals(":person/name".to_string());
        let clause = build_filter_clause(&pred);
        assert!(clause.contains("person/name"));
        assert!(clause.contains("SELECT entid FROM mentat.idents"));
    }

    #[test]
    fn test_build_filter_clause_attr_equals_with_quotes() {
        let pred = FilterPredicate::AttrEquals("\":person/age\"".to_string());
        let clause = build_filter_clause(&pred);
        assert!(clause.contains("person/age"));
    }

    #[test]
    fn test_build_filter_clause_entity_equals_zero() {
        let pred = FilterPredicate::EntityEquals(0);
        let clause = build_filter_clause(&pred);
        assert_eq!(clause, "e = 0");
    }

    #[test]
    fn test_build_filter_clause_entity_equals_large() {
        let pred = FilterPredicate::EntityEquals(999999);
        let clause = build_filter_clause(&pred);
        assert_eq!(clause, "e = 999999");
    }

    #[test]
    fn test_build_filter_clause_since_zero() {
        let pred = FilterPredicate::Since(0);
        let clause = build_filter_clause(&pred);
        assert_eq!(clause, "tx > 0");
    }

    #[test]
    fn test_build_filter_clause_custom_always_returns_false() {
        // Custom predicates are disabled for security
        let pred = FilterPredicate::Custom("a = 10".to_string());
        let clause = build_filter_clause(&pred);
        assert_eq!(clause, "FALSE");
    }

    #[test]
    fn test_build_filter_clause_custom_blocks_injection() {
        let pred = FilterPredicate::Custom("a = 10; DROP TABLE users".to_string());
        let clause = build_filter_clause(&pred);
        assert_eq!(clause, "FALSE");
        assert!(!clause.contains("DROP"));
    }

    // ---- parse_query_results edge cases ----

    #[test]
    fn test_parse_query_results_single_column() {
        let json = serde_json::json!({
            "columns": ["?e"],
            "results": [[42], [43], [44]]
        });
        let result = parse_query_results(&json);
        assert!(result.is_ok());
        let rows = result.unwrap();
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn test_parse_query_results_keywords_in_results() {
        let json = serde_json::json!({
            "columns": ["?attr"],
            "results": [[":db/ident"], [":person/name"]]
        });
        let result = parse_query_results(&json);
        assert!(result.is_ok());
        let rows = result.unwrap();
        match &rows[0] {
            ResponseValue::Vector(vals) => match &vals[0] {
                ResponseValue::Keyword(k) => assert_eq!(k, "db/ident"),
                other => panic!("Expected Keyword, got {:?}", other),
            },
            other => panic!("Expected Vector, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_query_results_null_value() {
        let json = serde_json::json!({
            "columns": ["?v"],
            "results": [[null]]
        });
        let result = parse_query_results(&json);
        assert!(result.is_ok());
        let rows = result.unwrap();
        match &rows[0] {
            ResponseValue::Vector(vals) => {
                assert!(matches!(&vals[0], ResponseValue::Nil));
            }
            other => panic!("Expected Vector, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_query_results_not_array() {
        let json = serde_json::json!({
            "results": "not an array"
        });
        let result = parse_query_results(&json);
        assert!(result.is_err());
    }

    // ---- parse_tx_report edge cases ----

    #[test]
    fn test_parse_tx_report_empty_object() {
        let result = parse_tx_report("{}");
        assert!(result.is_ok());
        match result.unwrap() {
            ResponseValue::Map(entries) => assert!(entries.is_empty()),
            other => panic!("Expected empty Map, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_tx_report_with_all_fields() {
        let report = r#"{"db-before":{"basis-t":0},"db-after":{"basis-t":1},"tx-data":[],"tempids":{},"tx-id":1,"tx-instant":"2024-01-01T00:00:00Z","datoms-inserted":0}"#;
        let result = parse_tx_report(report);
        assert!(result.is_ok());
        match result.unwrap() {
            ResponseValue::Map(entries) => {
                // Should have both Datomic and legacy fields
                assert!(entries.len() >= 5);
            }
            other => panic!("Expected Map, got {:?}", other),
        }
    }

    // ---- AnomalyCategory tests ----

    #[test]
    fn test_anomaly_category_keywords() {
        use crate::protocol::AnomalyCategory;
        assert_eq!(
            AnomalyCategory::Incorrect.as_keyword(),
            ":cognitect.anomalies/incorrect"
        );
        assert_eq!(
            AnomalyCategory::Forbidden.as_keyword(),
            ":cognitect.anomalies/forbidden"
        );
        assert_eq!(
            AnomalyCategory::NotFound.as_keyword(),
            ":cognitect.anomalies/not-found"
        );
        assert_eq!(
            AnomalyCategory::Unavailable.as_keyword(),
            ":cognitect.anomalies/unavailable"
        );
        assert_eq!(
            AnomalyCategory::Interrupted.as_keyword(),
            ":cognitect.anomalies/interrupted"
        );
        assert_eq!(
            AnomalyCategory::Fault.as_keyword(),
            ":cognitect.anomalies/fault"
        );
    }

    // ---- ServerError to Anomaly conversion ----

    #[test]
    fn test_server_error_parse_to_anomaly() {
        use crate::protocol::parser::ParseError;
        let err: ServerError = ParseError::MissingField("test".to_string()).into();
        let anomaly: Anomaly = err.into();
        assert!(matches!(anomaly.category, crate::protocol::AnomalyCategory::Incorrect));
    }

    #[test]
    fn test_server_error_internal_to_anomaly() {
        let err = ServerError::Internal("something went wrong".to_string());
        let anomaly: Anomaly = err.into();
        assert!(matches!(anomaly.category, crate::protocol::AnomalyCategory::Fault));
        assert!(anomaly.message.contains("something went wrong"));
    }

    // ---- Security tests ----

    #[test]
    fn test_is_valid_db_name_blocks_injection() {
        // SQL injection via database name
        assert!(!is_valid_db_name("test; DROP TABLE users"));
        assert!(!is_valid_db_name("test--comment"));
        assert!(!is_valid_db_name("test' OR '1'='1"));
        assert!(!is_valid_db_name(""));
        assert!(!is_valid_db_name("1starts_with_number"));
        // Valid names
        assert!(is_valid_db_name("test_db"));
        assert!(is_valid_db_name("mentat"));
        assert!(is_valid_db_name("MyDB123"));
    }

    #[test]
    fn test_is_valid_attribute_ident_blocks_injection() {
        // SQL injection via attribute ident
        assert!(!is_valid_attribute_ident("'; DROP TABLE mentat.datoms; --"));
        assert!(!is_valid_attribute_ident("person/name' UNION SELECT * FROM pg_shadow --"));
        assert!(!is_valid_attribute_ident(""));
        // Oversized ident
        let long_ident = "a".repeat(257);
        assert!(!is_valid_attribute_ident(&long_ident));
        // Valid idents
        assert!(is_valid_attribute_ident("person/name"));
        assert!(is_valid_attribute_ident("db.type/string"));
        assert!(is_valid_attribute_ident("my-attr/some-name"));
    }

    #[test]
    fn test_escape_sql_string() {
        assert_eq!(escape_sql_string("hello"), "hello");
        assert_eq!(escape_sql_string("it's"), "it''s");
        assert_eq!(escape_sql_string("a''b"), "a''''b");
        assert_eq!(escape_sql_string("'; DROP TABLE users; --"), "''; DROP TABLE users; --");
    }

    #[test]
    fn test_quote_identifier() {
        assert_eq!(quote_identifier("test_db"), "\"test_db\"");
        assert_eq!(quote_identifier("test\"db"), "\"test\"\"db\"");
        assert_eq!(quote_identifier(""), "\"\"");
    }

    #[test]
    fn test_filter_clause_attr_injection_blocked() {
        // Attribute ident with SQL injection attempt should produce FALSE
        let pred = FilterPredicate::AttrEquals("person/name' OR 1=1 --".to_string());
        let clause = build_filter_clause(&pred);
        assert_eq!(clause, "FALSE");
    }

    #[test]
    fn test_filter_clause_attr_valid() {
        let pred = FilterPredicate::AttrEquals(":person/name".to_string());
        let clause = build_filter_clause(&pred);
        assert!(clause.contains("person/name"));
        assert!(!clause.contains("FALSE"));
    }

    #[test]
    fn test_body_size_limit_configured() {
        // Verify the body size limit constant is reasonable (not too large)
        assert!(MAX_BODY_SIZE <= 64 * 1024 * 1024, "Body limit should not exceed 64 MiB");
        assert!(MAX_BODY_SIZE >= 1024 * 1024, "Body limit should be at least 1 MiB");
    }

    #[test]
    fn test_constant_time_eq_identical() {
        assert!(constant_time_eq(b"secret-key-123", b"secret-key-123"));
    }

    #[test]
    fn test_constant_time_eq_different() {
        assert!(!constant_time_eq(b"secret-key-123", b"wrong-key-456"));
    }

    #[test]
    fn test_constant_time_eq_different_lengths() {
        assert!(!constant_time_eq(b"short", b"longer-string"));
    }

    #[test]
    fn test_constant_time_eq_empty() {
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn test_constant_time_eq_single_bit_diff() {
        // Differ by one bit in the last byte
        assert!(!constant_time_eq(b"abcA", b"abcB"));
    }

    // ---- Datomic transaction report format compliance tests ----

    #[test]
    fn test_tx_report_all_required_fields_present() {
        // Datomic clients require exactly these 4 fields in a transaction report
        let report = r#"{"db-before":{"basis-t":100},"db-after":{"basis-t":101},"tx-data":[[101,10,{"_t":"inst","v":1714000000000000},101,true]],"tempids":{}}"#;
        let result = parse_tx_report(report).unwrap();
        match result {
            ResponseValue::Map(entries) => {
                let keys: Vec<&str> = entries
                    .iter()
                    .filter_map(|(k, _)| match k {
                        ResponseValue::Keyword(s) => Some(s.as_str()),
                        _ => None,
                    })
                    .collect();
                assert!(keys.contains(&"db-before"), "Missing :db-before");
                assert!(keys.contains(&"db-after"), "Missing :db-after");
                assert!(keys.contains(&"tx-data"), "Missing :tx-data");
                assert!(keys.contains(&"tempids"), "Missing :tempids");
            }
            other => panic!("Expected Map, got {:?}", other),
        }
    }

    #[test]
    fn test_tx_report_basis_t_structure() {
        // db-before and db-after must be maps with :basis-t
        let report = r#"{"db-before":{"basis-t":999},"db-after":{"basis-t":1000},"tx-data":[],"tempids":{}}"#;
        let result = parse_tx_report(report).unwrap();
        match result {
            ResponseValue::Map(entries) => {
                // db-before
                let db_before = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "db-before")).unwrap();
                match &db_before.1 {
                    ResponseValue::Map(inner) => {
                        let bt = inner.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "basis-t")).unwrap();
                        assert!(matches!(&bt.1, ResponseValue::Integer(999)));
                    }
                    other => panic!("db-before should be Map, got {:?}", other),
                }
                // db-after
                let db_after = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "db-after")).unwrap();
                match &db_after.1 {
                    ResponseValue::Map(inner) => {
                        let bt = inner.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "basis-t")).unwrap();
                        assert!(matches!(&bt.1, ResponseValue::Integer(1000)));
                    }
                    other => panic!("db-after should be Map, got {:?}", other),
                }
            }
            _ => panic!("Expected Map"),
        }
    }

    #[test]
    fn test_tx_report_type_tagged_instant() {
        // Extension returns instants as {"_t":"inst","v":<micros>}
        let report = r#"{"db-before":{"basis-t":0},"db-after":{"basis-t":1},"tx-data":[[1,10,{"_t":"inst","v":1714000000000000},1,true]],"tempids":{}}"#;
        let result = parse_tx_report(report).unwrap();
        match result {
            ResponseValue::Map(entries) => {
                let tx_data = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "tx-data")).unwrap();
                match &tx_data.1 {
                    ResponseValue::Vector(datoms) => {
                        assert_eq!(datoms.len(), 1);
                        match &datoms[0] {
                            ResponseValue::Vector(d) => {
                                assert_eq!(d.len(), 5);
                                // The value (3rd element) should be Instant
                                match &d[2] {
                                    ResponseValue::Instant(micros) => {
                                        assert_eq!(*micros, 1714000000000000_i64);
                                    }
                                    other => panic!("Expected Instant for value, got {:?}", other),
                                }
                            }
                            other => panic!("Expected Vector for datom, got {:?}", other),
                        }
                    }
                    other => panic!("Expected Vector for tx-data, got {:?}", other),
                }
            }
            _ => panic!("Expected Map"),
        }
    }

    #[test]
    fn test_tx_report_type_tagged_uuid() {
        let report = r#"{"db-before":{"basis-t":0},"db-after":{"basis-t":1},"tx-data":[[1,20,{"_t":"uuid","v":"550e8400-e29b-41d4-a716-446655440000"},1,true]],"tempids":{}}"#;
        let result = parse_tx_report(report).unwrap();
        match result {
            ResponseValue::Map(entries) => {
                let tx_data = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "tx-data")).unwrap();
                match &tx_data.1 {
                    ResponseValue::Vector(datoms) => {
                        match &datoms[0] {
                            ResponseValue::Vector(d) => {
                                match &d[2] {
                                    ResponseValue::Uuid(u) => {
                                        assert_eq!(u, "550e8400-e29b-41d4-a716-446655440000");
                                    }
                                    other => panic!("Expected Uuid, got {:?}", other),
                                }
                            }
                            other => panic!("Expected Vector, got {:?}", other),
                        }
                    }
                    other => panic!("Expected Vector, got {:?}", other),
                }
            }
            _ => panic!("Expected Map"),
        }
    }

    #[test]
    fn test_tx_report_type_tagged_double() {
        let report = r#"{"db-before":{"basis-t":0},"db-after":{"basis-t":1},"tx-data":[[1,30,{"_t":"double","v":3.14},1,true]],"tempids":{}}"#;
        let result = parse_tx_report(report).unwrap();
        match result {
            ResponseValue::Map(entries) => {
                let tx_data = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "tx-data")).unwrap();
                match &tx_data.1 {
                    ResponseValue::Vector(datoms) => {
                        match &datoms[0] {
                            ResponseValue::Vector(d) => {
                                match &d[2] {
                                    ResponseValue::Float(f) => {
                                        assert!((f - 3.14).abs() < 0.001);
                                    }
                                    other => panic!("Expected Float, got {:?}", other),
                                }
                            }
                            other => panic!("Expected Vector, got {:?}", other),
                        }
                    }
                    other => panic!("Expected Vector, got {:?}", other),
                }
            }
            _ => panic!("Expected Map"),
        }
    }

    #[test]
    fn test_tx_report_tempids_map() {
        let report = r#"{"db-before":{"basis-t":0},"db-after":{"basis-t":1},"tx-data":[],"tempids":{"alice":5001,"bob":5002}}"#;
        let result = parse_tx_report(report).unwrap();
        match result {
            ResponseValue::Map(entries) => {
                let tempids = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "tempids")).unwrap();
                match &tempids.1 {
                    ResponseValue::Map(inner) => {
                        assert_eq!(inner.len(), 2);
                        // Datomic tempids map uses String keys (the tempid names), not Keywords.
                        // EDN: {"alice" 5001, "bob" 5002}
                        for (k, v) in inner {
                            assert!(matches!(k, ResponseValue::String(_)), "Tempid key should be String, got {:?}", k);
                            assert!(matches!(v, ResponseValue::Integer(_)));
                        }
                    }
                    other => panic!("Expected Map for tempids, got {:?}", other),
                }
            }
            _ => panic!("Expected Map"),
        }
    }

    #[test]
    fn test_tx_report_edn_serialization_complete() {
        use crate::protocol::serializer::serialize_response;
        use crate::protocol::Response;

        // Full report with all types including type-tagged values
        let report = r#"{"db-before":{"basis-t":100},"db-after":{"basis-t":101},"tx-data":[[101,10,{"_t":"inst","v":1714000000000000},101,true],[5001,20,"Alice",101,true]],"tempids":{"alice":5001}}"#;
        let result = parse_tx_report(report).unwrap();
        let response = Response::Success { result };
        let output = serialize_response(&response);

        // Verify the EDN output contains properly formatted fields
        assert!(output.contains(":db-before {:basis-t 100}"), "Missing db-before in: {}", output);
        assert!(output.contains(":db-after {:basis-t 101}"), "Missing db-after in: {}", output);
        assert!(output.contains(":tx-data"), "Missing tx-data in: {}", output);
        assert!(output.contains("#inst \""), "Missing #inst in tx-data: {}", output);
        assert!(output.contains(":tempids"), "Missing tempids in: {}", output);
    }

    #[test]
    fn test_tx_report_transit_json_serialization() {
        use crate::protocol::transit_serializer::serialize_transit_json;
        use crate::protocol::Response;

        let report = r#"{"db-before":{"basis-t":100},"db-after":{"basis-t":101},"tx-data":[[101,10,{"_t":"inst","v":1714000000000000},101,true]],"tempids":{}}"#;
        let result = parse_tx_report(report).unwrap();
        let response = Response::Success { result };
        let output = serialize_transit_json(&response);

        // Transit: instant should be ~m<millis>
        assert!(output.contains("~m1714000000000"), "Missing Transit instant ~m in: {}", output);
        // Transit: keywords should be ~:
        assert!(output.contains("~:db-before"), "Missing ~:db-before in: {}", output);
        assert!(output.contains("~:db-after"), "Missing ~:db-after in: {}", output);
        assert!(output.contains("~:tx-data"), "Missing ~:tx-data in: {}", output);
        assert!(output.contains("~:tempids"), "Missing ~:tempids in: {}", output);
    }

    #[test]
    fn test_tx_report_datom_five_tuple() {
        // Each datom in tx-data must be [e a v tx added]
        let report = r#"{"db-before":{"basis-t":0},"db-after":{"basis-t":1},"tx-data":[[5001,100,"Alice",1001,true],[5001,101,30,1001,true]],"tempids":{}}"#;
        let result = parse_tx_report(report).unwrap();
        match result {
            ResponseValue::Map(entries) => {
                let tx_data = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "tx-data")).unwrap();
                match &tx_data.1 {
                    ResponseValue::Vector(datoms) => {
                        assert_eq!(datoms.len(), 2);
                        for datom in datoms {
                            match datom {
                                ResponseValue::Vector(d) => {
                                    assert_eq!(d.len(), 5, "Each datom must be a 5-tuple [e a v tx added]");
                                    // e and a should be integers
                                    assert!(matches!(&d[0], ResponseValue::Integer(_)));
                                    assert!(matches!(&d[1], ResponseValue::Integer(_)));
                                    // tx should be integer
                                    assert!(matches!(&d[3], ResponseValue::Integer(_)));
                                    // added should be boolean
                                    assert!(matches!(&d[4], ResponseValue::Boolean(_)));
                                }
                                other => panic!("Expected datom Vector, got {:?}", other),
                            }
                        }
                    }
                    other => panic!("Expected Vector, got {:?}", other),
                }
            }
            _ => panic!("Expected Map"),
        }
    }

    #[test]
    fn test_json_to_response_value_type_tagged_instant() {
        let val = serde_json::json!({"_t": "inst", "v": 1714000000000000_i64});
        match json_to_response_value(&val) {
            ResponseValue::Instant(micros) => assert_eq!(micros, 1714000000000000),
            other => panic!("Expected Instant, got {:?}", other),
        }
    }

    #[test]
    fn test_json_to_response_value_type_tagged_uuid() {
        let val = serde_json::json!({"_t": "uuid", "v": "550e8400-e29b-41d4-a716-446655440000"});
        match json_to_response_value(&val) {
            ResponseValue::Uuid(u) => assert_eq!(u, "550e8400-e29b-41d4-a716-446655440000"),
            other => panic!("Expected Uuid, got {:?}", other),
        }
    }

    #[test]
    fn test_json_to_response_value_type_tagged_double() {
        let val = serde_json::json!({"_t": "double", "v": 2.718});
        match json_to_response_value(&val) {
            ResponseValue::Float(f) => assert!((f - 2.718).abs() < 0.001),
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    #[test]
    fn test_json_to_response_value_unknown_type_tag_is_map() {
        // Unknown _t values should fall through to normal Map handling
        let val = serde_json::json!({"_t": "unknown", "v": 42});
        match json_to_response_value(&val) {
            ResponseValue::Map(_) => {} // expected
            other => panic!("Expected Map for unknown type tag, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_tx_report_tempids_have_string_keys() {
        // Datomic tempids map uses string keys, not keyword keys.
        // EDN: {:tempids {"alice" 5001, "bob" 5002}}
        // NOT: {:tempids {:alice 5001, :bob 5002}}
        let report_str = r#"{"db-before":{"basis-t":0},"db-after":{"basis-t":1},"tx-data":[],"tempids":{"alice":5001,"bob":5002}}"#;
        let result = parse_tx_report(report_str).unwrap();

        match result {
            ResponseValue::Map(entries) => {
                let tempids = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "tempids"));
                assert!(tempids.is_some(), "Missing :tempids");
                match &tempids.unwrap().1 {
                    ResponseValue::Map(inner) => {
                        assert_eq!(inner.len(), 2);
                        // All keys must be String, not Keyword
                        for (k, v) in inner {
                            match k {
                                ResponseValue::String(_) => {}
                                other => panic!("Tempid key should be String, got {:?}", other),
                            }
                            match v {
                                ResponseValue::Integer(_) => {}
                                other => panic!("Tempid value should be Integer, got {:?}", other),
                            }
                        }
                    }
                    other => panic!("Expected Map for tempids, got {:?}", other),
                }
            }
            other => panic!("Expected Map, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_tx_report_tempids_serialized_as_strings() {
        // Verify the full EDN serialization uses quoted strings for tempid keys
        use crate::protocol::serializer::serialize_response;
        use crate::protocol::Response;

        let report_str = r#"{"db-before":{"basis-t":0},"db-after":{"basis-t":1},"tx-data":[],"tempids":{"alice":5001}}"#;
        let result = parse_tx_report(report_str).unwrap();
        let response = Response::Success { result };
        let output = serialize_response(&response);

        // tempids should be {"alice" 5001}, not {:alice 5001}
        assert!(
            output.contains(r#""alice" 5001"#),
            "Tempids should have string keys in EDN, got: {}",
            output
        );
        assert!(
            !output.contains(":alice"),
            "Tempids should NOT have keyword keys in EDN, got: {}",
            output
        );
    }

    #[test]
    fn test_parse_tx_report_complete_datomic_structure() {
        // End-to-end test: verify the full transaction report structure matches Datomic format
        use crate::protocol::serializer::serialize_response;
        use crate::protocol::Response;

        let report_str = r#"{"db-before":{"basis-t":1000},"db-after":{"basis-t":1001},"tx-data":[[1001,10,{"_t":"inst","v":1714052800000000},1001,true],[5001,100,"Alice",1001,true]],"tempids":{"person-1":5001}}"#;
        let result = parse_tx_report(report_str).unwrap();
        let response = Response::Success { result };
        let output = serialize_response(&response);

        // Verify all 4 required Datomic fields
        assert!(output.contains(":db-before"), "Missing :db-before in: {}", output);
        assert!(output.contains(":db-after"), "Missing :db-after in: {}", output);
        assert!(output.contains(":tx-data"), "Missing :tx-data in: {}", output);
        assert!(output.contains(":tempids"), "Missing :tempids in: {}", output);

        // Verify db-before/db-after structure
        assert!(output.contains(":db-before {:basis-t 1000}"), "Wrong db-before in: {}", output);
        assert!(output.contains(":db-after {:basis-t 1001}"), "Wrong db-after in: {}", output);

        // Verify instant value is serialized as #inst
        assert!(output.contains("#inst \""), "Missing #inst in tx-data: {}", output);

        // Verify tempids use string keys
        assert!(output.contains(r#""person-1" 5001"#), "Tempids should use string keys: {}", output);
    }
}
