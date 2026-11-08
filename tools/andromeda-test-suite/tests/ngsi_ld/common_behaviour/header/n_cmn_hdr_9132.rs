//! The Location of a Temporal Evolution names its id percent-encoded as one path segment.

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
use ngsi_ld::model::{EntityId, Property, evolution::TemporalEntity};

/// Pins: Create or Update Temporal Evolution of an Entity with an id holding /, ?, #, %41 or a non-ASCII character
/// answers 201 with a Location ending in /ngsi-ld/v1/temporal/entities/ and the id percent-encoded as one path
/// segment, and a GET on that Location answers the Temporal Evolution with that id
/// Clause: 6.18.3.1, 6.19.3.1
/// Recorded: IETF RFC 3986 section 3.3 admits only pchar in a path segment, so the relative path names the created
/// Temporal Evolution only with every other octet of its id percent-encoded, % included
#[gtest]
#[tokio::test]
async fn n_cmn_hdr_9132_the_location_of_a_temporal_evolution_names_its_id_percent_encoded_as_one_path_segment() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    for segment_identifier in segment_identifiers(&namespace, "Sensor") {
        let sensor_id = EntityId::verbatim(&segment_identifier.id)?;
        let evolution = TemporalEntity::builder()
            .id(sensor_id.clone())
            .entity_type(sensor_type.clone())
            .property("reading", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
            .build()?;
        let created = broker.with_responses().temporal().create_or_update(&evolution).send().await?;
        expect_that!(created.status, eq(StatusCode::CREATED));
        expect_that!(
            header_value(created.head.headers(), &LOCATION),
            some(ends_with(format!("/ngsi-ld/v1/temporal/entities/{}", segment_identifier.segment)))
        );
        let answer = broker.send_unchecked(request(&broker, created.head.headers())?).await;
        expect_that!(&answer, answered(StatusCode::OK));
        expect_that!(identifier(&answer?)?, eq(sensor_id.as_uri()));
    }
    Ok(())
}
