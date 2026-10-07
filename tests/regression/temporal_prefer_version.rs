//! `Prefer: ngsi-ld=<version>` against the three temporal read bindings.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.6 makes the `Preference-Applied` echo
//! mandatory on any response carrying a payload body once the client expressed
//! the preference, and Tables 6.18.3.2-2 and 6.19.3.1-2 each carry the row that
//! turns an amended
//! answer into "203 Non-Authoritative Information ... amended to conform to a
//! specific version of the NGSI-LD specification as mandated in clause
//! 4.3.6.8". The `-1` table of each pair is the URL-parameter table and names no
//! `Prefer`, because `Prefer` is a header rather than a parameter.
//!
//! The temporal bindings must honour the header as `GET /entities` does,
//! answering 203 with `Preference-Applied` after the clause 4.3.6.8 adaptation
//! rather than 200 with neither.
//!
//! Clause 4.5.6 makes the Temporal Evolution of an Entity the Entity of clause
//! 4.5.1 with each Attribute replaced by its temporal representation, and that
//! changes two of the fallback rows. Clause 4.5.7 makes an Attribute's array
//! "the sequence of instances of the referred Property during a period of
//! time", not the `datasetId`-separated set that notes 2 and 3 of Table
//! 4.3.6.8-1 collapse below 1.3; and it makes the Temporal Property of each
//! instance -- "typically `observedAt`" -- what places that instance in the
//! sequence, so the Table 4.3.6.8-2 row that removes `observedAt` below 1.3
//! would leave a payload that is no version of a temporal representation rather
//! than an older one. Both are asserted here, because a naive threading of the
//! header truncates a history to one point and strips its timestamps.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};

/// The core context every payload here is written against.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// The window every temporal read of this module asks for.
const WINDOW: &str = "timerel=after&timeAt=2000-01-01T00:00:00Z";

/// The three instants the Property under test was observed at, in order.
const OBSERVATIONS: [&str; 3] = ["2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z", "2026-01-03T00:00:00Z"];

/// The `datasetId` the observed Property instances carry (clause 4.5.5).
const DATASET: &str = "urn:ngsi-ld:Dataset:primary";

/// The Relationship target the temporal Relationship points at.
const TARGET: &str = "urn:ngsi-ld:Building:target";

/// The version this broker conforms to, which clause 6.3.6 echoes back when the
/// payload needed no amendment at all.
const CONFORMANT: &str = "ngsi-ld=1.9";

/// One Temporal Evolution carrying everything the fallback tables reach.
///
/// The Entity names several types (Table 4.3.6.8-1 note 1, 1.3) and a `scope`
/// (Table 4.3.6.8-1, 1.4). `speed` carries a three-instance sequence whose
/// instances each hold `observedAt`, `unitCode` and `datasetId` (Table
/// 4.3.6.8-2, all three at 1.3); `label` is a `LanguageProperty` (1.4) and
/// `refBuilding` a Relationship carrying `objectType` (Table 4.3.6.8-3, 1.8).
fn evolution(id: &str) -> Value {
    json!({
        "id": id,
        "type": ["Vehicle", "Car"],
        // Clause 4.18's ScopeLevel admits letters, digits and `_` only.
        "scope": "/regression/temporalPrefer",
        "speed": OBSERVATIONS
            .iter()
            .enumerate()
            .map(|(index, observed)| json!({
                "type": "Property",
                "value": index + 1,
                "observedAt": observed,
                "unitCode": "KMH",
                "datasetId": DATASET
            }))
            .collect::<Vec<Value>>(),
        "label": [{"type": "LanguageProperty", "languageMap": {"en": "Pump"}, "observedAt": OBSERVATIONS[0]}],
        "refBuilding": [{"type": "Relationship", "object": TARGET, "objectType": "Building", "observedAt": OBSERVATIONS[0]}],
        "@context": CORE_CONTEXT
    })
}

/// Create one Temporal Evolution and assert clause 6.20.3.1 answered 201.
async fn create(client: &TestClient, body: &Value) {
    let response = client.create_temporal_entity(body).await.expect("create temporal entity request");
    assert_eq!(response.status, StatusCode::CREATED, "create {}: {:?}", body["id"], response.body);
}

