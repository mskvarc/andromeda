//! A three dimensional registration location reads back with its altitude.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, geometry_matcher::geometry, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: a Context Source Registration whose location is a Polygon with three-ordinate positions reads back with
/// every ordinate, altitude included
/// Clause: 5.2.9, 5.9.2, 5.10.1
/// Recorded: clause 5.2.9 types the location of a registration as a `GeoJSON` geometry; clause 4.7.2 acknowledges
/// an "optional altitude" in a geometry but leaves the degree of precision held to the implementation, and nothing
/// says a registration must return every ordinate it was given; Andromeda returns the geometry as written
#[gtest]
#[tokio::test]
async fn o_csr_ret_0001_a_three_dimensional_registration_location_reads_back_with_its_altitude() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .build()?;
    // The model refuses it (a registration location), so `location` is written as text.
    let location: RawJson =
        r#"{"type": "Polygon", "coordinates": [[[13.0, 52.0, 120.0], [14.0, 52.0, 120.0], [14.0, 53.0, 120.0], [13.0, 53.0, 120.0], [13.0, 52.0, 120.0]]]}"#
            .parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("location", &location);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(
        registration_2.location(),
        some(
            geometry()
                .of_type("Polygon")
                .length([], 1)
                .length([0], 5)
                .length([0, 0], 3)
                .ordinate([0, 0, 0], 13)
                .ordinate([0, 0, 1], 52)
                .ordinate([0, 0, 2], 120)
                .length([0, 1], 3)
                .ordinate([0, 1, 0], 14)
                .ordinate([0, 1, 1], 52)
                .ordinate([0, 1, 2], 120)
                .length([0, 2], 3)
                .ordinate([0, 2, 0], 14)
                .ordinate([0, 2, 1], 53)
                .ordinate([0, 2, 2], 120)
                .length([0, 3], 3)
                .ordinate([0, 3, 0], 13)
                .ordinate([0, 3, 1], 53)
                .ordinate([0, 3, 2], 120)
                .length([0, 4], 3)
                .ordinate([0, 4, 0], 13)
                .ordinate([0, 4, 1], 52)
                .ordinate([0, 4, 2], 120)
        )
    );
    Ok(())
}
