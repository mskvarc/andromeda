//! A query of context source registrations by type answers the registration whose property and relationship names match
//! the attributes it names, or every one of the type when it names none (clause 5.10.2).

use crate::{
    registration_operations::{building_registration_json, create_registration, csource_endpoint, delete_registrations},
    registration_payloads::building_registration,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::AttributeName;
use serde_json::json;

/// The terms of the cases' `@context`.
const TERMS: [&str; 4] = ["Building", "name", "subCategory", "locatedAt"];

/// Registers the vocabulary and the Building registration `-1`, queries it by type `Building` and `attrs`, requires
/// 200 and the registration alone, and deletes it.
async fn queried(attrs: &[&str]) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let _registration = create_registration(&broker, &vocabulary, &building_registration(&namespace, 1, &endpoint)?).await?;
    let attrs = attrs.iter().map(|name| Ok(name.parse::<AttributeName>()?)).collect::<Result<Vec<_>>>()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .registrations()
        .query()
        .entity_type("Building")
        .attrs(attrs)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([building_registration_json(&namespace, 1, &endpoint)]))
            .hiding_everywhere("mode")
            .hiding_everywhere("operations")
    );
    delete_registrations(&broker, &namespace, &[1]).await
}

/// Pins: Check that one can query context source registrations matching property and relationship names of
/// `RegistrationInfo`
/// Clause: 5.10.2
#[gtest]
#[tokio::test]
async fn e037_06_01_query_with_matching_properties_and_relationships() -> Result<()> {
    queried(&["name", "locatedAt"]).await
}

/// Pins: Check that one can query context source registrations matching property and relationship names of
/// `RegistrationInfo`
/// Clause: 5.10.2
#[gtest]
#[tokio::test]
async fn e037_06_02_query_without_properties_and_relationships() -> Result<()> {
    queried(&[]).await
}
