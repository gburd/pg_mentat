use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::sleep;

pub struct TestServer {
    pub client: TestClient,
    _shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl TestServer {
    pub async fn start() -> Self {
        let config = mentatd::config::Config::from_env();

        let pool = mentatd::pool::create_pool(
            &config.database.connection_string,
            config.database.pool_size,
        )
        .unwrap_or_else(|e| {
            panic!("Failed to create connection pool: {}. Make sure PostgreSQL is running and DATABASE_URL is set correctly.", e);
        });

        let state = mentatd::server::AppState::new(pool, config.clone());
        let app = mentatd::server::create_router(state);

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind to random port");

        let addr = listener.local_addr().expect("Failed to get local addr");

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    shutdown_rx.await.ok();
                })
                .await
                .expect("Server failed");
        });

        // Wait for server to be ready
        sleep(Duration::from_millis(100)).await;

        let client = TestClient::new(addr);

        Self {
            client,
            _shutdown_tx: shutdown_tx,
        }
    }
}

#[derive(Clone)]
pub struct TestClient {
    base_url: String,
    client: reqwest::Client,
}

pub struct TestResponse {
    pub status: u16,
    pub body: String,
    pub content_type: Option<String>,
}

impl TestClient {
    fn new(addr: SocketAddr) -> Self {
        Self {
            base_url: format!("http://{}", addr),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    pub async fn get(&self, path: &str) -> TestResponse {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .expect("Failed to send GET request");

        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let body = response.text().await.expect("Failed to read response body");

        TestResponse {
            status,
            body,
            content_type,
        }
    }

    pub async fn post(&self, path: &str, body: &str) -> TestResponse {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/edn")
            .body(body.to_string())
            .send()
            .await
            .expect("Failed to send POST request");

        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let body = response.text().await.expect("Failed to read response body");

        TestResponse {
            status,
            body,
            content_type,
        }
    }

    pub async fn post_transit_json(&self, path: &str, body: &str) -> TestResponse {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/transit+json")
            .header("Accept", "application/transit+json")
            .body(body.to_string())
            .send()
            .await
            .expect("Failed to send Transit+JSON POST request");

        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let body = response.text().await.expect("Failed to read response body");

        TestResponse {
            status,
            body,
            content_type,
        }
    }

    pub async fn post_transit_msgpack(&self, path: &str, body: Vec<u8>) -> TestRawResponse {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/transit+msgpack")
            .header("Accept", "application/transit+msgpack")
            .body(body)
            .send()
            .await
            .expect("Failed to send Transit+MessagePack POST request");

        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let body = response.bytes().await.expect("Failed to read response body");

        TestRawResponse {
            status,
            body: body.to_vec(),
            content_type,
        }
    }
}

pub struct TestRawResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: Option<String>,
}
