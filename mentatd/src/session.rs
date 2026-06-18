//! Session management for Datomic Client API protocol compatibility.
//!
//! Each client connection (whether HTTP or WebSocket) is associated with a
//! session identified by a UUID. Sessions track:
//! - The connection ID (maps to Datomic's connection concept)
//! - The database name being accessed
//! - Active database values (immutable snapshots)
//! - Session creation time and last-activity time for expiration
//!
//! Sessions are used by the Datomic Client API to maintain state across
//! multiple requests on the same connection, particularly for WebSocket
//! connections where a single TCP connection carries many operations.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use uuid::Uuid;

/// A session representing a client connection to a specific database.
#[derive(Debug, Clone)]
pub struct Session {
    /// Unique session identifier.
    pub id: Uuid,
    /// The database name this session is connected to.
    pub db_name: String,
    /// When the session was created.
    pub created_at: Instant,
    /// When the session was last used.
    pub last_activity: Instant,
    /// Active database value snapshots keyed by db-id string.
    /// Each maps to a basis-t value for point-in-time queries.
    pub db_snapshots: HashMap<String, i64>,
}

impl Session {
    /// Create a new session for the given database.
    pub fn new(db_name: String) -> Self {
        let now = Instant::now();
        Self {
            id: Uuid::new_v4(),
            db_name,
            created_at: now,
            last_activity: now,
            db_snapshots: HashMap::new(),
        }
    }

    /// Update the last-activity timestamp.
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Record a database snapshot for this session.
    pub fn add_snapshot(&mut self, db_id: String, basis_t: i64) {
        self.db_snapshots.insert(db_id, basis_t);
    }

    /// Look up a database snapshot by db-id.
    pub fn get_snapshot(&self, db_id: &str) -> Option<i64> {
        self.db_snapshots.get(db_id).copied()
    }

    /// Check if this session has expired based on the given TTL.
    pub fn is_expired(&self, ttl: Duration) -> bool {
        self.last_activity.elapsed() > ttl
    }
}

/// Thread-safe session store managing all active client sessions.
///
/// Sessions are automatically cleaned up when they exceed the configured TTL.
/// The default TTL is 30 minutes, matching Datomic's session timeout behavior.
pub struct SessionStore {
    sessions: RwLock<HashMap<Uuid, Session>>,
    ttl: Duration,
}

impl SessionStore {
    /// Create a new session store with the given TTL.
    pub fn new(ttl: Duration) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            ttl,
        }
    }

    /// Create a new session for the given database and return it.
    pub async fn create(&self, db_name: String) -> Session {
        let session = Session::new(db_name);
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id, session.clone());
        session
    }

    /// Look up a session by its UUID. Returns `None` if not found or expired.
    pub async fn get(&self, id: &Uuid) -> Option<Session> {
        let sessions = self.sessions.read().await;
        sessions.get(id).and_then(|s| {
            if s.is_expired(self.ttl) {
                None
            } else {
                Some(s.clone())
            }
        })
    }

    /// Update a session's last-activity timestamp.
    pub async fn touch(&self, id: &Uuid) -> bool {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(id) {
            if session.is_expired(self.ttl) {
                sessions.remove(id);
                return false;
            }
            session.touch();
            true
        } else {
            false
        }
    }

    /// Add a database snapshot to a session.
    pub async fn add_snapshot(&self, session_id: &Uuid, db_id: String, basis_t: i64) -> bool {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.add_snapshot(db_id, basis_t);
            true
        } else {
            false
        }
    }

    /// Remove an expired or explicitly closed session.
    pub async fn remove(&self, id: &Uuid) -> Option<Session> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(id)
    }

    /// Clean up all expired sessions. Returns the number removed.
    pub async fn cleanup_expired(&self) -> usize {
        let mut sessions = self.sessions.write().await;
        let ttl = self.ttl;
        let before = sessions.len();
        sessions.retain(|_, s| !s.is_expired(ttl));
        before - sessions.len()
    }

    /// Return the number of active (non-expired) sessions.
    pub async fn active_count(&self) -> usize {
        let sessions = self.sessions.read().await;
        let ttl = self.ttl;
        sessions.values().filter(|s| !s.is_expired(ttl)).count()
    }
}

/// Create a default session store with a 30-minute TTL.
pub fn default_session_store() -> Arc<SessionStore> {
    Arc::new(SessionStore::new(Duration::from_secs(30 * 60)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_session() {
        let store = SessionStore::new(Duration::from_secs(300));
        let session = store.create("test-db".to_string()).await;
        assert_eq!(session.db_name, "test-db");
        assert!(!session.id.is_nil());
    }

    #[tokio::test]
    async fn test_get_session() {
        let store = SessionStore::new(Duration::from_secs(300));
        let session = store.create("test-db".to_string()).await;
        let retrieved = store.get(&session.id).await;
        assert!(retrieved.is_some());
        assert_eq!(
            retrieved.as_ref().map(|s| &s.db_name),
            Some(&"test-db".to_string())
        );
    }

    #[tokio::test]
    async fn test_session_not_found() {
        let store = SessionStore::new(Duration::from_secs(300));
        let fake_id = Uuid::new_v4();
        assert!(store.get(&fake_id).await.is_none());
    }

    #[tokio::test]
    async fn test_session_expiration() {
        // Use a very short TTL
        let store = SessionStore::new(Duration::from_millis(1));
        let session = store.create("test-db".to_string()).await;

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(store.get(&session.id).await.is_none());
    }

    #[tokio::test]
    async fn test_session_touch() {
        let store = SessionStore::new(Duration::from_secs(300));
        let session = store.create("test-db".to_string()).await;
        assert!(store.touch(&session.id).await);
    }

    #[tokio::test]
    async fn test_session_remove() {
        let store = SessionStore::new(Duration::from_secs(300));
        let session = store.create("test-db".to_string()).await;
        let removed = store.remove(&session.id).await;
        assert!(removed.is_some());
        assert!(store.get(&session.id).await.is_none());
    }

    #[tokio::test]
    async fn test_cleanup_expired() {
        let store = SessionStore::new(Duration::from_millis(1));
        store.create("db1".to_string()).await;
        store.create("db2".to_string()).await;

        tokio::time::sleep(Duration::from_millis(10)).await;

        let removed = store.cleanup_expired().await;
        assert_eq!(removed, 2);
        assert_eq!(store.active_count().await, 0);
    }

    #[tokio::test]
    async fn test_session_snapshots() {
        let store = SessionStore::new(Duration::from_secs(300));
        let session = store.create("test-db".to_string()).await;

        store
            .add_snapshot(&session.id, "snap-1".to_string(), 1000)
            .await;

        let retrieved = store.get(&session.id).await;
        assert!(retrieved.is_some());
        let s = retrieved.as_ref().map(|s| s.get_snapshot("snap-1"));
        assert_eq!(s, Some(Some(1000)));
    }

    #[test]
    fn test_session_is_expired() {
        let mut session = Session::new("db".to_string());
        assert!(!session.is_expired(Duration::from_secs(300)));

        // Manually set last_activity to the past
        session.last_activity = Instant::now() - Duration::from_secs(400);
        assert!(session.is_expired(Duration::from_secs(300)));
    }
}