/// The `Preference-Applied` header a response carried, if any.
fn preference_applied(headers: &HeaderMap) -> Option<String> {
    headers.get("preference-applied").and_then(|value| value.to_str().ok()).map(str::to_owned)
}

/// One temporal read, optionally carrying a `Prefer` header, answered as its
/// status, its `Preference-Applied` echo and its body.
async fn read(client: &TestClient, path: &str, prefer: Option<&str>) -> (StatusCode, Option<String>, Value) {
    let mut request = client.raw(Method::GET, path).header("Accept", "application/json");
    if let Some(prefer) = prefer {
        request = request.header("Prefer", prefer);
    }
    let response = request.send().await.expect("temporal read request");
    let body = response.json_value().expect("a temporal read answers with JSON");
    (response.status, preference_applied(&response.headers), body)
}

/// The single-entity retrieval of clause 6.19.3.1.
async fn retrieve(client: &TestClient, id: &str, selection: &str, prefer: Option<&str>) -> (StatusCode, Option<String>, Value) {
    let path = format!("/ngsi-ld/v1/temporal/entities/{}?{WINDOW}{selection}", urlencoding::encode(id));
    read(client, &path, prefer).await
}

/// The query of clause 6.18.3.2, answered as the one Entity it selected.
async fn query_one(client: &TestClient, id: &str, prefer: Option<&str>) -> (StatusCode, Option<String>, Value) {
    // Clause 5.7.4.4 lists no identifier among the narrowing inputs, so the
    // type selector is what lets this query run at all.
    let path = format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&id={}&{WINDOW}", urlencoding::encode(id));
    let (status, applied, body) = read(client, &path, prefer).await;
    (status, applied, single(&body))
}

/// The POST form of clause 6.24.3.1, answered as the one Entity it selected.
async fn post_query_one(client: &TestClient, id: &str, prefer: Option<&str>) -> (StatusCode, Option<String>, Value) {
    let mut request = client
        .raw(Method::POST, "/ngsi-ld/v1/temporal/entityOperations/query")
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header(
            "Link",
            &format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        )
        .json(&json!({
            "type": "Query",
            "entities": [{"id": id, "type": "Vehicle"}],
            "temporalQ": {"timerel": "after", "timeAt": "2000-01-01T00:00:00Z"}
        }));
    if let Some(prefer) = prefer {
        request = request.header("Prefer", prefer);
    }
    let response = request.send().await.expect("temporal query request");
    let body = response.json_value().expect("a temporal query answers with JSON");
    (response.status, preference_applied(&response.headers), single(&body))
}

/// The one Entity a temporal collection carries.
fn single(body: &Value) -> Value {
    let matches = body.as_array().expect("a temporal query answers a collection");
    assert_eq!(matches.len(), 1, "the query answers the one Entity it selected: {body}");
    matches[0].clone() // clone: the assertions outlive the collection they were read from
}

/// Assert the clause 4.3.6.8 amendment a 1.0 target makes to the normalized
/// temporal representation, whichever binding produced it.
fn assert_amended_to_v1_0(body: &Value) {
    // Table 4.3.6.8-1 note 1: a 1.0 target receives one Entity type, the first.
    assert_eq!(body["type"], json!("Vehicle"), "{body}");
    // Table 4.3.6.8-1: `scope` arrived at 1.4, and clause 4.5.6 carries it as a
    // temporal Property, so the entity member is what goes.
    assert!(body.get("scope").is_none(), "{body}");

    // Clause 4.5.7: the whole sequence stands, in order, each instance still
    // placed in it by the Temporal Property that indexes it.
    let speed = body["speed"].as_array().expect("the sequence is an array");
    assert_eq!(speed.len(), OBSERVATIONS.len(), "the sequence is whole: {body}");
    for (instance, observed) in speed.iter().zip(OBSERVATIONS) {
        assert_eq!(instance["observedAt"], json!(observed), "{body}");
        // Table 4.3.6.8-2: `unitCode` and `datasetId` arrived at 1.3.
        assert!(instance.get("unitCode").is_none(), "{body}");
        assert!(instance.get("datasetId").is_none(), "{body}");
    }

    // Table 4.3.6.8-1: a LanguageProperty arrived at 1.4 and is reformatted as
    // a Property, which moves `languageMap` into `value`.
    assert_eq!(body["label"][0]["type"], json!("Property"), "{body}");
    assert_eq!(body["label"][0]["value"], json!({"en": "Pump"}), "{body}");
    assert_eq!(body["label"][0]["observedAt"], json!(OBSERVATIONS[0]), "{body}");

    // Table 4.3.6.8-3: `objectType` arrived at 1.8, `observedAt` at 1.3.
    assert_eq!(body["refBuilding"][0]["object"], json!(TARGET), "{body}");
    assert!(body["refBuilding"][0].get("objectType").is_none(), "{body}");
    assert_eq!(body["refBuilding"][0]["observedAt"], json!(OBSERVATIONS[0]), "{body}");
}

