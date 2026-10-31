//! A csf is passed on to the discovery a peer performs.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::{BrokerRole, PeerRole},
    context_source::ContextSource,
    entity_matcher::has_attribute,
    forwarded_query::query_values,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    query::q,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Entities with csf mode=="inclusive" over a registration of broker B is answered with the Entity
/// behind broker B, and broker B passes the same csf on to its own registered source
/// Clause: 5.7.2.4, 4.3.6.5
#[gtest]
#[tokio::test]
async fn n_fed_qry_7408_a_csf_is_passed_on_to_the_discovery_a_peer_performs() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker_b.registrations().register(&registration_2).send().await?;
    let source_entities = [Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?];
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .csf(q::attr("mode").eq("inclusive"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_id)));
    expect_that!(page.items(), nth(0, has_attribute("speed", any_attribute().value(50))));
    let forwarded = source.wait_for_on(Method::GET, "/a/ngsi-ld/v1/entities", 1).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(query_values(forwarded_one, "csf"), nth(0, eq(r#"mode=="inclusive""#)));
    broker_b.registrations().delete(&registration_2_id).send().await?;
    Ok(())
}
