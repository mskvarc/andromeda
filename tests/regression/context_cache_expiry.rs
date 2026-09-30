//! Expiration of cached `@contexts`, and what a stored `@context` keeps.
//!
//! Clause 6.3.16 makes the `Expires` header field (IETF RFC 7234 section 5.3)
//! and the `max-age`/`s-maxage` response directives of `Cache-Control`
//! (RFC 7234 section 5.2.2) of a downloaded `@context` authoritative over how
//! long the broker may reuse it, and clause 5.13.1 requires the entries flagged
//! `"Cached"` to be invalidated accordingly — periodically, on the broker's own
//! clock, not only when the next download happens along. Clause 5.13.2.3
//! governs the other direction: whatever a `@context` document carries outside
//! its `@context` subtree is discarded before anything is kept, whether a
//! client handed the document over or the resolver downloaded it. Clause
//! 5.13.5.4 governs the round trip: a reload replaces the stored copy with the
//! newly downloaded one, and refuses anything that is not a `Cached` entry.
//!
//! Each test drives one mock origin server whose path is unique to the run, so
//! a lifetime asserted here is the lifetime of that context alone, and removes
//! the `"Cached"` row it left behind — those rows are keyed by the origin URL
//! rather than by the run's identifier prefix, so the environment's own cleanup
//! cannot reach them.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::hosted_context::{forget_cached, serve};
use andromeda_test_client::response::TestResponse;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{callback_path::CallbackPath, jsonld_context_server::JsonLdContextServer, server_config::MockServerConfig};
use axum::http::{
    HeaderMap,
    HeaderValue,
    header::{CACHE_CONTROL, EXPIRES, HeaderName},
};
use chrono::{TimeDelta, Utc};
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::types::Json;
use std::{slice::from_ref, time::Duration};
use tokio::time::{Instant, sleep};
use urlencoding::encode;
use uuid::Uuid;

/// The context every test serves: two terms, enough for one entity to expand.
const CONTEXT_DOCUMENT: &[u8] = br#"{"@context":{"Device":"https://example.org/Device","temperature":"https://example.org/temperature"}}"#;

/// The same context, inside a document that also carries members clause
/// 5.13.2.3 requires be discarded.
const CONTEXT_DOCUMENT_WITH_EXTRAS: &[u8] = br#"{"@context":{"Device":"https://example.org/Device","temperature":"https://example.org/temperature"},"id":"urn:ngsi-ld:Context:from-the-origin-server","unrelated":[1,2,3]}"#;

/// How long a test waits before the request that must find the entry lapsed.
///
/// Comfortably longer than every one-second lifetime granted below, so a slow
/// broker is a slow test rather than a flaky one.
const PAST_EVERY_GRANTED_LIFETIME: Duration = Duration::from_secs(3);

/// How long a test will wait for the periodic invalidation pass to come round.
///
/// The pass runs on a cadence of its own, and a test joins that cadence at an
/// arbitrary phase, so the wait has to cover a full period and then some.
const PAST_THE_INVALIDATION_CADENCE: Duration = Duration::from_secs(90);

/// The `@context` subtree every served document carries, as the broker must
/// store it.
fn served_subtree() -> Value {
    json!({ "@context": { "Device": "https://example.org/Device", "temperature": "https://example.org/temperature" } })
}

