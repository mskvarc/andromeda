//! A scope query reaches only the source registered under a matching scope.

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
    model::{Entity, RawJson},
    query::scope_query::ScopeQuery,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities on broker A by type and `scopeQ` /Europe/Madrid/# over inclusive registrations of brokers
/// B, C and D scoped /Europe/Madrid, /Europe/Berlin and /Europe/Nordic, each broker holding one Entity in its own
/// scope, answers the Entity of broker B alone
/// Clause: 5.7.2.4, 5.10.2.4, 4.19
#[gtest]
#[tokio::test]
async fn n_fed_qry_8001_a_scope_query_reaches_only_the_source_registered_under_a_matching_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let building_b_id = namespace.urn("Building", "b")?;
    let building_type = namespace.entity_type("Building")?;
    let broker_c = broker::client(BrokerRole::C)?;
    let building_c_id = namespace.urn("Building", "c")?;
    let broker_d = broker::client(BrokerRole::D)?;
    let building_d_id = namespace.urn("Building", "d")?;
    let registration_b_id = namespace.registration_id("b")?;
    let registration_c_id = namespace.registration_id("c")?;
    let registration_d_id = namespace.registration_id("d")?;
    let building_b = Entity::builder()
        .id(building_b_id.clone())
        .entity_type(building_type.clone())
        .scope(["/Europe/Madrid"])
        .property("speed", 1)
        .build()?;
    broker_b.entities().create(&building_b).send().await?.done()?;
    let building_c = Entity::builder()
        .id(building_c_id)
        .entity_type(building_type.clone())
        .scope(["/Europe/Berlin"])
        .property("speed", 1)
        .build()?;
    broker_c.entities().create(&building_c).send().await?.done()?;
    let building_d = Entity::builder()
        .id(building_d_id)
        .entity_type(building_type.clone())
        .scope(["/Europe/Nordic"])
        .property("speed", 1)
        .build()?;
    broker_d.entities().create(&building_d).send().await?.done()?;
    let registration_b = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_b_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    // The model refuses it (a registration scope), so `scope` is written as text.
    let scope: RawJson = r#"["/Europe/Madrid"]"#.parse()?;
    let document = UncheckedDocument::try_from(registration_b.render())?.set_member("scope", &scope);
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
    // The model refuses it (a registration scope), so `scope` is written as text.
    let scope_2: RawJson = r#"["/Europe/Berlin"]"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_c.render())?.set_member("scope", &scope_2);
    let request_2 = broker
        .unchecked(&RegisterContextSource { registration: &registration_c })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let _registration_c_cleanup = Cleanup::of(&broker).registration(&registration_c_id);
    let registration_d = CSourceRegistration::builder(broker::peer(PeerRole::D)?)
        .id(registration_d_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    // The model refuses it (a registration scope), so `scope` is written as text.
    let scope_3: RawJson = r#"["/Europe/Nordic"]"#.parse()?;
    let document_3 = UncheckedDocument::try_from(registration_d.render())?.set_member("scope", &scope_3);
    let request_3 = broker
        .unchecked(&RegisterContextSource { registration: &registration_d })?
        .document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, answered(StatusCode::CREATED));
    let _registration_d_cleanup = Cleanup::of(&broker).registration(&registration_d_id);
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&building_type)
        .scope_q("/Europe/Madrid/#".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_b_id)));
    Ok(())
}
