//! Clause C.2.4: the merged `@context` behind clause C.2's examples.
//!
//! The clause prints the single `@context` obtained by dereferencing and
//! merging the three vocabulary URLs the C.2 examples reference, so what it
//! demonstrates is that the two dispositions are interchangeable: the same
//! Entity written under the three separate documents reads back identically
//! under the one merged document.

use crate::{
    clean_tenant,
    client,
    comparison::{ContextMember, EntityOrder, assert_matches_example, corpus},
    context_link,
    create_entity,
    example_context::{HostedExampleContexts, hosted_contexts},
    get_json,
};
use reqwest::StatusCode;
use serde_json::Value;
use serial_test::serial;
use url::Url;

#[tokio::test]
#[serial]
async fn annex_c_2_4_merged_context_renders_the_same_terms_as_its_parts() {
    let environment = clean_tenant().await;
    let hosted = hosted_contexts();
    let separate = context_link(hosted.url("aggregatedContext.jsonld").as_str());

    // Written under the three separate vocabulary documents.
    let vehicle = corpus(include_str!("corpus/c-2-2-normalized-unitless.json"));
    create_entity(&client(), &vehicle, Some(&separate)).await;

    // Read back under the one merged document clause C.2.4 prints. Only the
    // terms C.2.4 actually defines can be asked for, so the Entity is trimmed
    // to those: the merged listing states in its own NOTE 1 that it omits the
    // API terms, and it omits `route`, `street`, `category` and the `Person`
    // the `passengers` Relationship names outright.
    let merged = context_link(merged_context_url(hosted).as_str());
    let (status, actual) = get_json(
        &client(),
        "/entities/urn:ngsi-ld:Vehicle:A4567?attrs=brandName,isParked",
        "application/ld+json",
        Some(&merged),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "retrieval under the merged @context failed: {actual}");

    let expected = trimmed_vehicle(&vehicle);
    assert_matches_example("C.2.4 merged @context", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.reset_and_finish().await.expect("tenant reset");
}

/// The hosted URL of the merged `@context` document clause C.2.4 prints.
fn merged_context_url(hosted: &HostedExampleContexts) -> &Url {
    hosted.url("merged-context.jsonld")
}

/// The Vehicle reduced to the two Attributes the merged `@context` names.
fn trimmed_vehicle(vehicle: &Value) -> Value {
    let members = ["id", "type", "brandName", "isParked"];
    Value::Object(
        vehicle
            .as_object()
            .expect("the corpus Vehicle is a JSON object")
            .iter()
            .filter(|(key, _)| members.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone())) // clone: the trimmed copy owns its members.
            .collect(),
    )
}
