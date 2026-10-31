//! A redirect registration creates merges and deletes the entity on the peer.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Entity, Merge Entity and Delete Entity on broker A under a redirect registration of broker B with
/// `redirectionOps` each act on broker B, which reads the Entity back after the create with its speed, after the
/// merge with the merged speed and not at all after the delete, while broker A holds nothing locally
/// Clause: 4.3.6.3, 5.6.1.4, 5.6.17.4, 5.6.6.4
#[gtest]
#[tokio::test]
async fn n_fed_wrt_8000_a_redirect_registration_creates_merges_and_deletes_the_entity_on_the_peer() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([OperationGroup::RedirectionOps])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("speed", 21)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker_b.with_accept(Accept::any()).entities().retrieve(&building_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(21)));
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&building_id)
        .locality(Locality::Local)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let fragment = EntityFragment::builder().property("speed", 22).build()?;
    broker.entities().merge(&building_id, &fragment).send().await?.done()?;
    let retrieved_2 = broker_b.with_accept(Accept::any()).entities().retrieve(&building_id).send().await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("speed", any_attribute().value(22)));
    broker.entities().delete(&building_id).send().await?.done()?;
    let answer_2 = broker_b
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&building_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
