//! Replace entity with a fragment id naming another entity is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ReplaceEntity},
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: a Replace Entity whose Fragment id names another Entity than the target is refused with 400
/// `BadRequestData`, and the target is unchanged
/// Clause: 5.4, 5.5.4, 5.6.18.4, 6.5.3.3
#[gtest]
#[tokio::test]
async fn n_cmn_val_0006_replace_entity_with_a_fragment_id_naming_another_entity_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 55)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The operation writes its path from the body's id; the request goes to the path the test names.
    let replacement = Entity::builder()
        .id(vehicle_2_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 99)
        .build()?;
    let request = broker
        .unchecked(&ReplaceEntity::builder().entity(&replacement).build())?
        .uri(api_uri(&broker, &format!("entities/{vehicle_id}"))?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&vehicle_id));
    expect_that!(entity, lacks_attribute("fuel"));
    expect_that!(entity, has_attribute("speed", any_attribute().value(55)));
    Ok(())
}
