//! Append attributes with a fragment id percent encoding a character is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, EntityId, ErrorType, NgsiLdMediaType},
};

/// Pins: an Append Entity Attributes whose Fragment id percent-encodes a hyphen of the target's id is refused with
/// 400 `BadRequestData`, and the target is unchanged
/// Clause: 5.4, 5.5.4, 5.6.3.4, 6.6.3.1
/// Recorded: clause 5.4 asks the Fragment id to be "equal to the id of the target (mutated) NGSI-LD element"
/// without defining equality; Andromeda reads it as IETF RFC 8141 section 3.1 URN-equivalence, under which
/// percent-encoded characters are not decoded, so an encoded hyphen names another Entity
#[gtest]
#[tokio::test]
async fn o_cmn_val_2702_append_attributes_with_a_fragment_id_percent_encoding_a_character_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 55)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .id(format!("urn:ngsi-ld:Vehicle:{namespace}%2D1").parse::<EntityId>()?)
        .property("fuel", 40)
        .build()?;
    let answer = broker.with_responses().entities().append_attributes(&vehicle_id, &fragment).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("fuel"));
    expect_that!(entity, has_attribute("speed", any_attribute().value(55)));
    Ok(())
}
