#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Discovery answered from the census the entity writes keep (clauses 5.7.5 to
//! 5.7.10).
//!
//! Every scenario walks a sequence of writes through the public API and, after
//! each, checks two things: the six discovery answers say exactly what the
//! existing entities carry, and the census ledgers in the tenant's database
//! sum to what its search rows recompute to. The answers follow clause 5.7:
//! only what exists now is reported, so an attribute the last carrier lost and
//! a type the last entity dropped disappear; `attributeTypes` of a type are the
//! families its own entities' instances declare (clause 5.7.7); and
//! `attributeCount` counts attribute instances, each `datasetId` instance
//! apart and an entity with two types once (Table 5.2.28-1).

use crate::{BASE_URL, client, provision_tenant};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::time::rendering::instant_text;
use andromeda_postgresql_context_store::entity::census::fold::fold_census;
use andromeda_test_harness::{
    discovery_census::{census_drift, census_rows},
    tenant_scoped_environment::TenantScopedEnvironment,
};
use chrono::{Duration as TimeDelta, Utc};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::sleep;

const T: &str = "https://example.org/census/T";
const U: &str = "https://example.org/census/U";
const V: &str = "https://example.org/census/V";
const A: &str = "https://example.org/census/a";
const B: &str = "https://example.org/census/b";
const C: &str = "https://example.org/census/c";
const D: &str = "https://example.org/census/d";
const SCOPE: &str = "https://uri.etsi.org/ngsi-ld/scope";

/// The IRI as one path segment.
fn segment(iri: &str) -> String {
    iri.replace(':', "%3A").replace('/', "%2F").replace('#', "%23")
}

fn property(value: i64) -> Value {
    json!({"type": "Property", "value": value})
}

fn relationship(object: &str) -> Value {
    json!({"type": "Relationship", "object": object})
}

fn point() -> Value {
    json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [14.5, 46.05]}})
}

/// One request against the private tenant, with a JSON body when given.
async fn send(c: &Client, tenant: &str, method: Method, path: &str, body: Option<&Value>) -> (StatusCode, Value) {
    let mut request = c.request(method, format!("{BASE_URL}{path}")).header("NGSILD-Tenant", tenant);
    if let Some(body) = body {
        request = request.header("Content-Type", "application/json").json(body);
    }
    let response = request.send().await.expect("request failed");
    let status = response.status();
    let body = response.json().await.unwrap_or_default();
    (status, body)
}

async fn create(c: &Client, tenant: &str, entity: &Value) {
    let (status, body) = send(c, tenant, Method::POST, "/entities", Some(entity)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "create {entity}: {body}");
}

/// The discovery answers of the tenant, reduced to comparable sets.
struct Answers {
    types: Vec<String>,
    type_attributes: Vec<(String, Vec<String>)>,
    attributes: Vec<String>,
    attribute_types: Vec<(String, Vec<String>)>,
}

fn strings(value: &Value) -> Vec<String> {
    let mut values: Vec<String> = match value {
        Value::Array(items) => items.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
        Value::String(single) => vec![single.clone()],
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => Vec::new(),
    };
    values.sort();
    values
}

async fn answers(c: &Client, tenant: &str) -> Answers {
    let (status, types) = send(c, tenant, Method::GET, "/types", None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "{types}");
    let (status, details) = send(c, tenant, Method::GET, "/types?details=true", None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "{details}");
    let (status, attributes) = send(c, tenant, Method::GET, "/attributes", None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "{attributes}");
    let (status, attribute_details) = send(c, tenant, Method::GET, "/attributes?details=true", None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "{attribute_details}");
    let mut type_attributes: Vec<(String, Vec<String>)> = details
        .as_array()
        .expect("an array of EntityType")
        .iter()
        .map(|entry| (entry["typeName"].as_str().expect("typeName").to_owned(), strings(&entry["attributeNames"])))
        .collect();
    type_attributes.sort();
    let mut attribute_types: Vec<(String, Vec<String>)> = attribute_details
        .as_array()
        .expect("an array of Attribute")
        .iter()
        .map(|entry| (entry["attributeName"].as_str().expect("attributeName").to_owned(), strings(&entry["typeNames"])))
        .collect();
    attribute_types.sort();
    Answers {
        types: strings(&types["typeList"]),
        type_attributes,
        attributes: strings(&attributes["attributeList"]),
        attribute_types,
    }
}

