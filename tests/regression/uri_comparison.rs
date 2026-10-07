//! Clause 4.9 `URI` and the comparison of URI operands, end to end.
//!
//! Clause 4.9: "URI shall be a URI as mandated by IETF RFC 3986 [5] or an IRI
//! as mandated by IETF RFC 3987 [23], appendix A, production rule named URI",
//! and the adjacent recommendation is that "URI comparison should be performed
//! so that the number of false negatives is minimized, as recommended by IETF
//! RFC 3986 section 6".
//!
//! A Relationship object is normalised by the RFC 3986 section 6.2.2 and 6.2.3
//! rules when it is parsed for storage, so a `q` operand has to be compared in
//! the same normalised form, not as the bytes the client typed: an entity
//! created with `HTTP://Example.com/a` is stored as `http://example.com/a` and
//! has to be found by the very URI that created it.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The entity type every fixture in this module carries.
const FIXTURE_TYPE: &str = "UriComparison";

/// The Relationship whose object the filters compare against.
const RELATIONSHIP: &str = "isParkedIn";

async fn create(client: &TestClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED, "create entity: {:?}", response.body);
}

async fn query(client: &TestClient, query_str: &str) -> (StatusCode, Value) {
    let response = client
        .query_entities(query_str, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");
    let body = response.json_value().expect("query response must be valid JSON");
    (response.status, body)
}

fn ids_from_response(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("response must be a JSON array")
        .iter()
        .map(|entity| entity["id"].as_str().expect("entity must have an id member").to_owned())
        .collect()
}

/// Store one entity whose Relationship points at `object`, and return its id.
async fn create_linked(scope: &TestEnvironment, client: &TestClient, suffix: &str, object: &str) -> String {
    let id = scope.id(&format!("{FIXTURE_TYPE}:{suffix}"));
    create(
        client,
        &json!({
            "id": id,
            "type": FIXTURE_TYPE,
            RELATIONSHIP: {"type": "Relationship", "object": object},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    id
}

/// A `q` equality filter over one test's own entities.
fn object_filter(ids: &[&str], object: &str) -> String {
    format!(
        "type={FIXTURE_TYPE}&local=true&id={}&q={}",
        ids.iter().map(|id| urlencoding::encode(id).into_owned()).collect::<Vec<_>>().join(","),
        urlencoding::encode(&format!("{RELATIONSHIP}==\"{object}\""))
    )
}

/// An entity is found by the exact URI that created it, although the object is
/// normalised on the way in.
#[tokio::test]
async fn regression_uri_comparison_an_entity_is_found_by_the_uri_that_created_it() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let written = "HTTP://Example.com/a";
    let id = create_linked(&scope, client, "as-written", written).await;

    let (status, body) = query(client, &object_filter(&[&id], written)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        ids_from_response(&body),
        vec![id],
        "an entity has to be findable by the URI its own creation supplied"
    );

    scope.finish().await.expect("cleanup failed");
}

/// RFC 3986 section 6.2.2 case normalisation, section 6.2.2.3 path segment
/// normalisation and section 6.2.3 scheme-based normalisation each make a
/// spelling equivalent to the stored one, so each has to find the entity.
#[tokio::test]
async fn regression_uri_comparison_every_rfc3986_equivalent_spelling_matches() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_linked(&scope, client, "equivalences", "http://example.com/a/c").await;

    for equivalent in [
        // 6.2.2.1 case normalisation of scheme and host.
        "HTTP://Example.com/a/c",
        "http://EXAMPLE.COM/a/c",
        // 6.2.3 removal of a default port.
        "http://example.com:80/a/c",
        // 6.2.2.3 path segment normalisation.
        "http://example.com/a/./c",
        "http://example.com/a/b/../c",
    ] {
        let (status, body) = query(client, &object_filter(&[&id], equivalent)).await;
        assert_eq!(status, StatusCode::OK, "{equivalent}: {body}");
        assert_eq!(
            ids_from_response(&body),
            vec![id.clone()],
            "{equivalent} is equivalent to the stored URI under RFC 3986 section 6"
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// Minimising false negatives must not manufacture false positives: a URI that
/// RFC 3986 does *not* make equivalent still fails to match. The path and the
/// namespace-specific part of a URN are case-sensitive.
#[tokio::test]
async fn regression_uri_comparison_a_uri_rfc3986_does_not_make_equivalent_still_does_not_match() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_linked(&scope, client, "distinct", "http://example.com/a/c").await;

    for distinct in [
        "http://example.com/A/C",
        "http://example.com/a/d",
        "http://example.org/a/c",
        "https://example.com/a/c",
    ] {
        let (status, body) = query(client, &object_filter(&[&id], distinct)).await;
        assert_eq!(status, StatusCode::OK, "{distinct}: {body}");
        assert!(ids_from_response(&body).is_empty(), "{distinct} names a different resource and must not match");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9 EXAMPLE 3, `?q=isParked=="urn:ngsi-ld:OffStreetParking:Downtown1"`.
/// A URN has no scheme-based normalisation beyond its case-insensitive prefix,
/// so it compares exactly.
#[tokio::test]
async fn regression_uri_comparison_a_urn_object_compares_as_the_clause_4_9_example_writes_it() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let urn = "urn:ngsi-ld:OffStreetParking:Downtown1";
    let id = create_linked(&scope, client, "urn-object", urn).await;

    let (status, body) = query(client, &object_filter(&[&id], urn)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), vec![id.clone()]);

    // Annex A.3 makes the scheme and NID case-insensitive while the
    // namespace-specific string stays case-sensitive.
    let (status, body) = query(client, &object_filter(&[&id], "URN:NGSI-LD:OffStreetParking:Downtown1")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), vec![id.clone()], "the URN prefix is case-insensitive");

    let (status, body) = query(client, &object_filter(&[&id], "urn:ngsi-ld:offstreetparking:downtown1")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(ids_from_response(&body).is_empty(), "the namespace specific string stays case-sensitive");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.2.4 requires only "a valid URI" of an entity identifier, so
/// narrowing what is accepted would itself be a conformance risk. Every one of
/// these satisfies the RFC 3986 `URI` production and has to be storable.
#[tokio::test]
async fn regression_uri_comparison_every_rfc3986_uri_shape_is_accepted_as_a_relationship_object() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (index, object) in [
        "urn:ngsi-ld:Building:001",
        "urn:uuid:6e8bc430-9c3a-11d9-9669-0800200c9a66",
        "tag:example.com,2024:foo",
        "mailto:someone@example.com",
        "did:example:123",
        "file:///tmp/x",
        "ftp://example.com/x",
        "s3://bucket/key",
        "coap+tcp://host/x",
        "x-custom.scheme+v1:opaque",
        "ldap://[2001:db8::7]/c=GB?objectClass?one",
        "https://example.com/a?b=c#d",
    ]
    .into_iter()
    .enumerate()
    {
        create_linked(&scope, client, &format!("shape-{index}"), object).await;
    }

    scope.finish().await.expect("cleanup failed");
}
