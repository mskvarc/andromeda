//! A list relationship carrying an object is refused.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity with a `ListRelationship` carrying object beside its `objectList` answers 400
/// `BadRequestData` and stores nothing
/// Clause: 4.5.22.2, 5.5.4, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_cmn_val_9006_a_list_relationship_carrying_an_object_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    // The model refuses it (member `object` is not allowed here at partOf), so `partOf` is written as text.
    let part_of: RawJson =
        format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Site:{namespace}-1"], "object": "urn:ngsi-ld:Site:{namespace}-2"}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&building, Lossless::Normalized).set_member("partOf", &part_of);
    let request = broker.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
