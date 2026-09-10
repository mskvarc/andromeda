//! Serving a `@context` document to the broker for the duration of one test.
//!
//! A test that needs the broker to resolve a user `@context` has to host one,
//! name it in a `Link` header, and then remove the `"Cached"` row the resolver
//! writes for it: `TestEnvironment` cleans by identifier prefix and a cached
//! context row is keyed by the origin URL instead, so it would outlive the run
//! that caused it (clause 5.13.1 makes the cached Context a resource of its
//! own).
//!
//! Every suite that hosts a context does the same three things, so they are
//! written once here rather than per module.

#![allow(dead_code, reason = "each suite uses the part of the helper its own tests need")]

use andromeda_test_mocks::{callback_path::CallbackPath, jsonld_context_server::JsonLdContextServer, server_config::MockServerConfig};
use axum::{body::Bytes, http::HeaderMap};
use sqlx::PgPool;
use uuid::Uuid;

/// Start a context origin on an ephemeral port.
///
/// # Panics
///
/// Panics when the callback configuration is absent or the listener does not
/// bind, neither of which a test can proceed past.
pub async fn start_origin() -> JsonLdContextServer {
    JsonLdContextServer::start(MockServerConfig::from_environment().expect("callback host"))
        .await
        .expect("context server starts")
}

/// Serve one `@context` document under a path unique to this run, and answer
/// the path it is served at together with the URL the broker fetches it from.
///
/// # Panics
///
/// Panics when the path is not absolute or the route cannot be configured.
pub async fn serve(server: &JsonLdContextServer, label: &str, document: &str, headers: HeaderMap) -> (CallbackPath, String) {
    let path = CallbackPath::parse(format!("/{label}-{}.jsonld", Uuid::new_v4())).expect("absolute context path");
    let url = server
        .serve(path.clone(), Bytes::from(document.to_owned()), headers) // clone: the caller keeps the path, the server keeps its own
        .await
        .expect("context route must be configured")
        .to_string();
    (path, url)
}

/// The `Link` header value naming one hosted `@context` (clause 5.5.7).
#[must_use]
pub fn context_link(url: &str) -> String {
    format!(r#"<{url}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#)
}

/// Drop the `"Cached"` rows the resolver wrote for the served contexts.
///
/// # Panics
///
/// Panics when a row cannot be removed, which would leave the next run reading
/// a context this one hosted.
pub async fn forget_cached(pool: &PgPool, urls: &[String]) {
    for url in urls {
        sqlx::query("DELETE FROM jsonld_contexts WHERE url = $1")
            .bind(url)
            .execute(pool)
            .await
            .expect("the cached context row must be removable");
    }
}
