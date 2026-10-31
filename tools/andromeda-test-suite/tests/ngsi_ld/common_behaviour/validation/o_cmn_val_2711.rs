//! Replace entity with a fragment id percent encoding a character is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ReplaceEntity},
    model::{Entity, EntityId, ErrorType, NgsiLdMediaType},
};

/// Pins: a Replace Entity whose Fragment id percent-encodes a hyphen of the target's id is refused with 400
/// `BadRequestData`, and the target is unchanged
/// Clause: 5.4, 5.5.4, 5.6.18.4, 6.5.3.3
/// Recorded: clause 5.4 asks the Fragment id to be "equal to the id of the target (mutated) NGSI-LD element"
/// without defining equality; Andromeda reads it as IETF RFC 8141 section 3.1 URN-equivalence, under which
/// percent-encoded characters are not decoded, so an encoded hyphen names another Entity
#[gtest]
#[tokio::test]
async fn o_cmn_val_2711_replace_entity_with_a_fragment_id_percent_encoding_a_character_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 55)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The operation writes its path from the body's id; the request goes to the path the test names.
    let replacement = Entity::builder()
        .id(format!("urn:ngsi-ld:Vehicle:{namespace}%2D1").parse::<EntityId>()?)
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
    expect_that!(entity, has_attribute("speed", any_attribute().value(55)));
    Ok(())
}
