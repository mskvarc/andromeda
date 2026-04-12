//! NGSI-LD context broker info integration tests.
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly -- no dependency
//! on the libtest-mimic harness or its fixtures module.
//!
//! Naming convention: `<operation>_<scenario>_<expected_outcome>`.

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport errors.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod source_identity;

use reqwest::header::HeaderMap;
use reqwest::{Client, StatusCode};
use serde_json::Value;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";

fn client() -> Client {
    Client::new()
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

/// GET /info/sourceIdentity with optional Accept header. Returns (status, headers, body).
async fn retrieve_source_identity(client: &Client, accept: Option<&str>) -> (StatusCode, HeaderMap, Value) {
    let mut req = client.get(format!("{BASE_URL}/info/sourceIdentity"));
    if let Some(accept) = accept {
        req = req.header("Accept", accept);
    }
    let resp = req.send().await.expect("retrieve_source_identity request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}
