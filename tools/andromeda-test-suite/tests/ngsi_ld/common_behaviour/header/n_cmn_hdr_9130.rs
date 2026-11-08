//! The Location of an Entity whose id holds a slash leads back to the Entity.

use andromeda_test_suite::{broker, followed_location::request, namespace::Namespace, problem::answered, wire::header_value};
use googletest::prelude::*;
use http::{StatusCode, header::LOCATION};
use ngsi_ld::model::{Entity, EntityId, Lossless, Output};
use std::str;

/// Pins: Create Entity with the id urn:ngsi-ld:Building:<token>-x/y answers a Location ending in
/// /ngsi-ld/v1/entities/urn:ngsi-ld:Building:<token>-x%2Fy, and a GET on that Location answers the Entity
/// Clause: 6.4.3.1, 6.5.3.1
/// Recorded: IETF RFC 3986 section 3.3 keeps / out of a path segment, so the relative path names the created Entity
/// only with the slash of its id percent-encoded
#[gtest]
#[tokio::test]
async fn n_cmn_hdr_9130_the_location_of_an_entity_whose_id_holds_a_slash_leads_back_to_the_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = EntityId::verbatim(&namespace.urn_text("Building", "x/y"))?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(namespace.entity_type("Building")?)
        .property("name", "slashed")
        .build()?;
    let created = broker.with_responses().entities().create(&building).send().await?;
    expect_that!(
        header_value(created.head.headers(), &LOCATION),
        some(ends_with(format!("/ngsi-ld/v1/entities/{}", namespace.urn_text("Building", "x%2Fy"))))
    );
    let answer = broker.send_unchecked(request(&broker, created.head.headers())?).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let retrieved = Entity::<Output>::parse(str::from_utf8(answer?.body())?, Lossless::Normalized)?;
    expect_that!(retrieved.id, eq(&building_id));
    Ok(())
}
