//! Accepting geojson is not acceptable and lists the json representations.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    config::PeerRole,
    namespace::Namespace,
    raw_answer::send_raw,
    raw_call::target,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    model::geometry::{Position, Shape},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations on broker A by type with `geometryProperty` location and Accept
/// application/geo+json answers 406 listing application/json and application/ld+json as `availableRepresentations`
/// Clause: 6.3.4, 5.10.2
/// Recorded: 6.3.4 mandates the 406 and a body containing "the list of the available representations of the
/// resources" without fixing its shape; Andromeda answers `availableRepresentations`, the media types the endpoint
/// renders, in this order
#[gtest]
#[tokio::test]
async fn o_csr_qry_7606_accepting_geojson_is_not_acceptable_and_lists_the_json_representations() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "1-.*")?)])
            .build()?])
        .location(Shape::Point(Position::try_new(13.4, 52.5)?))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // The client takes only the media types the NGSI-LD API renders, so the GeoJSON `Accept` is sent as written.
    let type_text = vehicle_type.to_string();
    let request = request_to(
        &broker,
        Method::GET,
        &target("csourceRegistrations", &[("type", type_text.as_str()), ("geometryProperty", "location")]),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/geo+json"));
    let response = send_raw(request).await?;
    expect_that!(response.status(), eq(StatusCode::NOT_ACCEPTABLE));
    expect_that!(response.body(), json_count("$.availableRepresentations", 2));
    expect_that!(response.body(), json_equals("$.availableRepresentations[0]", "application/json"));
    expect_that!(response.body(), json_equals("$.availableRepresentations[1]", "application/ld+json"));
    Ok(())
}
