//! Axum-based Context Source mock used by distributed-operation tests.
//!
//! Migrated from `tests/distop_entity/helpers.rs:52-172`. Every `distop_*`
//! suite consumes this single implementation rather than reimplementing
//! the stub per module.

use crate::config::callback_url;
use axum::{Router, extract::Request, response::IntoResponse};
use dashmap::DashMap;
use reqwest::Method;
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::oneshot;

struct StubEntry {
    status_code: u16,
    body: Option<String>,
    content_type: String,
    count: AtomicUsize,
}

/// The registered stubs, keyed by the method and path they answer.
///
/// A `DashMap` rather than a locked map: registration and lookup take only a
/// shard lock, so two suites registering stubs on the same mock never contend
/// on one writer's guard. The request method is kept as a typed
/// [`reqwest::Method`] rather than a stringified copy.
type StubMap = DashMap<(Method, String), StubEntry>;

/// Context Source stub. Registers method/path stubs returning canned status
/// codes and optional bodies, and counts how many requests each stub served.
pub struct ContextSourceMock {
    port: u16,
    stubs: Arc<StubMap>,
    total_count: Arc<AtomicUsize>,
    shutdown: Option<oneshot::Sender<()>>,
}

impl ContextSourceMock {
    /// Bind to an ephemeral port and spawn the axum task.
    pub async fn start() -> ContextSourceMock {
        let stubs: Arc<StubMap> = Arc::new(DashMap::new());
        let total_count = Arc::new(AtomicUsize::new(0));
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let handler_stubs = stubs.clone();
        let handler_total = total_count.clone();
        let app = Router::new().fallback(move |request: Request| {
            let stubs = handler_stubs.clone();
            let total_count = handler_total.clone();
            async move {
                let method = request.method().clone();
                let path = request.uri().path().to_string();
                total_count.fetch_add(1, Ordering::Relaxed);
                match stubs.get(&(method, path)) {
                    Some(entry) => {
                        entry.count.fetch_add(1, Ordering::Relaxed);
                        let status = axum::http::StatusCode::from_u16(entry.status_code).unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
                        match entry.body.as_ref() {
                            Some(body) => (status, [(axum::http::header::CONTENT_TYPE, entry.content_type.clone())], body.clone()).into_response(),
                            None => status.into_response(),
                        }
                    }
                    None => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                }
            }
        });

        let listener = match tokio::net::TcpListener::bind("0.0.0.0:0").await {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("ContextSourceMock failed to bind ephemeral port: {error}");
                std::process::exit(2);
            }
        };
        let port = match listener.local_addr() {
            Ok(address) => address.port(),
            Err(error) => {
                eprintln!("ContextSourceMock could not read local address: {error}");
                std::process::exit(2);
            }
        };

        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });

        ContextSourceMock {
            port,
            stubs,
            total_count,
            shutdown: Some(shutdown_tx),
        }
    }

    /// Register a stub that returns `status_code` with an empty body.
    pub async fn set_stub(&self, method: Method, path: &str, status_code: u16) {
        self.stubs.insert(
            (method, path.to_owned()),
            StubEntry {
                status_code,
                body: None,
                content_type: "application/json".to_owned(),
                count: AtomicUsize::new(0),
            },
        );
    }

    /// Register a stub that returns `status_code` and a JSON body.
    pub async fn set_stub_with_body(&self, method: Method, path: &str, status_code: u16, body: &Value) {
        let serialized = match serde_json::to_string(body) {
            Ok(serialized) => serialized,
            Err(error) => {
                eprintln!("ContextSourceMock could not serialize stub body: {error}");
                std::process::exit(2);
            }
        };
        self.stubs.insert(
            (method, path.to_owned()),
            StubEntry {
                status_code,
                body: Some(serialized),
                content_type: "application/ld+json".to_owned(),
                count: AtomicUsize::new(0),
            },
        );
    }

    /// Register a stub that returns a raw string body with a caller-chosen
    /// content type.
    pub async fn set_stub_with_raw_body(&self, method: Method, path: &str, status_code: u16, body: &str, content_type: &str) {
        self.stubs.insert(
            (method, path.to_owned()),
            StubEntry {
                status_code,
                body: Some(body.to_owned()),
                content_type: content_type.to_owned(),
                count: AtomicUsize::new(0),
            },
        );
    }

    /// Return the number of times the stub registered for `(method, path)`
    /// was served.
    pub async fn get_count(&self, method: Method, path: &str) -> usize {
        self.stubs
            .get(&(method, path.to_owned()))
            .map_or(0, |entry| entry.count.load(Ordering::Relaxed))
    }

    /// Total number of requests the mock saw, including unmatched fallbacks.
    pub async fn get_total_count(&self) -> usize {
        self.total_count.load(Ordering::Relaxed)
    }

    /// Resolve the external URL the broker will dial, built from
    /// [`callback_host`](crate::config::callback_host) so the mock is
    /// reachable whether the broker runs in a container or on the host.
    pub fn url(&self, path: &str) -> String {
        callback_url(self.port, path)
    }

    /// Trigger graceful shutdown.
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for ContextSourceMock {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use crate::mock::context_source::ContextSourceMock;
    use reqwest::Method;

    #[tokio::test]
    async fn two_writers_register_stubs_concurrently_and_both_are_served() {
        let mock = ContextSourceMock::start().await;

        // Concurrent registration: the old single `RwLock` serialised these two
        // writers on one guard; a `DashMap` lets each take only its own shard.
        tokio::join!(mock.set_stub(Method::GET, "/alpha", 204), mock.set_stub(Method::GET, "/beta", 200));

        let client = reqwest::Client::new();
        let alpha = client
            .get(format!("http://127.0.0.1:{}/alpha", mock.port))
            .send()
            .await
            .expect("alpha reachable");
        let beta = client.get(format!("http://127.0.0.1:{}/beta", mock.port)).send().await.expect("beta reachable");

        assert_eq!(alpha.status().as_u16(), 204);
        assert_eq!(beta.status().as_u16(), 200);
        assert_eq!(mock.get_count(Method::GET, "/alpha").await, 1);
        assert_eq!(mock.get_count(Method::GET, "/beta").await, 1);
    }
}
