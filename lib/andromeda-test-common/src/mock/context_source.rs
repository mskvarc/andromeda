//! Axum-based Context Source mock used by distributed-operation tests.
//!
//! Migrated from `tests/distop_entity/helpers.rs:52-172`. Every `distop_*`
//! suite consumes this single implementation rather than reimplementing
//! the stub per module.

use crate::config::callback_url;
use axum::{Router, extract::Request, response::IntoResponse};
use reqwest::Method;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::{RwLock, oneshot};

struct StubEntry {
    status_code: u16,
    body: Option<String>,
    content_type: String,
    count: AtomicUsize,
}

struct MockState {
    stubs: HashMap<(String, String), StubEntry>,
    total_count: AtomicUsize,
}

/// Context Source stub. Registers method/path stubs returning canned status
/// codes and optional bodies, and counts how many requests each stub served.
pub struct ContextSourceMock {
    port: u16,
    state: Arc<RwLock<MockState>>,
    shutdown: Option<oneshot::Sender<()>>,
}

impl ContextSourceMock {
    /// Bind to an ephemeral port and spawn the axum task.
    pub async fn start() -> ContextSourceMock {
        let state = Arc::new(RwLock::new(MockState {
            stubs: HashMap::new(),
            total_count: AtomicUsize::new(0),
        }));
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let handler_state = state.clone();
        let app = Router::new().fallback(move |request: Request| {
            let state = handler_state.clone();
            async move {
                let method = request.method().to_string();
                let path = request.uri().path().to_string();
                let guard = state.read().await;
                guard.total_count.fetch_add(1, Ordering::Relaxed);
                let key = (method, path);
                match guard.stubs.get(&key) {
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
            state,
            shutdown: Some(shutdown_tx),
        }
    }

    /// Register a stub that returns `status_code` with an empty body.
    pub async fn set_stub(&self, method: Method, path: &str, status_code: u16) {
        let mut guard = self.state.write().await;
        guard.stubs.insert(
            (method.to_string(), path.to_owned()),
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
        let mut guard = self.state.write().await;
        let serialized = match serde_json::to_string(body) {
            Ok(serialized) => serialized,
            Err(error) => {
                eprintln!("ContextSourceMock could not serialize stub body: {error}");
                std::process::exit(2);
            }
        };
        guard.stubs.insert(
            (method.to_string(), path.to_owned()),
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
        let mut guard = self.state.write().await;
        guard.stubs.insert(
            (method.to_string(), path.to_owned()),
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
        let guard = self.state.read().await;
        guard
            .stubs
            .get(&(method.to_string(), path.to_owned()))
            .map_or(0, |entry| entry.count.load(Ordering::Relaxed))
    }

    /// Total number of requests the mock saw, including unmatched fallbacks.
    pub async fn get_total_count(&self) -> usize {
        let guard = self.state.read().await;
        guard.total_count.load(Ordering::Relaxed)
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
