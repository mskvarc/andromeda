//! Negative route-inventory checks for removed binary-owned handlers.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_common::{client::NgsiLdClient, scope::TestScope};
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

/// The deleted favicon handler is absent both at the process root and under the API root.
#[tokio::test]
async fn info_favicon_removed_returns_404_at_every_former_location() {
    let scope = TestScope::new("info-favicon-removed");
    let client = NgsiLdClient::new(&scope).await;

    for path in ["/favicon.ico", "/ngsi-ld/v1/favicon.ico"] {
        let response = client.raw(Method::GET, path).send().await.expect("favicon request failed");
        assert_eq!(response.status, StatusCode::NOT_FOUND, "{path} must not resolve");
    }
}
