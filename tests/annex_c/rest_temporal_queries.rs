//! Clauses C.5.5, C.5.6, C.5.14 and C.5.16: the temporal REST examples.
//!
//! Every one of them queries the same one hour of `speed` observations on
//! Vehicle B9211, which is seeded through clause 5.6.11's temporal creation so
//! the observations carry exactly the `observedAt` instants the annex prints.

use crate::{
    BASE_URL,
    clean_tenant,
    comparison::{ContextMember, EntityOrder, assert_matches_example, corpus},
    context_link,
    example_context::hosted_contexts,
    get_json,
};
use pretty_assertions::assert_eq;
use reqwest::{Client, StatusCode};
use serde_json::Value;

/// The window every temporal example in clause C.5 asks for.
const WINDOW: &str = "timerel=between&timeAt=2018-08-01T12:00:00Z&endTimeAt=2018-08-01T13:00:00Z";

/// Seed the speed evolution and answer the wrapper `@context` link.
async fn seed_speed_evolution(http: &Client) -> String {
    let link = context_link(hosted_contexts().url("aggregatedContext.jsonld").as_str());
    create_temporal_entity(http, &corpus(include_str!("corpus/c-5-5-temporal-seed.json")), &link).await;
    link
}

/// Create a Temporal Representation through clause 5.6.11.
async fn create_temporal_entity(http: &Client, entity: &Value, link: &str) {
    let response = http
        .post(format!("{BASE_URL}/temporal/entities"))
        .header("Content-Type", "application/json")
        .header("Link", link)
        .json(entity)
        .send()
        .await
        .expect("temporal creation request");
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    assert!(
        status == StatusCode::CREATED || status == StatusCode::NO_CONTENT,
        "temporal seeding refused with {status}: {body}"
    );
}

/// Run one temporal query and return its answer.
async fn temporal_query(http: &Client, link: &str, query: &str) -> Value {
    let (status, body) = get_json(http, &format!("/temporal/entities/?{query}"), "application/ld+json", Some(link)).await;
    assert_eq!(status, StatusCode::OK, "temporal query failed: {body}");
    body
}

