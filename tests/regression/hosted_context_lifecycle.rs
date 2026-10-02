//! What a `{contextId}` addresses, and what a use of it counts.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.13.4.3 gives Serve @context its input as
//! "the locally unique identifier that identifies the desired @context in the
//! broker's internal storage... For @contexts of kind 'Cached' this can also be
//! the original URL the broker downloaded the @context from", and clause
//! 5.13.5.3 says the same of Delete and Reload. Both spellings are URIs and
//! every stored entry is keyed by one, so a path segment that is neither keys
//! no entry.
//!
//! Clauses 5.13.4.4 and 5.13.5.4 then decide what that is: "It shall give back
//! `ResourceNotFound` if the identifier is not found", and "If the @context
//! identifier does not correspond to any existing entry in the @context
//! storage, then an error of type `ResourceNotFound` shall be raised". Only an
//! identifier that is *not supplied* is `BadRequestData`, and a path segment is
//! never missing. The broker typed the segment as a URI at extraction, so a
//! client asking about an identifier it had invented was told its request was
//! malformed instead of being told there is no such @context.
//!
//! Clause 5.13.3.5 gives the metadata a `numberOfHits`, "number of times the
//! @context was found in the storage". The count was only visible after a
//! background flush folded the resolver's pending deltas into the row, so a
//! client reading it straight after using a @context was told a number that was
//! already out of date -- and reported zero for a @context it had just used. A
//! reader is now told the accesses that have happened rather than the ones
//! already written down.
//!
//! One NGSI-LD operation resolves the same @context several times internally --
//! a write is screened (clause 5.5.4) and then expanded again for storage -- and
//! the count now folds those internal resolutions into one find per operation.
//! Clause 5.13.3.5's "number of times the @context was found in the storage" is
//! read as the client-visible use of the context, not the broker's internal
//! plumbing, so one request that uses a context reports one find and two
//! requests report two (`crate::operation scope` in `andromeda_jsonld`).
//!
//! A @context a client echoes back as the relative path clause 5.13.2.5 handed
//! it (`/ngsi-ld/v1/jsonldContexts/{id}`) resolves by its local id, while its
//! `numberOfHits` and the durable flush key on the serving URL; the resolver
//! attributes the find to that serving URL so a relative use is counted, where
//! before it was silently attributed to the bare identifier and lost.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::{environment::TestEnvironment, tenant_scoped_environment::TenantScopedEnvironment};
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The problem type clauses 5.13.4.4 and 5.13.5.4 name for an identifier that
/// corresponds to no entry.
const RESOURCE_NOT_FOUND: &str = "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound";

/// Path segments that are no URI, so no stored entry can be keyed by one.
const UNKEYED_SEGMENTS: [&str; 3] = ["wrong_id_context", "1234567890123456", "not%20a%20uri"];

/// A minimal hosted `@context` document (clause 5.13.2.3).
fn hosted_document() -> Value {
    json!({"@context": {"A1": "urn:ngsi-ld:attributes:A1"}})
}

/// Create one hosted `@context` and answer with the identifier the `Location`
/// header named (clause 5.13.2.5).
async fn create_hosted_context(client: &TestClient) -> String {
    let created = client.create_jsonld_context(&hosted_document()).await.expect("create context request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    let location = created
        .headers
        .get("location")
        .and_then(|value| value.to_str().ok())
        .expect("clause 5.13.2.5 answers with the identifier in a Location header");
    location.rsplit('/').next().expect("a Location header names an identifier").to_owned()
}

/// Read one `@context`'s metadata (clause 5.13.4.4 with `details=true`).
async fn context_metadata(client: &TestClient, id: &str) -> Value {
    let path = format!("jsonldContexts/{}?details=true", urlencoding::encode(id));
    let response = client.raw(Method::GET, &path).send().await.expect("metadata request");
    assert_eq!(response.status, StatusCode::OK, "metadata: {:?}", response.body);
    response.json_value().expect("metadata is JSON")
}

/// The `numberOfHits` clause 5.13.3.5 reports for one `@context`.
async fn hits(client: &TestClient, id: &str) -> u64 {
    context_metadata(client, id).await["numberOfHits"]
        .as_u64()
        .expect("clause 5.13.3.5 reports numberOfHits as a number")
}

/// Assert a refusal is the `ResourceNotFound` of clauses 5.13.4.4 and 5.13.5.4.
fn assert_not_found(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::NOT_FOUND, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(RESOURCE_NOT_FOUND), "{scenario}: {problem}");
    assert!(problem["title"].is_string(), "{scenario}: a problem carries a title: {problem}");
}

