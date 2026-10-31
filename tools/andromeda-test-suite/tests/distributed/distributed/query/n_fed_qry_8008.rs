//! A within query reaches the source registered for an area reaching into it.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{
        Entity,
        RawJson,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::geoquery,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities on broker A by type within a box around Madrid, over inclusive registrations of broker B
/// located over an area covering Spain and of broker C located over an area around Berlin, each holding an Entity
/// located at Madrid, answers the Entity of broker B alone
/// Clause: 5.7.2.4, 5.10.2.4, 4.3.6.1, 4.10
/// Recorded: clause 5.10.2.4 matches the geoquery against the registration location "if the location directly
/// matches or if the location possibly contains locations that would match the geoquery"; the area covering Spain
/// is not within the box but holds locations that are, so broker B is asked, while no location of the area around
/// Berlin is within it, so broker C is not
#[gtest]
#[tokio::test]
async fn n_fed_qry_8008_a_within_query_reaches_the_source_registered_for_an_area_reaching_into_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let building_b_id = namespace.urn("Building", "b")?;
    let building_type = namespace.entity_type("Building")?;
    let broker_c = broker::client(BrokerRole::C)?;
    let building_c_id = namespace.urn("Building", "c")?;
    let registration_b_id = namespace.registration_id("b")?;
    let registration_c_id = namespace.registration_id("c")?;
    let building_b = Entity::builder()
        .id(building_b_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(-3.7, 40.4)?))
        .build()?;
    broker_b.entities().create(&building_b).send().await?.done()?;
    let building_c = Entity::builder()
        .id(building_c_id)
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(-3.7, 40.4)?))
        .build()?;
    broker_c.entities().create(&building_c).send().await?.done()?;
    let registration_b = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_b_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    // The model refuses it (a registration location), so `location` is written as text.
    let location: RawJson = r#"{"type": "Polygon", "coordinates": [[[-9.0, 36.0], [3.0, 36.0], [3.0, 44.0], [-9.0, 44.0], [-9.0, 36.0]]]}"#.parse()?;
    let document = UncheckedDocument::try_from(registration_b.render())?.set_member("location", &location);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration_b })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_b_cleanup = Cleanup::of(&broker).registration(&registration_b_id);
    let registration_c = CSourceRegistration::builder(broker::peer(PeerRole::C)?)
        .id(registration_c_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    // The model refuses it (a registration location), so `location` is written as text.
    let location_2: RawJson = r#"{"type": "Polygon", "coordinates": [[[12.0, 51.0], [15.0, 51.0], [15.0, 54.0], [12.0, 54.0], [12.0, 51.0]]]}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_c.render())?.set_member("location", &location_2);
    let request_2 = broker
        .unchecked(&RegisterContextSource { registration: &registration_c })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let _registration_c_cleanup = Cleanup::of(&broker).registration(&registration_c_id);
    let page = broker
        .with_accept(Accept::any())
        .entities()
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
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_b_id)));
    Ok(())
}
