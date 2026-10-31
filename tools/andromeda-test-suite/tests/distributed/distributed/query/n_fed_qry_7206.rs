//! A query forwarded through a second broker reaches the source with both broker aliases in the via header in order.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, PeerRole},
    context_source::ContextSource,
    forwarded_head,
    namespace::Namespace,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::{Accept, ReceivedBy},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Entities by type through a registration at a second broker whose own registration points at a source
/// reaches that source once with a Via header naming the first broker and then the second
/// Clause: 6.3.18, 4.3.6.4
#[gtest]
#[tokio::test]
async fn n_fed_qry_7206_a_query_forwarded_through_a_second_broker_reaches_the_source_with_both_broker_aliases_in_the_via_header_in_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker_b.registrations().register(&registration).send().await?;
    let registration_2 = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let source_entities = [];
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let hosts = forwarded_head::via_hosts(forwarded_one).or_fail()?;
    let broker_a_alias = "broker-a".parse::<ReceivedBy>()?;
    let broker_b_alias = "broker-b".parse::<ReceivedBy>()?;
    let first = hosts.iter().position(|host| *host == broker_a_alias).or_fail()?;
    let second = hosts.iter().position(|host| *host == broker_b_alias).or_fail()?;
    expect_that!(first < second, eq(true));
    Ok(())
}