/// Table 6.19.3.1-2: the single-entity retrieval answers 203 with the echo, and
/// the payload is the one clause 4.3.6.8 asks for.
#[tokio::test]
async fn regression_temporal_prefer_version_retrieval_amends_and_echoes() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:temporal-prefer-retrieve");
    create(client, &evolution(&id)).await;

    let (status, applied, body) = retrieve(client, &id, "", Some("ngsi-ld=1.0")).await;

    assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION, "{body}");
    assert_eq!(applied.as_deref(), Some("ngsi-ld=1.0"), "{body}");
    assert_amended_to_v1_0(&body);

    scope.finish().await.expect("cleanup failed");
}

/// Table 6.18.3.2-2: the query binding answers the same way.
#[tokio::test]
async fn regression_temporal_prefer_version_query_amends_and_echoes() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:temporal-prefer-query");
    create(client, &evolution(&id)).await;

    let (status, applied, body) = query_one(client, &id, Some("ngsi-ld=1.0")).await;

    assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION, "{body}");
    assert_eq!(applied.as_deref(), Some("ngsi-ld=1.0"), "{body}");
    assert_amended_to_v1_0(&body);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.24.3.1 gives the POST form the clause 5.7.4 behaviour of the GET,
/// and clause 6.3.6 ties the echo to any response carrying a payload body, so
/// the two bindings cannot answer differently. Table 6.24.3.1-1 prints no 203
/// row, which is the half of the pair the clause makes redundant rather than a
/// different rule.
#[tokio::test]
async fn regression_temporal_prefer_version_post_query_amends_and_echoes() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:temporal-prefer-post-query");
    create(client, &evolution(&id)).await;

    let (status, applied, body) = post_query_one(client, &id, Some("ngsi-ld=1.0")).await;

    assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION, "{body}");
    assert_eq!(applied.as_deref(), Some("ngsi-ld=1.0"), "{body}");
    assert_amended_to_v1_0(&body);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.7 is what the sequence assertion rests on, so it is worth stating
/// against the unadapted answer: the amendment removes members from each
/// instance and reformats families, and changes nothing about how many
/// instances there are or where each one stands in time.
#[tokio::test]
async fn regression_temporal_prefer_version_amendment_never_shortens_the_sequence() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:temporal-prefer-sequence");
    create(client, &evolution(&id)).await;

    let (unadapted_status, unadapted_applied, unadapted) = retrieve(client, &id, "", None).await;
    let (_, _, adapted) = retrieve(client, &id, "", Some("ngsi-ld=1.0")).await;

    assert_eq!(unadapted_status, StatusCode::OK, "{unadapted}");
    assert_eq!(unadapted_applied, None, "no preference was expressed: {unadapted}");

    let instants = |body: &Value| -> Vec<Value> {
        body["speed"]
            .as_array()
            .expect("the sequence is an array")
            .iter()
            .map(|instance| instance["observedAt"].clone()) // clone: the comparison outlives the borrow
            .collect()
    };
    assert_eq!(instants(&adapted), instants(&unadapted), "the sequence is the same one: {adapted}");
    assert_eq!(instants(&adapted).len(), OBSERVATIONS.len(), "{adapted}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.3.6.8 short-circuits a target that is the conformant version or
/// newer: the payload was never amended, so it stays authoritative and answers
/// 200, while clause 6.3.6 still names the version it conforms to.
#[tokio::test]
async fn regression_temporal_prefer_version_a_conformant_target_stays_authoritative() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:temporal-prefer-conformant");
    create(client, &evolution(&id)).await;

    let (_, _, unadapted) = retrieve(client, &id, "", None).await;
    let (status, applied, body) = retrieve(client, &id, "", Some(CONFORMANT)).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(applied.as_deref(), Some(CONFORMANT), "{body}");
    assert_eq!(body, unadapted, "an unamended payload is the one the broker would have sent anyway");

    scope.finish().await.expect("cleanup failed");
}