/// An `Expires` value, formatted as the IMF-fixdate an HTTP-date must be.
fn expires_in(seconds: i64) -> String {
    let instant = Utc::now() + TimeDelta::try_seconds(seconds).expect("a small number of seconds is a valid delta");
    instant.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// Build a response header map, allowing a value that is not readable text: an
/// origin server may send one, and clause 6.3.16 still has to be applied.
fn response_headers<const N: usize>(entries: [(HeaderName, HeaderValue); N]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in entries {
        headers.append(name, value);
    }
    headers
}

fn cache_headers(name: HeaderName, value: &str) -> HeaderMap {
    response_headers([(name, HeaderValue::from_str(value).expect("a cache directive is a valid header value"))])
}

/// Serve `body` under a path unique to this run.
async fn serve_document(server: &JsonLdContextServer, label: &str, body: &'static [u8], headers: HeaderMap) -> (CallbackPath, String) {
    serve(server, &format!("cache-expiry-{label}"), &String::from_utf8_lossy(body), headers).await
}

/// Serve `CONTEXT_DOCUMENT` under a path unique to this run.
async fn serve_context(server: &JsonLdContextServer, label: &str, headers: HeaderMap) -> (CallbackPath, String) {
    serve_document(server, label, CONTEXT_DOCUMENT, headers).await
}

/// Create one entity that names `context_url`, forcing the broker to resolve it.
async fn create_entity_naming(scope: &TestEnvironment, context_url: &str, local_id: &str) {
    let entity = json!({
        "id": scope.id(local_id),
        "type": "Device",
        "temperature": { "type": "Property", "value": 21 },
        "@context": [context_url]
    });
    let created = scope.client().post_entity(&entity).send().await.expect("entity creation request failed");
    assert_eq!(created.status, StatusCode::CREATED, "entity creation must succeed: {:?}", created.body);
}

/// The `@context` document the broker holds under `context_url`, if any.
async fn cached_payload(scope: &TestEnvironment, context_url: &str) -> Option<Value> {
    let stored: Option<Json<Value>> = sqlx::query_scalar("SELECT payload FROM jsonld_contexts WHERE url = $1")
        .bind(context_url)
        .fetch_optional(scope.database_pool())
        .await
        .expect("the stored context row must be readable");
    stored.map(|payload| payload.0)
}

/// The local identifier the broker generated for the Context it cached from
/// `context_url` (clause 5.13.1).
///
/// The durable row is written after the resolve completes, so the listing is
/// polled rather than read once.
async fn cached_local_id(scope: &TestEnvironment, context_url: &str) -> Option<String> {
    for _ in 0..20 {
        let listed = scope
            .client()
            .raw(Method::GET, "jsonldContexts?kind=Cached&details=true")
            .send()
            .await
            .expect("listing contexts failed");
        assert_eq!(listed.status, StatusCode::OK, "listing contexts: {:?}", listed.body);
        let entries = listed.json_value().expect("the listing is JSON");
        if let Some(cached) = entries
            .as_array()
            .expect("the detailed listing is an array")
            .iter()
            .find(|entry| entry["URL"].as_str().is_some_and(|held| held == context_url))
            && let Some(found) = cached["localId"].as_str()
        {
            return Some(found.to_owned());
        }
        sleep(Duration::from_millis(250)).await;
    }
    None
}

/// Ask the broker to delete, or reload, one stored Context.
async fn remove_stored_context(scope: &TestEnvironment, identifier: &str, query: &str) -> TestResponse {
    scope
        .client()
        .raw(Method::DELETE, &format!("jsonldContexts/{}{query}", encode(identifier)))
        .send()
        .await
        .expect("removing the stored context failed")
}

/// Clause 6.3.16: an implementation "shall comply with … a max-age or s-maxage
/// response directive of Cache-Control header field … that may be present in the
/// downloaded @context", which clause 5.13.1 turns into invalidating the
/// `"Cached"` entry. A one-second `max-age` therefore governs, not the
/// deployment's `jsonld.cache_ttl_seconds`.
#[tokio::test]
async fn regression_context_cache_expiry_max_age_governs_the_entry_lifetime() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let (path, context_url) = serve_context(&server, "max-age", cache_headers(CACHE_CONTROL, "public, max-age=1")).await;

    create_entity_naming(&scope, &context_url, "Device:max-age-first").await;
    let after_first = server.request_count(&path);
    assert!(after_first >= 1, "the first entity must have made the broker download the context");

    sleep(PAST_EVERY_GRANTED_LIFETIME).await;
    create_entity_naming(&scope, &context_url, "Device:max-age-second").await;

    assert!(
        server.request_count(&path) > after_first,
        "a max-age of one second must expire the cached @context, not the configured TTL: {after_first} fetches before, {} after",
        server.request_count(&path)
    );

    forget_cached(scope.database_pool(), from_ref(&context_url)).await;
    server.shutdown().await.expect("context server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.16 names `s-maxage` alongside `max-age`; RFC 7234 section 5.2.2.9,
/// which it cites, gives `s-maxage` precedence over `max-age` for a shared
/// cache, and a broker caching an `@context` on behalf of every client it serves
/// is one. An hour of `max-age` beside a second of `s-maxage` must expire in a
/// second.
#[tokio::test]
async fn regression_context_cache_expiry_s_maxage_overrides_max_age() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let (path, context_url) = serve_context(&server, "s-maxage", cache_headers(CACHE_CONTROL, "public, max-age=3600, s-maxage=1")).await;

    create_entity_naming(&scope, &context_url, "Device:s-maxage-first").await;
    let after_first = server.request_count(&path);
    assert!(after_first >= 1, "the first entity must have made the broker download the context");

    sleep(PAST_EVERY_GRANTED_LIFETIME).await;
    create_entity_naming(&scope, &context_url, "Device:s-maxage-second").await;

    assert!(
        server.request_count(&path) > after_first,
        "s-maxage must override max-age for the shared cache a broker keeps: {after_first} fetches before, {} after",
        server.request_count(&path)
    );

    forget_cached(scope.database_pool(), from_ref(&context_url)).await;
    server.shutdown().await.expect("context server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// RFC 7234 section 5.2.2.9, which clause 6.3.16 cites, makes `s-maxage`
/// *override* `max-age` for a shared cache rather than merely tighten it. Every
/// other precedence case here puts the shorter span on `s-maxage`, which an
/// implementation taking the smaller of the two would also pass; this one puts
/// the longer span there, so only an implementation that actually prefers
/// `s-maxage` keeps the entry. The origin server said the shared copy lives an
/// hour, so the second entity must not re-download it.
#[tokio::test]
async fn regression_context_cache_expiry_s_maxage_governs_even_when_it_is_the_longer_span() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let (path, context_url) = serve_context(&server, "s-maxage-longer", cache_headers(CACHE_CONTROL, "public, max-age=1, s-maxage=3600")).await;

    create_entity_naming(&scope, &context_url, "Device:s-maxage-longer-first").await;
    let after_first = server.request_count(&path);
    assert!(after_first >= 1, "the first entity must have made the broker download the context");

    sleep(PAST_EVERY_GRANTED_LIFETIME).await;
    create_entity_naming(&scope, &context_url, "Device:s-maxage-longer-second").await;

    assert_eq!(
        server.request_count(&path),
        after_first,
        "s-maxage overrides max-age outright, so an hour on s-maxage must survive a second on max-age"
    );

    forget_cached(scope.database_pool(), from_ref(&context_url)).await;
    server.shutdown().await.expect("context server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.16: an implementation "shall comply with the Expires header field
/// (section 5.3 of IETF RFC 7234)". An `Expires` one second out, with no
/// `Cache-Control` beside it, is the whole of the entry's lifetime.
#[tokio::test]
async fn regression_context_cache_expiry_expires_header_alone_is_honoured() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let (path, context_url) = serve_context(&server, "expires", cache_headers(EXPIRES, &expires_in(1))).await;

    create_entity_naming(&scope, &context_url, "Device:expires-first").await;
    let after_first = server.request_count(&path);
    assert!(after_first >= 1, "the first entity must have made the broker download the context");

    sleep(PAST_EVERY_GRANTED_LIFETIME).await;
    create_entity_naming(&scope, &context_url, "Device:expires-second").await;

    assert!(
        server.request_count(&path) > after_first,
        "an Expires header with no Cache-Control beside it must still expire the entry: {after_first} fetches before, {} after",
        server.request_count(&path)
    );

    forget_cached(scope.database_pool(), from_ref(&context_url)).await;
    server.shutdown().await.expect("context server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// RFC 7234 section 4.2.1, which clause 6.3.16 binds, orders the two: a cache
/// computes freshness from `max-age` when it is present, and consults `Expires`
/// only when it is not. An hour of `max-age` beside an `Expires` one second out
/// therefore keeps the entry — an implementation that let the sooner of the two
/// win, or that read `Expires` first, would re-download.
#[tokio::test]
async fn regression_context_cache_expiry_max_age_overrides_a_conflicting_expires() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let headers = response_headers([
        (CACHE_CONTROL, HeaderValue::from_static("public, max-age=3600")),
        (EXPIRES, HeaderValue::from_str(&expires_in(1)).expect("an HTTP-date is a valid header value")),
    ]);
    let (path, context_url) = serve_context(&server, "max-age-over-expires", headers).await;

    create_entity_naming(&scope, &context_url, "Device:max-age-over-expires-first").await;
    let after_first = server.request_count(&path);
    assert!(after_first >= 1, "the first entity must have made the broker download the context");

    sleep(PAST_EVERY_GRANTED_LIFETIME).await;
    create_entity_naming(&scope, &context_url, "Device:max-age-over-expires-second").await;

    assert_eq!(
        server.request_count(&path),
        after_first,
        "max-age is consulted before Expires, so the hour it grants must survive a date one second out"
    );

    forget_cached(scope.database_pool(), from_ref(&context_url)).await;
    server.shutdown().await.expect("context server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.16 reads the `Cache-Control` header field of a downloaded
/// `@context`, and RFC 7234 section 3 forbids storing a response carrying
/// `no-store` at all. Nothing may be reused, so the very next request that names
/// the context downloads it again.
#[tokio::test]
async fn regression_context_cache_expiry_no_store_prevents_caching() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let (path, context_url) = serve_context(&server, "no-store", cache_headers(CACHE_CONTROL, "no-store")).await;

    create_entity_naming(&scope, &context_url, "Device:no-store-first").await;
    let after_first = server.request_count(&path);
    assert!(after_first >= 1, "the first entity must have made the broker download the context");

    create_entity_naming(&scope, &context_url, "Device:no-store-second").await;

    assert!(
        server.request_count(&path) > after_first,
        "no-store forbids storing the @context, so the next request must download it again: {after_first} fetches before, {} after",
        server.request_count(&path)
    );

    forget_cached(scope.database_pool(), from_ref(&context_url)).await;
    server.shutdown().await.expect("context server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.16 binds the `Cache-Control` field of a downloaded `@context`, and
/// a field is not only the values of it that happen to be readable text. An
/// origin server may send a value carrying bytes that are not text, and the
/// `no-store` beside it still forbids storing the document; discarding the whole
/// field on one bad byte, and then assigning the heuristic lifetime clause
/// 6.3.16 reserves for an origin server that stated nothing, cached for an hour
/// a document that was never to be stored at all.
#[tokio::test]
async fn regression_an_unreadable_cache_control_does_not_discard_no_store() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let headers = response_headers([
        (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        (
            CACHE_CONTROL,
            HeaderValue::from_bytes(b"private, note=\xff\xfe").expect("obs-text bytes are a valid header value"),
        ),
    ]);
    let (path, context_url) = serve_context(&server, "unreadable-cache-control", headers).await;

    create_entity_naming(&scope, &context_url, "Device:unreadable-first").await;
    let after_first = server.request_count(&path);
    assert!(after_first >= 1, "the first entity must have made the broker download the context");

    assert_eq!(
        cached_payload(&scope, &context_url).await,
        None,
        "no-store forbids storing the @context, whatever the value beside it looked like"
    );

    create_entity_naming(&scope, &context_url, "Device:unreadable-second").await;

    assert!(
        server.request_count(&path) > after_first,
        "nothing may be reused, so the next request must download the context again: {after_first} fetches before, {} after",
        server.request_count(&path)
    );

    forget_cached(scope.database_pool(), from_ref(&context_url)).await;
    server.shutdown().await.expect("context server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// RFC 7234 section 5.3, which clause 6.3.16 binds, requires a cache recipient
/// to "interpret invalid date formats, especially the value `0`, as
/// representing a time in the past (i.e. 'already expired')". `Expires: 0` is
/// the conventional spelling of that, so the document is stale on arrival and is
/// never stored — it is not an origin server that stated nothing, and it does
/// not earn the heuristic lifetime of clause 6.3.16.
#[tokio::test]
async fn regression_an_expires_of_zero_is_already_expired() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let (path, context_url) = serve_context(&server, "expires-zero", cache_headers(EXPIRES, "0")).await;

    create_entity_naming(&scope, &context_url, "Device:expires-zero-first").await;
    let after_first = server.request_count(&path);
    assert!(after_first >= 1, "the first entity must have made the broker download the context");

    assert_eq!(
        cached_payload(&scope, &context_url).await,
        None,
        "a document that was already expired on arrival is never stored"
    );

    create_entity_naming(&scope, &context_url, "Device:expires-zero-second").await;

    assert!(
        server.request_count(&path) > after_first,
        "an already-expired document must be downloaded again for the next request: {after_first} fetches before, {} after",
        server.request_count(&path)
    );

    forget_cached(scope.database_pool(), from_ref(&context_url)).await;
    server.shutdown().await.expect("context server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.16: "Since origin servers do not always provide explicit
/// expiration times, implementations should assign a heuristic … expiration time
/// when an explicit time is not specified." Andromeda's heuristic is the
/// configured `jsonld.cache_ttl_seconds` (an hour by default), so a context
/// served with no directives at all stays cached across a short wait.
#[tokio::test]
async fn regression_context_cache_expiry_no_directives_falls_back_to_the_configured_ttl() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let (path, context_url) = serve_context(&server, "no-directives", HeaderMap::new()).await;

    create_entity_naming(&scope, &context_url, "Device:no-directives-first").await;
    let after_first = server.request_count(&path);
    assert!(after_first >= 1, "the first entity must have made the broker download the context");

    sleep(PAST_EVERY_GRANTED_LIFETIME).await;
    create_entity_naming(&scope, &context_url, "Device:no-directives-second").await;

    assert_eq!(
        server.request_count(&path),
        after_first,
        "a response stating no expiration time must fall back to the configured lifetime, not re-download"
    );

    forget_cached(scope.database_pool(), from_ref(&context_url)).await;
    server.shutdown().await.expect("context server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.13.1 and clause 6.3.16 both require that "Implementations shall
/// periodically invalidate the "Cached" @contexts." Periodically means on the
/// broker's own clock: a broker that downloads nothing more still has to
/// invalidate what it already holds, so a lapsed entry must disappear without
/// another cache write happening anywhere.
#[tokio::test]
async fn regression_cached_contexts_are_periodically_invalidated() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let (_path, context_url) = serve_context(&server, "invalidated", cache_headers(CACHE_CONTROL, "public, max-age=1")).await;

    create_entity_naming(&scope, &context_url, "Device:invalidated").await;
    assert!(
        cached_payload(&scope, &context_url).await.is_some(),
        "resolving the remote context must leave a Cached entry to invalidate"
    );

    // Nothing else is asked of the broker from here on: the entry has to lapse
    // because the pass came round, not because another download replaced it.
    let deadline = Instant::now() + PAST_THE_INVALIDATION_CADENCE;
    let mut invalidated = false;
    while Instant::now() < deadline {
        if cached_payload(&scope, &context_url).await.is_none() {
            invalidated = true;
            break;
        }
        sleep(Duration::from_secs(2)).await;
    }

    assert!(
        invalidated,
        "a Cached @context whose max-age of one second has passed must be invalidated by the periodic pass"
    );

    forget_cached(scope.database_pool(), from_ref(&context_url)).await;
    server.shutdown().await.expect("context server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.13.2.3: the input to Add @context is "a JSON object that has a
/// top-level field named @context", and "all extra information located outside
/// of the @context subtree in the referenced object shall be discarded". What
/// clause 5.13.4 later serves back must therefore carry the subtree and nothing
/// else.
#[tokio::test]
async fn regression_context_cache_expiry_added_context_discards_members_outside_the_subtree() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let subtree = json!({ "Device": "https://example.org/Device", "temperature": "https://example.org/temperature" });
    let submitted = json!({
        "@context": subtree,
        "id": "urn:ngsi-ld:Context:supplied-by-the-client",
        "kind": "Hosted",
        "createdAt": "2020-01-01T00:00:00Z",
        "extraInfo": { "owner": "someone" },
        "unrelated": [1, 2, 3]
    });

    let created = client.create_jsonld_context(&submitted).await.expect("create context request failed");
    assert_eq!(created.status, StatusCode::CREATED, "hosted context creation must succeed: {:?}", created.body);
    let location = created
        .location()
        .expect("Location header must contain valid text")
        .expect("a Location header naming the hosted context");
    let context_id = location.rsplit('/').next().expect("the Location names a context id").to_owned();

    let payload_response = client.retrieve_jsonld_context(&context_id).await.expect("retrieve context request failed");
    assert_eq!(
        payload_response.status,
        StatusCode::OK,
        "a Hosted context is served on demand: {:?}",
        payload_response.body
    );
    let document = payload_response.json_value().expect("the served context is JSON");

    assert_eq!(
        document,
        json!({ "@context": subtree }),
        "everything outside the @context subtree must have been discarded before the document was stored"
    );

    let deleted = client.delete_jsonld_context(&context_id).await.expect("delete context request failed");
    assert_eq!(
        deleted.status,
        StatusCode::NO_CONTENT,
        "the hosted context must be removable: {:?}",
        deleted.body
    );
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.13.2.3 requires that "all extra information located outside of the
/// @context subtree in the referenced object shall be discarded", and a
/// downloaded `@context` document is such a referenced object. The rule was
/// applied where a client hands a document over and skipped where the resolver
/// downloads one, so a cached entry kept whatever else the origin server's file
/// happened to carry.
#[tokio::test]
async fn regression_a_cached_document_discards_members_outside_the_context_subtree() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let (_path, context_url) = serve_document(&server, "cached-extras", CONTEXT_DOCUMENT_WITH_EXTRAS, HeaderMap::new()).await;

    create_entity_naming(&scope, &context_url, "Device:cached-extras").await;

    assert_eq!(
        cached_payload(&scope, &context_url).await,
        Some(served_subtree()),
        "a downloaded @context is stored as its subtree alone, not as the whole file the origin server served"
    );

    forget_cached(scope.database_pool(), from_ref(&context_url)).await;
    server.shutdown().await.expect("context server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.13.5.4: with reload true on a `Cached` `@context`, "the existing
/// @context is replaced with the newly downloaded one." Replaced, not nested:
/// the broker used to store the download inside a second `@context` member, and
/// because clause 5.13.4.4 never serves a `Cached` entry and clause 5.13.5 has
/// no operation that repairs one, the stored document became permanently
/// unusable — every later request naming that URL answered `BadRequestData`.
#[tokio::test]
async fn regression_reloading_a_cached_context_replaces_it_with_the_download() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let (_path, context_url) = serve_context(&server, "reload", HeaderMap::new()).await;

    create_entity_naming(&scope, &context_url, "Device:reload-first").await;
    let local_id = cached_local_id(&scope, &context_url)
        .await
        .expect("resolving the remote context must leave a Cached entry");

    let reloaded = remove_stored_context(&scope, &local_id, "?reload=true").await;
    assert_eq!(
        reloaded.status,
        StatusCode::NO_CONTENT,
        "reloading a Cached @context from a reachable origin succeeds: {:?}",
        reloaded.body
    );

    assert_eq!(
        cached_payload(&scope, &context_url).await,
        Some(served_subtree()),
        "the stored @context is the newly downloaded one, not the download wrapped in another @context"
    );

    // The proof that matters: the reloaded entry still resolves. A doubly
    // wrapped document expands to nothing usable and refuses every entity.
    create_entity_naming(&scope, &context_url, "Device:reload-second").await;

    forget_cached(scope.database_pool(), from_ref(&context_url)).await;
    server.shutdown().await.expect("context server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.13.5.4: "If the @context identifier does not correspond to any
/// existing entry in the @context storage, then an error of type
/// `ResourceNotFound` shall be raised." That holds with reload true as much as
/// without it — an identifier naming nothing has no kind for the reload rule to
/// object to, so answering `BadRequestData` reported the wrong failure.
#[tokio::test]
async fn regression_reloading_an_unknown_context_is_resource_not_found() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let unknown = format!("urn:ngsi-ld:JsonLdContext:{}", Uuid::new_v4());

    let refused = remove_stored_context(&scope, &unknown, "?reload=true").await;

    assert_eq!(
        refused.status,
        StatusCode::NOT_FOUND,
        "an identifier that names no stored @context is ResourceNotFound: {:?}",
        refused.body
    );
    let problem = refused.json_value().expect("the error body is JSON");
    assert_eq!(
        problem["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound",
        "the ProblemDetails type is the clause 5.13.5.4 error: {problem}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.13.5.4: "If reload is true and the kind of the @context is not
/// "Cached", implementations shall return a `BadRequestData` error." The
/// embedded core `@context` is surfaced as `ImplicitlyCreated` (clause 5.13.1),
/// so it is not reloadable; answering `204` claimed a re-download that never
/// happened and could not happen.
#[tokio::test]
async fn regression_reloading_the_core_context_is_bad_request_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");

    let refused = remove_stored_context(&scope, "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld", "?reload=true").await;

    assert_eq!(
        refused.status,
        StatusCode::BAD_REQUEST,
        "only a Cached @context can be reloaded: {:?}",
        refused.body
    );
    let problem = refused.json_value().expect("the error body is JSON");
    assert_eq!(
        problem["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
        "the ProblemDetails type is the clause 5.13.5.4 error: {problem}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.13.4.4: serving the full content of an `@context` from internal
/// storage is defined for the `Hosted` and `ImplicitlyCreated` kinds; for a
/// `Cached` one the broker "shall give back `OperationNotSupported` error".
/// Clause 5.13.1 gives the reason — a `"Cached"` entry is a local performance
/// copy of someone else's document and is never served on demand. The broker
/// previously served it, republishing a third party's document under its own
/// identifier.
#[tokio::test]
async fn regression_a_cached_context_is_not_served_on_demand() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let (_path, context_url) = serve_context(&server, "not-served", HeaderMap::new()).await;

    // Resolving the remote context is what makes the broker cache it.
    create_entity_naming(&scope, &context_url, "Device:cached-not-served").await;
    let local_id = cached_local_id(&scope, &context_url)
        .await
        .expect("resolving the remote context must leave a Cached entry");

    // details=false asks for the document itself, which is the projection the
    // clause restricts to Hosted and ImplicitlyCreated contexts.
    let payload_response = scope
        .client()
        .raw(Method::GET, &format!("jsonldContexts/{local_id}"))
        .send()
        .await
        .expect("serving the cached context failed");
    assert_eq!(
        payload_response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "clause 5.13.4.4: a Cached @context answers OperationNotSupported (422), not its content: {:?}",
        payload_response.body
    );
    let problem = payload_response.json_value().expect("the error body is JSON");
    assert_eq!(
        problem["type"], "https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported",
        "the ProblemDetails type is the clause 5.13.4.4 error: {problem}"
    );

    // details=true is metadata, which the clause leaves unrestricted.
    let metadata = scope
        .client()
        .raw(Method::GET, &format!("jsonldContexts/{local_id}?details=true"))
        .send()
        .await
        .expect("reading cached context metadata failed");
    assert_eq!(
        metadata.status,
        StatusCode::OK,
        "clause 5.13.4.4 restricts only the payload projection, not metadata: {:?}",
        metadata.body
    );

    forget_cached(scope.database_pool(), from_ref(&context_url)).await;
    server.shutdown().await.expect("context server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}