#[tokio::test]
async fn annex_c_5_5_temporal_query_of_the_speed_evolution() {
    let environment = clean_tenant().await;
    let link = seed_speed_evolution(&environment.client()).await;

    let actual = temporal_query(
        &environment.client(),
        &link,
        &format!("type=Vehicle&q=brandName!=Mercedes&attrs=speed&{WINDOW}"),
    )
    .await;
    let expected = corpus(include_str!("corpus/c-5-5-temporal-answer.json"));
    assert_matches_example("C.5.5 EXAMPLE 1", &expected, &actual, ContextMember::Ignored, EntityOrder::Unordered);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_5_5_temporal_query_of_the_speed_and_brand_name_evolution() {
    let environment = clean_tenant().await;
    let link = seed_speed_evolution(&environment.client()).await;

    // Clause C.5.5.1 EXAMPLE 2 asks for both Attributes and prints only
    // `speed`, on the stated grounds that "brandName attribute does not have
    // any temporal evolution". The seeded brandName observation lies outside
    // the requested window, which is the only way that premise holds for a
    // broker that historises every Attribute.
    let actual = temporal_query(
        &environment.client(),
        &link,
        &format!("type=Vehicle&q=brandName!=Mercedes&attrs=speed,brandName&{WINDOW}"),
    )
    .await;
    let expected = corpus(include_str!("corpus/c-5-5-temporal-answer.json"));
    assert_matches_example("C.5.5 EXAMPLE 2", &expected, &actual, ContextMember::Ignored, EntityOrder::Unordered);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_5_6_temporal_query_in_the_simplified_representation() {
    let environment = clean_tenant().await;
    let link = seed_speed_evolution(&environment.client()).await;

    let actual = temporal_query(
        &environment.client(),
        &link,
        &format!("type=Vehicle&q=brandName!=Mercedes&attrs=speed&{WINDOW}&format=temporalValues"),
    )
    .await;
    let expected = corpus(include_str!("corpus/c-5-6-temporal-values-answer.json"));
    assert_matches_example("C.5.6", &expected, &actual, ContextMember::Ignored, EntityOrder::Unordered);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_5_14_temporal_query_in_the_aggregated_representation() {
    let environment = clean_tenant().await;
    let link = seed_speed_evolution(&environment.client()).await;

    let query = format!("type=Vehicle&q=brandName!=Mercedes&attrs=speed&{WINDOW}&aggrMethods=max,avg&aggrPeriodDuration=PT4M&format=aggregatedValues");
    let actual = temporal_query(&environment.client(), &link, &query).await;
    let expected = corpus(include_str!("corpus/c-5-14-aggregated-answer.json"));
    assert_matches_example("C.5.14", &expected, &actual, ContextMember::Ignored, EntityOrder::Unordered);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_5_16_temporal_scope_queries_example() {
    let environment = clean_tenant().await;
    let link = context_link(hosted_contexts().url("aggregatedContext.jsonld").as_str());

    // B9211 was already in /Madrid/Centro before the window opens; A8311 only
    // enters it inside the window, so only its later observations are in scope.
    create_temporal_entity(
        &environment.client(),
        &scoped_vehicle(
            "B9211",
            "2018-08-01T11:00:00Z",
            "/Madrid/Centro",
            &[(30, "12:03"), (60, "12:05"), (50, "12:07")],
        ),
        &link,
    )
    .await;
    create_temporal_entity(
        &environment.client(),
        &scoped_vehicle_with_scopes(
            "A8311",
            "2018-08-01T12:10:00Z",
            &["/Madrid/Centro", "/Company123/UnitA"],
            &[(40, "12:12"), (60, "12:14"), (50, "12:16")],
        ),
        &link,
    )
    .await;

    // The printed request selects `attrs=speed,scope`, which Table 6.18.3.2-1
    // admits: it restricts `attrs` to "an Attribute (Property or Relationship)
    // name", and clause 4.18 makes the Scope "a special scope Property that is
    // non-reified in the normalized Entity representation and reified in the
    // temporal representation of an Entity". `pick` selects the same members by
    // the other lawful spelling, so both are replayed against the one answer.
    let expected = corpus(include_str!("corpus/c-5-16-temporal-answer.json"));
    for selection in ["attrs=speed,scope", "pick=id,type,speed,scope"] {
        let query = format!("type=Vehicle&{selection}&{WINDOW}&scopeQ=%2FMadrid%2FCentro&format=temporalValues");
        let actual = temporal_query(&environment.client(), &link, &query).await;
        assert_matches_example("C.5.16", &expected, &actual, ContextMember::Ignored, EntityOrder::Unordered);
    }

    environment.finish().await;
}

/// A temporal Vehicle payload carrying one scope value and a speed evolution.
fn scoped_vehicle(suffix: &str, scope_at: &str, scope: &str, speeds: &[(u32, &str)]) -> Value {
    scoped_vehicle_payload(suffix, scope_at, &Value::String(scope.to_owned()), speeds)
}

/// The same, for a Vehicle that is in several scopes at once.
fn scoped_vehicle_with_scopes(suffix: &str, scope_at: &str, scopes: &[&str], speeds: &[(u32, &str)]) -> Value {
    let scope = Value::Array(scopes.iter().map(|scope| Value::String((*scope).to_owned())).collect());
    scoped_vehicle_payload(suffix, scope_at, &scope, speeds)
}

/// Build the temporal creation payload the scope examples need.
fn scoped_vehicle_payload(suffix: &str, scope_at: &str, scope: &Value, speeds: &[(u32, &str)]) -> Value {
    let instances: Vec<Value> = speeds
        .iter()
        .map(|(value, minute)| serde_json::json!({"type": "Property", "value": value, "observedAt": format!("2018-08-01T{minute}:00Z")}))
        .collect();
    serde_json::json!({
        "id": format!("urn:ngsi-ld:Vehicle:{suffix}"),
        "type": "Vehicle",
        "scope": [{"type": "Property", "value": scope, "observedAt": scope_at}],
        "speed": instances
    })
}