/// RFC 7240 section 2 leaves a server free to ignore a preference it does not
/// understand, and clause 6.3.6 ties the echo to `ngsi-ld=<version>` having
/// been set. A token outside the `major.minor` form is neither.
#[tokio::test]
async fn regression_temporal_prefer_version_an_unreadable_token_earns_no_echo() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:temporal-prefer-unreadable");
    create(client, &evolution(&id)).await;

    let (status, applied, body) = retrieve(client, &id, "", Some("ngsi-ld=latest")).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(applied, None, "{body}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.9 renders a temporal Attribute as one object per `datasetId`
/// holding a plural member -- `values`, `objects`, `languageMaps` and the rest
/// -- whose value is the array of `[value, Temporal Property]` pairs. The
/// sequence in time therefore lives inside that member, so Table 4.3.6.8-1's
/// family reformat has to rename the member and rewrite each pair, and the
/// Table 4.3.6.8-2 rows that name an instance's own members reach nothing.
#[tokio::test]
async fn regression_temporal_prefer_version_temporal_values_reformats_the_plural_member() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:temporal-prefer-values");
    create(client, &evolution(&id)).await;

    let (unadapted_status, _, unadapted) = retrieve(client, &id, "&options=temporalValues", None).await;
    let (status, applied, body) = retrieve(client, &id, "&options=temporalValues", Some("ngsi-ld=1.0")).await;

    // The control: clause 4.5.9 gives a LanguageProperty `languageMaps`, whose
    // pairs carry a one-member `{"languageMap": ...}` object.
    assert_eq!(unadapted_status, StatusCode::OK, "{unadapted}");
    assert_eq!(unadapted["label"]["type"], json!("LanguageProperty"), "{unadapted}");
    assert_eq!(
        unadapted["label"]["languageMaps"],
        json!([[{"languageMap": {"en": "Pump"}}, OBSERVATIONS[0]]]),
        "{unadapted}"
    );

    assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION, "{body}");
    assert_eq!(applied.as_deref(), Some("ngsi-ld=1.0"), "{body}");

    // A Property in this representation carries `values` with a bare value, so
    // the reformat renames the member and unwraps each pair.
    assert_eq!(body["label"]["type"], json!("Property"), "{body}");
    assert!(body["label"].get("languageMaps").is_none(), "{body}");
    assert_eq!(body["label"]["values"], json!([[{"en": "Pump"}, OBSERVATIONS[0]]]), "{body}");

    // The sequence inside the member is untouched, and so is a family every
    // version has known.
    assert_eq!(body["speed"]["type"], json!("Property"), "{body}");
    assert_eq!(body["speed"]["values"], unadapted["speed"]["values"], "{body}");
    assert!(body["speed"].get("datasetId").is_none(), "`datasetId` arrived at 1.3: {body}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.19's aggregated representation shares that shape, and its
/// aggregation vocabulary appears in no row of the fallback tables: the family
/// and the `datasetId` are adapted, the aggregates pass through.
#[tokio::test]
async fn regression_temporal_prefer_version_aggregated_values_keeps_its_aggregates() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:temporal-prefer-aggregated");
    create(client, &evolution(&id)).await;

    let selection = "&options=aggregatedValues&aggrMethods=totalCount&aggrPeriodDuration=P1D";
    let (unadapted_status, _, unadapted) = retrieve(client, &id, selection, None).await;
    let (status, applied, body) = retrieve(client, &id, selection, Some("ngsi-ld=1.0")).await;

    assert_eq!(unadapted_status, StatusCode::OK, "{unadapted}");
    assert_eq!(unadapted["label"]["type"], json!("LanguageProperty"), "{unadapted}");

    assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION, "{body}");
    assert_eq!(applied.as_deref(), Some("ngsi-ld=1.0"), "{body}");
    assert_eq!(body["label"]["type"], json!("Property"), "{body}");
    assert_eq!(body["speed"]["totalCount"], unadapted["speed"]["totalCount"], "{body}");
    assert!(body["speed"].get("datasetId").is_none(), "{body}");

    scope.finish().await.expect("cleanup failed");
}
