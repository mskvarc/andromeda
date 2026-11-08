//! The Location of an Entity names its id percent-encoded as one path segment.

use andromeda_test_suite::{
    broker,
    followed_location::{identifier, request},
    namespace::Namespace,
    problem::answered,
    segment_identifier::segment_identifiers,
    wire::header_value,
};
use googletest::prelude::*;
use http::{StatusCode, header::LOCATION};
use ngsi_ld::model::{Entity, EntityId};

/// Pins: Create Entity with an id holding /, ?, #, %41 or a non-ASCII character answers a Location ending in
/// /ngsi-ld/v1/entities/ and the id percent-encoded as one path segment, and a GET on that Location answers the
/// Entity with that id
/// Clause: 6.4.3.1, 6.5.3.1
/// Recorded: IETF RFC 3986 section 3.3 admits only pchar in a path segment, so the relative path names the created
/// Entity only with every other octet of its id percent-encoded, % included
#[gtest]
#[tokio::test]
async fn n_cmn_hdr_9131_the_location_of_an_entity_names_its_id_percent_encoded_as_one_path_segment() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    for segment_identifier in segment_identifiers(&namespace, "Building") {
        let building_id = EntityId::verbatim(&segment_identifier.id)?;
        let building = Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "located")
            .build()?;
        let created = broker.with_responses().entities().create(&building).send().await?;
        expect_that!(
            header_value(created.head.headers(), &LOCATION),
            some(ends_with(format!("/ngsi-ld/v1/entities/{}", segment_identifier.segment)))
        );
        let answer = broker.send_unchecked(request(&broker, created.head.headers())?).await;
        expect_that!(&answer, answered(StatusCode::OK));
        expect_that!(identifier(&answer?)?, eq(building_id.as_uri()));
    }
    Ok(())
}
