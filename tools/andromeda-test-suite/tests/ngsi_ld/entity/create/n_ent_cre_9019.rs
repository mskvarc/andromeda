//! A create refuses the null under a key of a property value.

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

/// Pins: Create Entity whose Property value holds urn:ngsi-ld:null under a key answers 400 `BadRequestData` and
/// stores nothing
/// Clause: 5.5.4, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9019_a_create_refuses_the_null_under_a_key_of_a_property_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_create_nested_null_id = namespace.urn("Building", "create-nested-null")?;
    let building_type = namespace.entity_type("Building")?;
    let building_create_nested_null = Entity::builder()
        .id(building_create_nested_null_id.clone())
        .entity_type(building_type)
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in a member of `value` at address), so `address` is written as text.
    let address: RawJson = r#"{"type": "Property", "value": {"country": "urn:ngsi-ld:null"}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building_create_nested_null, Lossless::Normalized).set_member("address", &address);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_create_nested_null).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_create_nested_null_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
