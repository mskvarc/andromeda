//! A registration naming a tenant reads the peer entity of that tenant only.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    entity_matcher::has_attribute,
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
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities on broker A in the default tenant, under an inclusive registration of broker B naming the
/// tenant the file creates on B, answers the Entity B holds in that tenant and not the Entity of the same id B
/// holds in its default tenant
/// Clause: 4.14, 4.3.6.4, 5.2.9, 5.7.2.4
/// Tenant: private (the subject is a registration naming a Tenant of the peer, which comes into existence only by a
/// write that targets it)
#[gtest]
#[tokio::test]
async fn n_fed_qry_8007_a_registration_naming_a_tenant_reads_the_peer_entity_of_that_tenant_only() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_b_tenant = broker::private_tenant(&broker_b, &namespace).await?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration_id = namespace.registration_id(1)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("speed", 42)
        .build()?;
    broker_b_tenant.entities().create(&building).send().await?.done()?;
    let building_2 = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("speed", 41)
        .build()?;
    broker_b.entities().create(&building_2).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    // The model refuses it (a registration tenant), so `tenant` is written as text.
    let tenant: RawJson = format!(r#""{namespace}""#).parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("tenant", &tenant);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&building_type).send().await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_attribute("speed", any_attribute().value(42))));
    Ok(())
}