/// `(entityCount, [(attribute, sorted families)])` of one type, or `None` on 404.
async fn type_info(c: &Client, tenant: &str, entity_type: &str) -> Option<(u64, Vec<(String, Vec<String>)>)> {
    let (status, body) = send(c, tenant, Method::GET, &format!("/types/{}", segment(entity_type)), None).await;
    if status == StatusCode::NOT_FOUND {
        return None;
    }
    pretty_assertions::assert_eq!(status, StatusCode::OK, "{body}");
    let mut details: Vec<(String, Vec<String>)> = body["attributeDetails"]
        .as_array()
        .expect("attributeDetails")
        .iter()
        .map(|detail| {
            (
                detail["attributeName"].as_str().expect("attributeName").to_owned(),
                strings(&detail["attributeTypes"]),
            )
        })
        .collect();
    details.sort();
    Some((body["entityCount"].as_u64().expect("entityCount"), details))
}

/// `(attributeCount, sorted attributeTypes, sorted typeNames)` of one attribute, or `None` on 404.
async fn attribute_info(c: &Client, tenant: &str, attribute: &str) -> Option<(u64, Vec<String>, Vec<String>)> {
    let (status, body) = send(c, tenant, Method::GET, &format!("/attributes/{}", segment(attribute)), None).await;
    if status == StatusCode::NOT_FOUND {
        return None;
    }
    pretty_assertions::assert_eq!(status, StatusCode::OK, "{body}");
    Some((
        body["attributeCount"].as_u64().expect("attributeCount"),
        strings(&body["attributeTypes"]),
        strings(&body["typeNames"]),
    ))
}

/// The census the broker kept equals the one its search rows recompute to.
async fn assert_census_exact(env: &TenantScopedEnvironment, step: &str) {
    let pool = env.tenant_database().await.expect("the tenant database");
    let drift = census_drift(&pool, "public").await.expect("the census comparison");
    assert!(drift.is_empty(), "{step}: the census drifted from the search rows: {drift:?}");
    pool.close().await;
}

fn owned(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn families(pairs: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
    pairs.iter().map(|(name, families)| ((*name).to_owned(), owned(families))).collect()
}

/// The attribute map of a type grows when one entity of it gains an attribute
/// the others lack, and shrinks when the last carrier loses it again.
#[tokio::test]
async fn discovery_census_follows_an_attribute_map_that_grows_and_shrinks() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let tenant = env.tenant();
    provision_tenant(&c, tenant).await;
    let (x, y, z) = (env.id("census:x"), env.id("census:y"), env.id("census:z"));
    for id in [&x, &z] {
        create(&c, tenant, &json!({"id": id, "type": T, A: property(1), B: relationship(&y), C: property(2)})).await;
    }
    assert_census_exact(&env, "two entities").await;
    pretty_assertions::assert_eq!(
        type_info(&c, tenant, T).await,
        Some((2, families(&[(A, &["Property"]), (B, &["Relationship"]), (C, &["Property"])])))
    );

    create(
        &c,
        tenant,
        &json!({"id": y, "type": T, A: property(3), B: relationship(&x), C: property(4), D: point()}),
    )
    .await;
    assert_census_exact(&env, "a third entity with one more attribute").await;
    pretty_assertions::assert_eq!(
        type_info(&c, tenant, T).await,
        Some((
            3,
            families(&[(A, &["Property"]), (B, &["Relationship"]), (C, &["Property"]), (D, &["GeoProperty"])])
        ))
    );
    let seen = answers(&c, tenant).await;
    pretty_assertions::assert_eq!(seen.type_attributes, vec![(T.to_owned(), owned(&[A, B, C, D]))]);
    pretty_assertions::assert_eq!(seen.attributes, owned(&[A, B, C, D]));
    pretty_assertions::assert_eq!(attribute_info(&c, tenant, D).await, Some((1, owned(&["GeoProperty"]), owned(&[T]))));

    let (status, body) = send(&c, tenant, Method::DELETE, &format!("/entities/{y}/attrs/{}", segment(D)), None).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_census_exact(&env, "the only carrier lost the attribute").await;
    pretty_assertions::assert_eq!(attribute_info(&c, tenant, D).await, None);
    let seen = answers(&c, tenant).await;
    pretty_assertions::assert_eq!(seen.type_attributes, vec![(T.to_owned(), owned(&[A, B, C]))]);
    pretty_assertions::assert_eq!(seen.attributes, owned(&[A, B, C]));

    let (status, body) = send(&c, tenant, Method::POST, &format!("/entities/{x}/attrs"), Some(&json!({D: property(5)}))).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_census_exact(&env, "another entity gained the attribute as a Property").await;
    pretty_assertions::assert_eq!(attribute_info(&c, tenant, D).await, Some((1, owned(&["Property"]), owned(&[T]))));

    let (status, body) = send(&c, tenant, Method::DELETE, &format!("/entities/{x}"), None).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_census_exact(&env, "the carrier was deleted").await;
    pretty_assertions::assert_eq!(attribute_info(&c, tenant, D).await, None);
    pretty_assertions::assert_eq!(type_info(&c, tenant, T).await.map(|(count, _)| count), Some(2));

    for id in [&y, &z] {
        send(&c, tenant, Method::DELETE, &format!("/entities/{id}"), None).await;
    }
    assert_census_exact(&env, "every entity was deleted").await;
    let seen = answers(&c, tenant).await;
    assert!(
        seen.types.is_empty() && seen.attributes.is_empty(),
        "nothing exists any more: {:?} {:?}",
        seen.types,
        seen.attributes
    );
    pretty_assertions::assert_eq!(type_info(&c, tenant, T).await, None);
    env.finish().await.expect("cleanup failed");
}