/// Clauses 5.13.4.4 and 5.13.5.4: an identifier that corresponds to no entry is
/// `ResourceNotFound`, whether it is a well-formed URI the broker never stored
/// or a segment no entry could be keyed by at all.
#[tokio::test]
async fn regression_hosted_context_an_unknown_identifier_is_not_found() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let mut segments: Vec<String> = UNKEYED_SEGMENTS.iter().map(|segment| (*segment).to_owned()).collect();
    segments.push(scope.id("Context:never-stored"));
    segments.push("https://example.invalid/never-downloaded.jsonld".to_owned());

    for segment in &segments {
        let encoded = urlencoding::encode(segment);
        let served = client
            .raw(Method::GET, &format!("jsonldContexts/{encoded}"))
            .send()
            .await
            .expect("serve request");
        assert_not_found(&served, &format!("serve {segment}"));

        let detailed = client
            .raw(Method::GET, &format!("jsonldContexts/{encoded}?details=true"))
            .send()
            .await
            .expect("metadata request");
        assert_not_found(&detailed, &format!("serve {segment} with details"));

        let deleted = client
            .raw(Method::DELETE, &format!("jsonldContexts/{encoded}"))
            .send()
            .await
            .expect("delete request");
        assert_not_found(&deleted, &format!("delete {segment}"));

        for reload in ["true", "false"] {
            let reloaded = client
                .raw(Method::DELETE, &format!("jsonldContexts/{encoded}?reload={reload}"))
                .send()
                .await
                .expect("delete request");
            assert_not_found(&reloaded, &format!("delete {segment} with reload={reload}"));
        }
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.13.2.5 hands back an identifier the following operations address,
/// so a stored `@context` is served, its metadata read and it is deleted --
/// after which the same identifier is `ResourceNotFound` like any other.
#[tokio::test]
async fn regression_hosted_context_a_stored_identifier_addresses_its_entry_until_it_is_deleted() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_hosted_context(client).await;

    let served = client.retrieve_jsonld_context(&id).await.expect("serve request");
    assert_eq!(served.status, StatusCode::OK, "serve: {:?}", served.body);
    let document = served.json_value().expect("a served context is JSON");
    assert!(document.get("@context").is_some(), "clause 5.13.4.4 serves the full content: {document}");

    let metadata = context_metadata(client, &id).await;
    assert_eq!(
        metadata["kind"],
        json!("Hosted"),
        "clause 5.13.1 flags an explicitly added context Hosted: {metadata}"
    );
    assert_eq!(metadata["localId"], json!(id), "clause 5.13.3.5 reports the local identifier: {metadata}");

    let deleted = client.delete_jsonld_context(&id).await.expect("delete request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete: {:?}", deleted.body);

    let gone = client.retrieve_jsonld_context(&id).await.expect("serve request");
    assert_not_found(&gone, "the deleted identifier");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.13.5.4: "If reload is true and the kind of the @context is not
/// 'Cached', implementations shall return a `BadRequestData` error." A hosted
/// `@context` has no source to re-download from.
#[tokio::test]
async fn regression_hosted_context_reload_is_refused_for_a_kind_that_was_never_downloaded() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_hosted_context(client).await;

    let reloaded = client
        .raw(Method::DELETE, &format!("jsonldContexts/{}?reload=true", urlencoding::encode(&id)))
        .send()
        .await
        .expect("delete request");
    assert_eq!(reloaded.status, StatusCode::BAD_REQUEST, "reload of a hosted context: {:?}", reloaded.body);
    let problem = reloaded.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!("https://uri.etsi.org/ngsi-ld/errors/BadRequestData"), "{problem}");

    // The refused reload left the entry alone: clause 5.13.5.4 removes nothing
    // when the operation ends in an error.
    let still_there = client.retrieve_jsonld_context(&id).await.expect("serve request");
    assert_eq!(still_there.status, StatusCode::OK, "a refused reload deletes nothing: {:?}", still_there.body);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.13.3.5: `numberOfHits` is the "number of times the @context was
