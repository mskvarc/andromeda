//! The scheme the broker mints its own absolute URLs with.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.2: "the `apiRoot` includes the scheme
//! (`http` or `https`), host and optional port. The API shall support HTTP over
//! TLS (also known as HTTPS - see IETF RFC 2818 [18]). TLS version 1.2 as
//! defined by IETF RFC 5246 [19] shall be supported. HTTP without TLS is not
//! recommended."
//!
//! Andromeda meets that clause by terminating TLS at a reverse proxy
//! (`docs/tls-deployment.md`), which leaves the broker serving plaintext HTTP.
//! It previously hardcoded `http://` when reconstructing its own origin, so a
//! broker behind a terminator advertised `http://` URLs for an `https://`
//! deployment -- including the `@context` URL clause 5.13 has it store, which
//! outlives the request that created it.
//!
//! The fix reads `Forwarded` (IETF RFC 7239) and `X-Forwarded-Proto`, but only
//! when an operator has declared the edge trusted through
//! `ANDROMEDA_HTTP_FORWARDED_HEADERS`. Both fields are ordinary request fields,
//! so believing them by default would let any client choose the scheme of a
//! persisted `@context` URL. Every test here runs against the broker under
//! test; the trusted half additionally runs against
//! `ANDROMEDA_TEST_TRUSTED_PROXY_BROKER_URL` when that names a broker started
//! with the setting enabled.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{broker_endpoint::BrokerEndpoint, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::env;

/// Names a broker started with `ANDROMEDA_HTTP_FORWARDED_HEADERS=trusted`.
const TRUSTED_PROXY_BROKER_ENV: &str = "ANDROMEDA_TEST_TRUSTED_PROXY_BROKER_URL";

/// A `@context` payload the broker will host under an identifier of its own.
fn hosted_context() -> Value {
    json!({"@context": {"regressionForwardedProto": "https://example.org/regression/forwardedProto"}})
}

/// Create a hosted `@context` through `client` with the given forwarded fields,
/// read back the absolute URL the broker stored for it, and remove it again.
///
/// The stored URL is the durable consequence of the origin reconstruction:
/// clause 5.13 has the broker serve the `@context` at it, and every later
/// reader is handed that value.
async fn hosted_context_url(client: &TestClient, forwarded: &[(&str, &str)]) -> String {
    let mut request = client
        .raw(Method::POST, "jsonldContexts")
        .header("Content-Type", "application/json")
        .json(&hosted_context());
    for (name, value) in forwarded {
        request = request.header(name, *value);
    }
    let created = request.send().await.expect("create hosted @context failed");
    assert_eq!(created.status, StatusCode::CREATED, "create @context: {:?}", created.body);
    let location = created
        .location()
        .expect("a Location header must be valid text")
        .expect("a create answers with a Location")
        .to_owned();

    let details = client
        .raw(Method::GET, &format!("{location}?details=true"))
        .send()
        .await
        .expect("read hosted @context metadata failed");
    assert_eq!(details.status, StatusCode::OK, "read @context: {:?}", details.body);
    let body = details.json_value().expect("@context metadata must be valid JSON");
    assert!(body["URL"].is_string(), "a hosted @context created with a Host header carries a URL: {body}");
    let url = body["URL"].as_str().expect("asserted to be a string above").to_owned();

    let removed = client.raw(Method::DELETE, &location).send().await.expect("delete hosted @context failed");
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "delete @context: {:?}", removed.body);
    url
}

/// The broker declared trusted through [`TRUSTED_PROXY_BROKER_ENV`], if any.
fn trusted_proxy_client() -> Option<TestClient> {
    let configured = env::var(TRUSTED_PROXY_BROKER_ENV).ok().filter(|value| !value.is_empty())?;
    let endpoint = BrokerEndpoint::parse(configured).expect("a valid trusted-proxy broker URL");
    Some(TestClient::new(endpoint, None))
}