/// Families are reported per type, an attribute's count is its instances (a
/// `datasetId` instance apart, a two-typed entity once), and `scope` is an
/// entity member, not an attribute.
#[tokio::test]
async fn discovery_census_scopes_families_per_type_and_counts_every_instance_once() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let tenant = env.tenant();
    provision_tenant(&c, tenant).await;
    let ids: Vec<String> = (1..=5).map(|n| env.id(&format!("census:e{n}"))).collect();
    create(&c, tenant, &json!({"id": ids[0], "type": T, A: property(1), B: relationship(&ids[1])})).await;
    create(
        &c,
        tenant,
        &json!({"id": ids[1], "type": T, A: property(2), B: relationship(&ids[0]), C: point()}),
    )
    .await;
    create(&c, tenant, &json!({"id": ids[2], "type": U, A: relationship(&ids[0])})).await;
    create(
        &c,
        tenant,
        &json!({"id": ids[3], "type": [T, U], A: [property(4), {"type": "Property", "value": 5, "datasetId": "urn:ngsi-ld:Dataset:x"}]}),
    )
    .await;
    create(&c, tenant, &json!({"id": ids[4], "type": T, "scope": "/census/x", A: property(6)})).await;
    assert_census_exact(&env, "five entities").await;

    pretty_assertions::assert_eq!(
        type_info(&c, tenant, T).await,
        Some((4, families(&[(A, &["Property"]), (B, &["Relationship"]), (C, &["GeoProperty"])])))
    );
    pretty_assertions::assert_eq!(type_info(&c, tenant, U).await, Some((2, families(&[(A, &["Property", "Relationship"])]))));
    pretty_assertions::assert_eq!(
        attribute_info(&c, tenant, A).await,
        Some((6, owned(&["Property", "Relationship"]), owned(&[T, U]))),
        "six instances: one per entity, two on the datasetId entity, which carries two types and counts once"
    );
    let seen = answers(&c, tenant).await;
    pretty_assertions::assert_eq!(seen.types, owned(&[T, U]));
    pretty_assertions::assert_eq!(seen.attributes, owned(&[A, B, C]), "scope is not an attribute");
    assert!(!seen.attributes.iter().any(|name| name == SCOPE || name == "scope"));
    pretty_assertions::assert_eq!(seen.type_attributes, vec![(T.to_owned(), owned(&[A, B, C])), (U.to_owned(), owned(&[A]))]);
    pretty_assertions::assert_eq!(
        seen.attribute_types,
        vec![(A.to_owned(), owned(&[T, U])), (B.to_owned(), owned(&[T])), (C.to_owned(), owned(&[T]))]
    );

    let (status, body) = send(
        &c,
        tenant,
        Method::DELETE,
        &format!("/entities/{}/attrs/{}?datasetId=urn:ngsi-ld:Dataset:x", ids[3], segment(A)),
        None,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_census_exact(&env, "one datasetId instance removed").await;
    pretty_assertions::assert_eq!(attribute_info(&c, tenant, A).await.map(|(count, _, _)| count), Some(5));

    for id in &ids[2..4] {
        send(&c, tenant, Method::DELETE, &format!("/entities/{id}"), None).await;
    }
    assert_census_exact(&env, "every entity of U deleted").await;
    pretty_assertions::assert_eq!(type_info(&c, tenant, U).await, None);
    pretty_assertions::assert_eq!(attribute_info(&c, tenant, A).await, Some((3, owned(&["Property"]), owned(&[T]))));
    env.finish().await.expect("cleanup failed");
}