/// found in the storage". A newly stored one has been found none, and a request
/// that resolves it has found it -- reported when the metadata is read rather
/// than when the background flush next runs.
#[tokio::test]
async fn regression_hosted_context_a_use_is_counted_before_the_flush_runs() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_hosted_context(client).await;
    let metadata = context_metadata(client, &id).await;
    let url = metadata["URL"]
        .as_str()
        .expect("clause 5.13.3.5 reports the URL a hosted context is served from")
        .to_owned();

    assert_eq!(hits(client, &id).await, 0, "a context nothing has used yet has been found nowhere");
    // Reading the metadata is not a find in the storage of the document.
    assert_eq!(hits(client, &id).await, 0, "reading the metadata twice still reports none");

    let entity_id = scope.id("Testing:hosted-context-use");
    let created = client
        .post_entity(&json!({"id": entity_id, "type": "Testing", "A1": "a value", "@context": [url]}))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    assert_eq!(
        hits(client, &id).await,
        1,
        "a request that resolved the context found it once, and the count says so at once"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.13.2.5 hands the client a relative path for the stored `@context`;
/// echoing it back as a request's `@context` must count as a find. Before, the
/// relative path was reduced to the bare identifier and the find was attributed
/// to a key `numberOfHits` never reads, so a genuine use reported zero.
#[tokio::test]
async fn regression_hosted_context_a_relative_reference_is_resolved_and_counted() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_hosted_context(client).await;
    assert_eq!(hits(client, &id).await, 0, "a freshly stored context has been found nowhere");

    // The path clause 5.13.2.5 hands back, exactly as a client would echo it.
    let relative = format!("/ngsi-ld/v1/jsonldContexts/{id}");
    let entity_id = scope.id("Testing:relative-context-use");
    let created = client
        .post_entity(&json!({"id": entity_id, "type": "Testing", "A1": "a value", "@context": [relative]}))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    assert_eq!(
        hits(client, &id).await,
        1,
        "a relative reference to a hosted context is resolved and its use counted exactly once"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 6.29.3.1 and 4.14: Add @context answers a tenant's client with the
/// root-relative path of the entry it stored for that tenant, and a resource
/// path names a resource of the tenant the request addresses. Echoed back as a
/// request's `@context` in the same tenant, the path names that tenant's entry.
/// Before, a root-relative path naming no `andromedaTenant` was read as the
/// default tenant's, so the only tenant it worked in was the default one.
#[tokio::test]
async fn regression_hosted_context_a_relative_reference_names_the_requesting_tenants_entry() {
    let scope = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");
    let client = scope.client();
    let seeded = client
        .post_entity(&json!({"id": scope.id("Testing:tenant-seed"), "type": "Testing", "@context": CORE_CONTEXT}))
        .with_ld_json()
        .send()
        .await
        .expect("seed entity request");
    assert_eq!(seeded.status, StatusCode::CREATED, "the seed brings the tenant into being: {:?}", seeded.body);
    let id = create_hosted_context(client).await;

    let relative = format!("/ngsi-ld/v1/jsonldContexts/{id}");
    let entity_id = scope.id("Testing:tenant-relative-context-use");
    let created = client
        .post_entity(&json!({"id": entity_id, "type": "Testing", "A1": "a value", "@context": [relative]}))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let stored = client
        .get_entity(&entity_id, "", "application/json", None)
        .await
        .expect("retrieve entity request");
    assert_eq!(stored.status, StatusCode::OK, "retrieve: {:?}", stored.body);
    let stored = stored.json_value().expect("an entity body");
    assert_eq!(
        stored["urn:ngsi-ld:attributes:A1"]["value"],
        json!("a value"),
        "the attribute expanded through the tenant's stored @context: {stored}"
    );
    assert_eq!(hits(client, &id).await, 1, "the use is counted against the tenant's entry");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.13.3.5, one find per operation: a single request that resolves a
/// context several times internally counts one, and a second request counts a
/// second, so the number tracks client-visible uses rather than internal
/// resolutions.
#[tokio::test]
async fn regression_hosted_context_each_request_counts_a_single_find() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_hosted_context(client).await;
    let relative = format!("/ngsi-ld/v1/jsonldContexts/{id}");

    for (index, expected) in [(1u64, 1u64), (2, 2)] {
        let entity_id = scope.id(&format!("Testing:one-find-per-request-{index}"));
        let created = client
            .post_entity(&json!({"id": entity_id, "type": "Testing", "A1": "a value", "@context": [relative]}))
            .with_ld_json()
            .send()
            .await
            .expect("create entity request");
        assert_eq!(created.status, StatusCode::CREATED, "create {index}: {:?}", created.body);
        assert_eq!(hits(client, &id).await, expected, "request {index} adds exactly one find");
    }

    scope.finish().await.expect("cleanup failed");
}
