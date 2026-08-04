//! HTTP client wrapper used by every integration test.
//!
//! [`NgsiLdClient`] is a thin layer on top of [`reqwest::Client`] that:
//! - carries the scope's `NGSILD-Tenant` header on every request,
//! - exposes per-endpoint builder-style request methods (create_entity,
//!   query_entities, create_subscription, …),
//! - returns a unified [`NgsiLdResponse`] so call sites can destructure
//!   `(status, headers, body)` uniformly regardless of the endpoint.

pub mod batch;
pub mod csr;
pub mod discovery;
pub mod entity;
pub mod entity_attribute;
pub mod entity_map;
pub mod jsonld_context;
pub mod raw;
pub mod snapshot;
pub mod subscription;
pub mod temporal;

use crate::{config::BASE_URL, scope::TestScope};
use reqwest::{Client, header::HeaderMap};
use serde_json::Value;
use std::sync::OnceLock;

/// HTTP response captured by every [`NgsiLdClient`] method.
#[derive(Debug)]
pub struct NgsiLdResponse {
    /// Response status.
    pub status: reqwest::StatusCode,
    /// Full response header map (multi-valued headers preserved).
    pub headers: HeaderMap,
    /// Response body decoded as JSON; `Value::Null` when the body was empty
    /// or not valid JSON.
    pub body: Value,
}

impl NgsiLdResponse {
    /// Return the value of the `Location` response header, if any.
    pub fn location(&self) -> Option<String> {
        self.headers.get("Location").and_then(|value| value.to_str().ok()).map(str::to_owned)
    }

    /// Convenience tuple accessor — many test bodies destructure
    /// `(status, headers, body)` in the legacy helpers.
    pub fn into_tuple(self) -> (reqwest::StatusCode, HeaderMap, Value) {
        (self.status, self.headers, self.body)
    }
}

/// HTTP client bound to a [`TestScope`].
#[derive(Debug, Clone)]
pub struct NgsiLdClient {
    http: Client,
    tenant: Option<String>,
}

impl NgsiLdClient {
    /// Construct a client for `scope`.
    ///
    /// Each `#[tokio::test]` runs in its own Tokio runtime and
    /// `reqwest::Client` binds its connection-pool dispatch task to the
    /// runtime that built it. Sharing one client across runtimes triggers
    /// `hyper::Error(User(DispatchGone))` when the first runtime exits.
    /// We therefore build a fresh client per `NgsiLdClient::new` call.
    ///
    /// The first `NgsiLdClient::new` in a process also issues a startup
    /// probe against `GET /info` so the test run fails fast with a clear
    /// message when the broker is not reachable, instead of hanging inside
    /// the first real request.
    pub async fn new(scope: &TestScope) -> NgsiLdClient {
        let http = build_http_client();
        run_startup_probe(&http).await;
        NgsiLdClient {
            http,
            tenant: scope.tenant().map(str::to_owned),
        }
    }

    /// Variant that does not carry any scope metadata. Used by the legacy
    /// helpers still under migration.
    pub fn bare() -> NgsiLdClient {
        NgsiLdClient {
            http: build_http_client(),
            tenant: None,
        }
    }

    /// Borrow the underlying `reqwest::Client`.
    pub const fn http(&self) -> &Client {
        &self.http
    }

    /// Tenant name bound to this client, if any.
    pub fn tenant(&self) -> Option<&str> {
        self.tenant.as_deref()
    }

    pub(crate) const fn base_url(&self) -> &'static str {
        BASE_URL
    }

    pub(crate) fn apply_tenant(&self, mut req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(tenant) = self.tenant.as_deref() {
            req = req.header("NGSILD-Tenant", tenant);
        }
        req
    }
}

fn build_http_client() -> Client {
    Client::new()
}

async fn run_startup_probe(http: &Client) {
    static PROBED: OnceLock<()> = OnceLock::new();
    if PROBED.get().is_some() {
        return;
    }
    let url = format!("{BASE_URL}/info");
    let result = http.get(url).timeout(std::time::Duration::from_secs(2)).send().await;
    match result {
        Ok(_) => {
            let _ = PROBED.set(());
        }
        Err(error) => panic_with_broker_unreachable(&error.to_string()),
    }
}

fn panic_with_broker_unreachable(error: &str) -> ! {
    eprintln!(
        "\nandromeda-test-common: broker at {BASE_URL} is not reachable. Start the broker before running integration tests.\n    underlying error: {error}\n"
    );
    std::process::exit(2);
}