/// The security property, and the one that matters most: with the default
/// configuration a client-supplied forwarded field must not decide the scheme
/// of a URL the broker mints and stores.
#[tokio::test]
async fn regression_forwarded_protocol_is_ignored_without_a_trusted_proxy() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for forwarded in [
        vec![("X-Forwarded-Proto", "https")],
        vec![("Forwarded", "proto=https")],
        vec![("Forwarded", "for=192.0.2.60;proto=https;host=evil.example")],
        vec![("Forwarded", "proto=https"), ("X-Forwarded-Proto", "https")],
    ] {
        let url = hosted_context_url(client, &forwarded).await;
        assert!(
            url.starts_with("http://"),
            "an untrusted client must not talk the broker into minting an https URL with {forwarded:?}: {url}"
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// A broker with the setting on mints the scheme its terminator forwarded, so
/// clause 6.2's `apiRoot` matches the origin a client actually reached.
///
/// Runs against [`TRUSTED_PROXY_BROKER_ENV`] when it names such a broker. When
/// it does not, the same requests are made against the broker under test and
/// must still be refused, which re-asserts the security default rather than
/// skipping.
#[tokio::test]
async fn regression_forwarded_protocol_is_honoured_behind_a_trusted_proxy() {
    if let Some(trusted) = trusted_proxy_client() {
        let url = hosted_context_url(&trusted, &[("X-Forwarded-Proto", "https")]).await;
        assert!(url.starts_with("https://"), "a trusted edge decides the scheme: {url}");

        let plaintext = hosted_context_url(&trusted, &[("X-Forwarded-Proto", "http")]).await;
        assert!(
            plaintext.starts_with("http://"),
            "trusting the edge is not a blanket upgrade to https: {plaintext}"
        );
    } else {
        let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
        let url = hosted_context_url(scope.client(), &[("X-Forwarded-Proto", "https")]).await;
        assert!(
            url.starts_with("http://"),
            "{TRUSTED_PROXY_BROKER_ENV} is unset, so the broker under test must be the untrusted default: {url}"
        );
        scope.finish().await.expect("cleanup failed");
    }
}

/// RFC 7239 clause 1 makes `Forwarded` the standards-track replacement for the
/// `X-Forwarded-*` family, so at a trusted edge it decides. Runs only where a
/// trusted broker is available; on the untrusted default both fields are
/// ignored, which the security test above already asserts.
#[tokio::test]
async fn regression_the_rfc_7239_field_outranks_the_de_facto_one() {
    let Some(trusted) = trusted_proxy_client() else {
        return;
    };
    let url = hosted_context_url(&trusted, &[("Forwarded", "proto=https"), ("X-Forwarded-Proto", "http")]).await;
    assert!(url.starts_with("https://"), "Forwarded must win over X-Forwarded-Proto: {url}");

    let reversed = hosted_context_url(&trusted, &[("Forwarded", "proto=http"), ("X-Forwarded-Proto", "https")]).await;
    assert!(reversed.starts_with("http://"), "Forwarded must win over X-Forwarded-Proto: {reversed}");
}

/// A forwarded field the broker cannot read must cost the request nothing: the
/// scheme decides only how a minted URL is spelled and is never a precondition
/// of the operation.
#[tokio::test]
async fn regression_a_malformed_forwarded_field_does_not_fail_the_request() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for malformed in [
        vec![("Forwarded", "definitely not rfc 7239")],
        vec![("Forwarded", "proto=gopher")],
        vec![("Forwarded", ";;;")],
        vec![("X-Forwarded-Proto", "not-a-scheme")],
    ] {
        let url = hosted_context_url(client, &malformed).await;
        assert!(
            url.starts_with("http://"),
            "a field the broker cannot read leaves it on its listener scheme: {malformed:?} -> {url}"
        );
    }

    if let Some(trusted) = trusted_proxy_client() {
        for malformed in [
            vec![("Forwarded", "definitely not rfc 7239")],
            vec![("Forwarded", "proto=gopher")],
            vec![("X-Forwarded-Proto", "not-a-scheme")],
        ] {
            let url = hosted_context_url(&trusted, &malformed).await;
            assert!(
                url.starts_with("http://"),
                "even a trusted edge falls back safely on an unreadable field: {malformed:?} -> {url}"
            );
        }
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.12.3.1 and the pagination clauses put no origin in these fields, so
/// they carry no scheme to get wrong and are correct behind any terminator, at
/// any mount point. This pins that they stay relative rather than acquiring an
/// origin along with the forwarded-protocol handling.
#[tokio::test]
async fn regression_location_and_pagination_links_stay_relative_to_the_api_root() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let registration = json!({
        "id": scope.id("ContextSourceRegistration:relative-fields"),
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": "http://context-source.invalid:1026/",
        "@context": [CORE_CONTEXT]
    });
    let created = client
        .raw(Method::POST, "csourceRegistrations")
        .header("Content-Type", "application/ld+json")
        .header("X-Forwarded-Proto", "https")
        .json(&registration)
        .send()
        .await
        .expect("create registration failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    let location = created
        .location()
        .expect("a Location header must be valid text")
        .expect("a create answers with a Location");
    assert!(
        location.starts_with("/ngsi-ld/v1/csourceRegistrations/"),
        "a Location is root-relative and carries no origin: {location}"
    );

    let page = client
        .raw(Method::GET, "csourceRegistrations?type=Vehicle&limit=1")
        .header("X-Forwarded-Proto", "https")
        .send()
        .await
        .expect("registration discovery failed");
    assert_eq!(page.status, StatusCode::OK, "discovery: {:?}", page.body);
    let pagination_targets: Vec<&str> = page
        .headers
        .get_all("link")
        .into_iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter(|link| ["prev", "next", "first", "last"].iter().any(|rel| link.contains(&format!("rel=\"{rel}\""))))
        .filter_map(|link| link.split_once('<').and_then(|(_, rest)| rest.split_once('>')).map(|(target, _)| target))
        .collect();
    assert!(
        !pagination_targets.is_empty(),
        "a limited discovery emits pagination Link fields: {:?}",
        page.headers.get_all("link").into_iter().collect::<Vec<_>>()
    );
    for target in &pagination_targets {
        assert!(
            target.starts_with("/ngsi-ld/v1/csourceRegistrations"),
            "a pagination Link is path-only and carries no origin: {target}"
        );
    }

    scope.finish().await.expect("cleanup failed");
}
