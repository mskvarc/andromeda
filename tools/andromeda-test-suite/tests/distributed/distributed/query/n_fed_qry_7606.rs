//! A profile link of an earlier version is accepted.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, PeerRole},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{
        headers::{Accept, Link, LinkValue, RelationType},
        operation::QueryEntities,
    },
    model::{Entity, Lossless},
    query::builders::SelectorTypeBuilder,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities on broker A by type with a profile Link naming NGSI-LD 1.8, over a local Entity and one
/// broker B serves through an inclusive registration, answers 200 with both
/// Clause: 6.3.21, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_fed_qry_7606_a_profile_link_of_an_earlier_version_is_accepted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 6)
        .build()?;
    broker_b.entities().create(&vehicle_2).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_2_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let profile = Link::try_new(
        LinkValue::builder(IriRefBuf::new("https://uri.etsi.org/ngsi-ld/profile/1.8".to_owned()).ok().or_fail()?)
            .rel(RelationType::Profile)
            .build(),
    )?;
    let selector = SelectorTypeBuilder::from(&vehicle_type).build()?;
    // The client names a profile only as the type of a context it links, so the profile link is set on the request.
    let request = broker
        .with_accept(Accept::any())
        .unchecked(&QueryEntities::builder().entity_type(&selector).format(Lossless::Normalized).build())?
        .typed_header(profile);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let found = entities(answer.as_ref().or_fail()?)?;
    expect_that!(found, len(eq(2)));
    expect_that!(found, contains(has_id(&vehicle_id)));
    expect_that!(found, contains(has_id(&vehicle_2_id)));
    Ok(())
}
