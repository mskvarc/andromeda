//! Update attributes with a fragment id naming another entity is bad request data.

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
    http::{headers::Accept, operation::UpdateAttributes},
    model::{Entity, EntityFragment, ErrorType, NgsiLdMediaType},
};

/// Pins: a Update Entity Attributes whose Fragment id names another Entity than the target is refused with 400
/// `BadRequestData`, and the target is unchanged
/// Clause: 5.4, 5.5.4, 5.6.2.4, 6.6.3.2
#[gtest]
#[tokio::test]
async fn n_cmn_val_0004_update_attributes_with_a_fragment_id_naming_another_entity_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 55)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder().id(vehicle_2_id.clone()).property("speed", 99).build()?;
    // The operation refuses a fragment naming another entity than its path; it is built for the fragment's entity and sent to the target's path.
    let request = broker
        .unchecked(&UpdateAttributes::builder().id(&vehicle_2_id).fragment(&fragment).build())?
        .uri(api_uri(&broker, &format!("entities/{vehicle_id}/attrs"))?);
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
