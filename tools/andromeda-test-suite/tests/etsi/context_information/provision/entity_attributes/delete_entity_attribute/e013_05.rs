//! ETSI `013_05`: an `observationSpace` geospatial Property is deleted from an entity (clauses 5.6.5 and 4.7).

use crate::{
    suite_keywords::{create_entity_with_context_string, delete_entities, naming_delete_all_false, retrieve_entity_json},
    suite_payloads::point,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::DeleteAttribute},
    model::{AttributeName, Entity},
};
use serde_json::json;

/// The terms of the `@context` the case registers.
const TERMS: [&str; 1] = ["Building"];

/// Pins: Check that one can delete an observationSpace geospatial Property from an entity
/// Clause: 5.6.5, 4.7
#[gtest]
#[tokio::test]
async fn e013_05_delete_an_observation_space_geospatial_property_from_an_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let id = namespace.urn("Building", 1)?;
    let building = Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .geo_property("observationSpace", point(13.3986, 52.5547)?)
        .build()?;
    create_entity_with_context_string(&broker, &vocabulary, &building).await?;
    let observation_space: AttributeName = "observationSpace".parse()?;

    let operation = DeleteAttribute::builder().id(&id).name(&observation_space).build();
    let deleted = broker.send_unchecked(naming_delete_all_false(&broker, &operation)?).await;
    verify_that!(&deleted, answered(StatusCode::NO_CONTENT))?;
    expect_that!(deleted?.body().len(), eq(0));

    let client = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let body = retrieve_entity_json(&client, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
        }))
    );

    delete_entities(&broker, &[&id]).await;
    Ok(())
}
