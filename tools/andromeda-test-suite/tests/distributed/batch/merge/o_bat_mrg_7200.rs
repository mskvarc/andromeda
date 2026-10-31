//! A batch element without an id makes the batch bad request data and merges nothing.

use andromeda_test_suite::{attribute_matcher::property, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::BatchMerge},
    model::{Entity, EntityFragment, ErrorType, RawJson},
    query::Locality,
};

/// Pins: Batch Entity Merge with an element holding no id answers 400 `BadRequestData` and the existing Entity is
/// unchanged
/// Clause: 5.6.20.4, 5.5.4, 6.20.3.1
/// Recorded: clause 5.6.20.4 has the batch answer a `BatchOperationResult` for entities in error and does not say
/// whether an element without an id, which no `BatchEntityError` can name, refuses the whole batch
#[gtest]
#[tokio::test]
async fn o_bat_mrg_7200_a_batch_element_without_an_id_makes_the_batch_bad_request_data_and_merges_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let placeholder = EntityFragment::for_entity(vehicle_id.clone(), vehicle_type).build()?;
    // The model requires an id of a batch element, so the element is written as text with the attribute only.
    let speed: RawJson = r#"{"type": "Property", "value": 1}"#.parse()?;
    let document = UncheckedDocument::empty().set_member("speed", &speed);
    let fragments = [placeholder];
    let request = broker.unchecked(&BatchMerge::builder().fragments(&fragments).build())?.documents(&[document]);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(5)));
    Ok(())
}
