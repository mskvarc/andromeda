//! A replacement naming the NGSI-LD Null as its type is bad request data.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::ReplaceEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType},
};

/// Pins: Replace Entity with a body of type "urn:ngsi-ld:null" and `speed` 60 answers 400 `BadRequestData`, and the
/// Entity still holds `speed` 50
/// Clause: 5.5.4, 5.6.18.4
#[gtest]
#[tokio::test]
async fn n_ent_rpl_9012_a_replacement_typed_as_the_null_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "replaced-null-typed")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let replacement = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 60)
        .build()?;
    // The model holds a type name, so the Null is written as text; Replace Entity names its target in the path only,
    // so the body carries no `id`.
    let document = UncheckedDocument::from_entity(&replacement, Lossless::Normalized)
        .remove_member("id")
        .set_text("type", "urn:ngsi-ld:null");
    let request = broker.unchecked(&ReplaceEntity::builder().entity(&replacement).build())?.document(&document);
    expect_that!(
        &broker.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_attribute("speed", any_attribute().value(50)));
    Ok(())
}