/// A type's families follow every change of an instance's family, and a family
/// stays reported while any entity of the type still carries it.
#[tokio::test]
async fn discovery_census_follows_a_change_of_attribute_family() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let tenant = env.tenant();
    provision_tenant(&c, tenant).await;
    let (x, z) = (env.id("census:fx"), env.id("census:fz"));
    create(&c, tenant, &json!({"id": x, "type": T, A: property(1)})).await;
    create(&c, tenant, &json!({"id": z, "type": T, A: property(2)})).await;

    let (status, body) = send(&c, tenant, Method::POST, &format!("/entities/{x}/attrs"), Some(&json!({A: relationship(&z)}))).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_census_exact(&env, "one instance became a Relationship").await;
    pretty_assertions::assert_eq!(type_info(&c, tenant, T).await, Some((2, families(&[(A, &["Property", "Relationship"])]))));

    let (status, body) = send(&c, tenant, Method::POST, &format!("/entities/{z}/attrs"), Some(&json!({A: relationship(&x)}))).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_census_exact(&env, "every instance became a Relationship").await;
    pretty_assertions::assert_eq!(type_info(&c, tenant, T).await, Some((2, families(&[(A, &["Relationship"])]))));
    pretty_assertions::assert_eq!(attribute_info(&c, tenant, A).await, Some((2, owned(&["Relationship"]), owned(&[T]))));
    env.finish().await.expect("cleanup failed");
}

/// Replacing an entity by a batch upsert in `replace` mode drops the types and
/// attributes it no longer states (clause 4.16), and a merge adds a type.
#[tokio::test]
async fn discovery_census_follows_types_replaced_and_added() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let tenant = env.tenant();
    provision_tenant(&c, tenant).await;
    let (x, y) = (env.id("census:tx"), env.id("census:ty"));
    create(&c, tenant, &json!({"id": x, "type": T, A: property(1)})).await;
    create(&c, tenant, &json!({"id": y, "type": [T, U], A: property(2), B: property(3)})).await;

    let replacement = json!([{"id": y, "type": V, C: property(4)}]);
    let (status, body) = send(&c, tenant, Method::POST, "/entityOperations/upsert?options=replace", Some(&replacement)).await;
    assert!(
        status == StatusCode::NO_CONTENT || status == StatusCode::CREATED,
        "upsert replace: {status} {body}"
    );
    assert_census_exact(&env, "a batch upsert replaced one entity").await;
    pretty_assertions::assert_eq!(type_info(&c, tenant, U).await, None);
    pretty_assertions::assert_eq!(type_info(&c, tenant, V).await, Some((1, families(&[(C, &["Property"])]))));
    pretty_assertions::assert_eq!(type_info(&c, tenant, T).await, Some((1, families(&[(A, &["Property"])]))));
    pretty_assertions::assert_eq!(attribute_info(&c, tenant, B).await, None);

    let merge = json!({"type": [T, U]});
    let (status, body) = send(&c, tenant, Method::PATCH, &format!("/entities/{x}"), Some(&merge)).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_census_exact(&env, "a merge added a type").await;
    pretty_assertions::assert_eq!(type_info(&c, tenant, U).await, Some((1, families(&[(A, &["Property"])]))));
    pretty_assertions::assert_eq!(attribute_info(&c, tenant, A).await, Some((1, owned(&["Property"]), owned(&[T, U]))));
    env.finish().await.expect("cleanup failed");
}

