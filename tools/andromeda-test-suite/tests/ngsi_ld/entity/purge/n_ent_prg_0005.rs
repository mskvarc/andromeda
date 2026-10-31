//! A q traversing a linked entity is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Purge Entities whose q filter traverses a Relationship into a Linked Entity is refused with 400
/// `BadRequestData` and deletes nothing
/// Clause: 5.6.21.4, 4.9, 6.4.3.3
#[gtest]
#[tokio::test]
async fn n_ent_prg_0005_a_q_traversing_a_linked_entity_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let parking_id = namespace.urn("Parking", 1)?;
    let parking_type = namespace.entity_type("Parking")?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type)
            .relationship("isParkedIn", parking_id.clone())
            .build()?,
        Entity::builder()
            .id(parking_id)
            .entity_type(parking_type)
            .property("category", "commercial")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // The purge parameter model reads no linked-entity q, so the request is sent as written.
    let request = request_to(
        &broker,
        Method::DELETE,
        &format!("entities?type=Building_{namespace}&q=isParkedIn%7Bcategory%3D%3D%22commercial%22%7D&local=true"),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    Ok(())
}
