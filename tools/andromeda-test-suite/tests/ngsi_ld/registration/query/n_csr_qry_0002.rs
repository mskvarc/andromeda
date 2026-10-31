//! A geo query reads the footprint of a three dimensional registration location.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, identity_matcher::has_id, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{
        NgsiLdMediaType,
        RawJson,
        geometry::{Position, Shape},
    },
    query::geoquery,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: a registration query with an intersects `geoQ` over a Point answers the registration whose three-ordinate
/// Polygon location covers the Point and the one with no location, and not the one whose Polygon lies elsewhere
/// Clause: 5.10.2, 5.2.9, 4.10, 5.12
/// Recorded: clause 5.10.2.4 matches the geoquery against the location of the registration "if the location
/// directly matches or if the location possibly contains locations that would match"; it does not say what a
/// registration with no location does, and Andromeda keeps it, as one that makes no spatial claim; the Polygon at
/// 500 m matching the Point rests on intersects ignoring altitude: clause 4.10 defers the geometric relationships
/// to OGC 06-103r4, whose clause 6.1.2.5 says spatial operations work in the "map geometry" of the data and "will
/// therefore not reflect z or m values in calculations" and whose clause 6.1.15.1 defines the relationships by the
/// projection of the objects onto the horizontal surface, so a georel that ignores the altitude is the specified
/// behaviour; the user ruled this on 2026-10-06
#[gtest]
#[tokio::test]
async fn n_csr_qry_0002_a_geo_query_reads_the_footprint_of_a_three_dimensional_registration_location() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_covering_id = namespace.registration_id("covering")?;
    let building_type = namespace.entity_type("Building")?;
    let registration_distant_id = namespace.registration_id("distant")?;
    let registration_unlocated_id = namespace.registration_id("unlocated")?;
    let registration_covering = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_covering_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .build()?;
    // The model refuses it (a registration location), so `location` is written as text.
    let location: RawJson =
        r#"{"type": "Polygon", "coordinates": [[[13.0, 52.0, 500.0], [14.0, 52.0, 500.0], [14.0, 53.0, 500.0], [13.0, 53.0, 500.0], [13.0, 52.0, 500.0]]]}"#
            .parse()?;
    let document = UncheckedDocument::try_from(registration_covering.render())?.set_member("location", &location);
    let request = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_covering,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_covering_cleanup = Cleanup::of(&broker).registration(&registration_covering_id);
    let registration_distant = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_distant_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .build()?;
    // The model refuses it (a registration location), so `location` is written as text.
    let location_2: RawJson = r#"{"type": "Polygon", "coordinates": [[[30.0, 10.0], [31.0, 10.0], [31.0, 11.0], [30.0, 11.0], [30.0, 10.0]]]}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_distant.render())?.set_member("location", &location_2);
    let request_2 = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_distant,
        })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let _registration_distant_cleanup = Cleanup::of(&broker).registration(&registration_distant_id);
    let registration_unlocated = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_unlocated_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_unlocated).send().await?;
    let _registration_unlocated_cleanup = Cleanup::of(&broker).registration(&registration_unlocated_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .geo_q(geoquery::intersects(Shape::Point(Position::try_new(13.4, 52.52)?)))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&registration_covering_id)));
    expect_that!(page.items(), contains(has_id(&registration_unlocated_id)));
    Ok(())
}
