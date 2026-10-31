//! A geoquery matches a registration whose location possibly contains a match.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{
        RawJson,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::geoquery,
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by type and a geoquery answers each registration whose location
/// possibly contains a location the geoquery matches: within a box around Madrid, the area covering Spain and the
/// point at Madrid and not the area around Berlin; containing the point at Madrid, the area covering Spain and that
/// point; disjoint from the box, the two areas and not the point inside it
/// Clause: 5.10.2.4, 4.10, 5.12
/// Recorded: clause 5.10.2.4 matches the geoquery against the registration location "if the location directly
/// matches or if the location possibly contains locations that would match the geoquery"; an area holds a location
/// within the box when it intersects the box, a location containing the point when the area contains the point, and
/// a location disjoint from the box when the area is not within the box
#[gtest]
#[tokio::test]
async fn n_csr_qry_9009_a_geoquery_matches_a_registration_whose_location_possibly_contains_a_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_spain_id = namespace.registration_id("spain")?;
    let building_type = namespace.entity_type("Building")?;
    let registration_berlin_id = namespace.registration_id("berlin")?;
    let registration_madrid_id = namespace.registration_id("madrid")?;
    let registration_spain = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_spain_id.clone())
        .information([RegistrationInfo::builder().entities([building_type.as_str()]).build()?])
        .build()?;
    // The model refuses it (a registration location), so `location` is written as text.
    let location: RawJson = r#"{"type": "Polygon", "coordinates": [[[-9.0, 36.0], [3.0, 36.0], [3.0, 44.0], [-9.0, 44.0], [-9.0, 36.0]]]}"#.parse()?;
    let document = UncheckedDocument::try_from(registration_spain.render())?.set_member("location", &location);
    let request = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_spain,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_spain_cleanup = Cleanup::of(&broker).registration(&registration_spain_id);
    let registration_berlin = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_berlin_id.clone())
        .information([RegistrationInfo::builder().entities([building_type.as_str()]).build()?])
        .build()?;
    // The model refuses it (a registration location), so `location` is written as text.
    let location_2: RawJson = r#"{"type": "Polygon", "coordinates": [[[12.0, 51.0], [15.0, 51.0], [15.0, 54.0], [12.0, 54.0], [12.0, 51.0]]]}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_berlin.render())?.set_member("location", &location_2);
    let request_2 = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_berlin,
        })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let _registration_berlin_cleanup = Cleanup::of(&broker).registration(&registration_berlin_id);
    let registration_madrid = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_madrid_id.clone())
        .information([RegistrationInfo::builder().entities([building_type.as_str()]).build()?])
        .build()?;
    // The model refuses it (a registration location), so `location` is written as text.
    let location_3: RawJson = r#"{"type": "Point", "coordinates": [-3.7, 40.4]}"#.parse()?;
    let document_3 = UncheckedDocument::try_from(registration_madrid.render())?.set_member("location", &location_3);
    let request_3 = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_madrid,
        })?
        .document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, answered(StatusCode::CREATED));
    let _registration_madrid_cleanup = Cleanup::of(&broker).registration(&registration_madrid_id);
    let page = broker
        .with_accept(Accept::any())
        .registrations()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(-4.2, 39.9)?,
            Position::try_new(-3.2, 39.9)?,
            Position::try_new(-3.2, 40.9)?,
            Position::try_new(-4.2, 40.9)?,
            Position::try_new(-4.2, 39.9)?,
        ])?])?)))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&registration_spain_id)));
    expect_that!(page.items(), contains(has_id(&registration_madrid_id)));
    let page_2 = broker
        .with_accept(Accept::any())
        .registrations()
        .query()
        .geo_q(geoquery::contains(Shape::Point(Position::try_new(-3.7, 40.4)?)))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&registration_spain_id)));
    expect_that!(page_2.items(), contains(has_id(&registration_madrid_id)));
    let page_3 = broker
        .with_accept(Accept::any())
        .registrations()
        .query()
        .geo_q(geoquery::disjoint(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(-4.2, 39.9)?,
            Position::try_new(-3.2, 39.9)?,
            Position::try_new(-3.2, 40.9)?,
            Position::try_new(-4.2, 40.9)?,
            Position::try_new(-4.2, 39.9)?,
        ])?])?)))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), contains(has_id(&registration_spain_id)));
    expect_that!(page_3.items(), contains(has_id(&registration_berlin_id)));
    broker.registrations().delete(&registration_spain_id).send().await?;
    broker.registrations().delete(&registration_berlin_id).send().await?;
    broker.registrations().delete(&registration_madrid_id).send().await?;
    Ok(())
}