/// A write that changes nothing the census counts adds nothing to it: a create
/// of an entity that exists, a batch that creates one entity and skips one,
/// and a value-only update.
#[tokio::test]
async fn discovery_census_is_unmoved_by_writes_that_change_no_type_or_instance() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let tenant = env.tenant();
    provision_tenant(&c, tenant).await;
    let (x, y) = (env.id("census:nx"), env.id("census:ny"));
    let entity = json!({"id": x, "type": T, A: property(1)});
    create(&c, tenant, &entity).await;
    let pool = env.tenant_database().await.expect("the tenant database");
    let rows_before = census_rows(&pool, "public").await.expect("census rows");

    let (status, _) = send(&c, tenant, Method::POST, "/entities", Some(&entity)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = send(&c, tenant, Method::PATCH, &format!("/entities/{x}/attrs"), Some(&json!({A: property(9)}))).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);
    pretty_assertions::assert_eq!(
        census_rows(&pool, "public").await.expect("census rows"),
        rows_before,
        "neither a refused create nor a value-only update counts anything"
    );

    let batch = json!([entity, {"id": y, "type": T, A: property(2)}]);
    let (status, body) = send(&c, tenant, Method::POST, "/entityOperations/create", Some(&batch)).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS, "{body}");
    assert_census_exact(&env, "a batch that created one entity and skipped one").await;
    pretty_assertions::assert_eq!(type_info(&c, tenant, T).await.map(|(count, _)| count), Some(2));
    pool.close().await;
    env.finish().await.expect("cleanup failed");
}

/// A batch of many entities in one statement is counted exactly, and so is
/// the batch delete that removes them.
#[tokio::test]
async fn discovery_census_counts_a_large_batch_and_its_deletion() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let tenant = env.tenant();
    provision_tenant(&c, tenant).await;
    let ids: Vec<String> = (0..300).map(|n| env.id(&format!("census:batch{n}"))).collect();
    let batch: Vec<Value> = ids
        .iter()
        .enumerate()
        .map(|(n, id)| {
            if n % 3 == 0 {
                json!({"id": id, "type": [T, U], A: property(1), B: [property(2), {"type": "Property", "value": 3, "datasetId": "urn:ngsi-ld:Dataset:b"}]})
            } else {
                json!({"id": id, "type": T, A: relationship(&ids[0])})
            }
        })
        .collect();
    let (status, body) = send(&c, tenant, Method::POST, "/entityOperations/create", Some(&Value::Array(batch))).await;
    assert!(
        status == StatusCode::CREATED || status == StatusCode::NO_CONTENT,
        "batch create: {status} {body}"
    );
    assert_census_exact(&env, "a batch of three hundred").await;
    pretty_assertions::assert_eq!(type_info(&c, tenant, T).await.map(|(count, _)| count), Some(300));
    pretty_assertions::assert_eq!(type_info(&c, tenant, U).await.map(|(count, _)| count), Some(100));
    pretty_assertions::assert_eq!(attribute_info(&c, tenant, B).await, Some((200, owned(&["Property"]), owned(&[T, U]))));
    pretty_assertions::assert_eq!(
        attribute_info(&c, tenant, A).await,
        Some((300, owned(&["Property", "Relationship"]), owned(&[T, U])))
    );

    let doomed: Vec<Value> = ids.iter().take(150).map(|id| Value::String(id.clone())).collect();
    let (status, body) = send(&c, tenant, Method::POST, "/entityOperations/delete", Some(&Value::Array(doomed))).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_census_exact(&env, "half of the batch deleted").await;
    pretty_assertions::assert_eq!(type_info(&c, tenant, T).await.map(|(count, _)| count), Some(150));
    pretty_assertions::assert_eq!(type_info(&c, tenant, U).await.map(|(count, _)| count), Some(50));
    env.finish().await.expect("cleanup failed");
}

