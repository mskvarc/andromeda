//! A discovered registration names the peer endpoint that holds the entity.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, PeerRole},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, OneOrMany, Scope},
    query::scope_query::ScopeQuery,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Context Source Registrations on broker A by type and `scopeQ` /Europe/Madrid/# answers the one
/// registration scoped /Europe/Madrid with broker B as its endpoint, and Query Entities by type on broker B answers
/// the Entity it holds
/// Clause: 5.10.2.4, 4.19, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_csr_qry_8000_a_discovered_registration_names_the_peer_endpoint_that_holds_the_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration_id = namespace.registration_id(1)?;
    let peer_b = broker::peer(PeerRole::B)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("speed", 11)
        .build()?;
    broker_b.entities().create(&building).send().await?.done()?;
    let registration = CSourceRegistration::builder(peer_b.clone())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .scope(OneOrMany::try_from(vec!["/Europe/Madrid".parse::<Scope>()?])?)
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&building_type)
        .scope_q("/Europe/Madrid/#".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&registration_id)));
    expect_that!(page.items().first().map(CSourceRegistration::endpoint), some(eq(&peer_b)));
    let entities = broker_b
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(entities.items(), len(eq(1)));
    expect_that!(entities.items(), contains(has_id(&building_id)));
    Ok(())
}