/// A fold changes no answer and leaves one ledger row per live key.
#[tokio::test]
async fn discovery_census_fold_preserves_every_answer() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let tenant = env.tenant();
    provision_tenant(&c, tenant).await;
    for n in 0..12 {
        let id = env.id(&format!("census:fold{n}"));
        create(&c, tenant, &json!({"id": id, "type": T, A: property(n)})).await;
        if n % 4 == 0 {
            send(&c, tenant, Method::DELETE, &format!("/entities/{id}"), None).await;
        }
    }
    let before = (
        answers(&c, tenant).await.type_attributes,
        type_info(&c, tenant, T).await,
        attribute_info(&c, tenant, A).await,
    );
    let pool = env.tenant_database().await.expect("the tenant database");
    let outcome = fold_census(&pool, &ContextTarget::Live).await.expect("the fold");
    pretty_assertions::assert_eq!(outcome.negative_sums, 0);
    pretty_assertions::assert_eq!(
        census_rows(&pool, "public").await.expect("census rows"),
        3,
        "one row per live key: T, (T, a, Property), a"
    );
    assert_census_exact(&env, "after the fold").await;
    let after = (
        answers(&c, tenant).await.type_attributes,
        type_info(&c, tenant, T).await,
        attribute_info(&c, tenant, A).await,
    );
    pretty_assertions::assert_eq!(after, before);
    pretty_assertions::assert_eq!(
        fold_census(&pool, &ContextTarget::Live).await.expect("a second fold").summed_rows,
        0,
        "a folded census folds nothing"
    );
    pool.close().await;
    env.finish().await.expect("cleanup failed");
}

/// An entity created through the Temporal API exists in current state, and
/// discovery counts it exactly as one created through the Core API.
#[tokio::test]
async fn discovery_census_counts_an_entity_the_temporal_api_created() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let tenant = env.tenant();
    provision_tenant(&c, tenant).await;
    let id = env.id("census:temporal");
    let evolution = json!({"id": id, "type": T, A: [{"type": "Property", "value": 1, "observedAt": "2026-01-01T00:00:00Z"}]});
    let (status, body) = send(&c, tenant, Method::POST, "/temporal/entities", Some(&evolution)).await;
    assert!(
        status == StatusCode::CREATED || status == StatusCode::NO_CONTENT,
        "temporal create: {status} {body}"
    );
    assert_census_exact(&env, "a temporal create").await;
    let (current, _) = send(&c, tenant, Method::GET, &format!("/entities/{id}"), None).await;
    let counted = type_info(&c, tenant, T).await.map(|(count, _)| count);
    pretty_assertions::assert_eq!(
        counted,
        (current == StatusCode::OK).then_some(1),
        "discovery reports exactly what current state holds"
    );
    env.finish().await.expect("cleanup failed");
}

/// An attribute instance past its `expiresAt` is counted until the expiry sweep
/// removes it (clause 4.22 lets the removal lag), and not after.
#[tokio::test]
async fn discovery_census_forgets_an_attribute_the_expiry_sweep_removed() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let tenant = env.tenant();
    provision_tenant(&c, tenant).await;
    let id = env.id("census:expiry");
    let expires = instant_text(Utc::now() + TimeDelta::seconds(2));
    create(
        &c,
        tenant,
        &json!({"id": id, "type": T, A: property(1), D: {"type": "Property", "value": 2, "expiresAt": expires}}),
    )
    .await;
    pretty_assertions::assert_eq!(attribute_info(&c, tenant, D).await.map(|(count, _, _)| count), Some(1));
    let mut gone = false;
    for _ in 0..60 {
        sleep(Duration::from_secs(1)).await;
        if attribute_info(&c, tenant, D).await.is_none() {
            gone = true;
            break;
        }
    }
    assert!(gone, "the swept attribute is still reported");
    assert_census_exact(&env, "after the expiry sweep").await;
    pretty_assertions::assert_eq!(type_info(&c, tenant, T).await, Some((1, families(&[(A, &["Property"])]))));
    env.finish().await.expect("cleanup failed");
}
