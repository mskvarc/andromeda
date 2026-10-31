//! An exclusive registration adds its id and attribute to a type only query.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    forwarded_query::query_values,
    namespace::Namespace,
    nth::nth,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type alone over an exclusive registration of one id and speed is forwarded once carrying
/// that type, id and speed
/// Clause: 5.7.2.4, 4.3.6.3
/// Recorded: 5.7.2.4 and 5.12 select the registrations to contact and do not say whether the forwarded request is
/// cut down to what a registration covers; Andromeda narrows type, ids and attribute names to the registration
#[gtest]
#[tokio::test]
async fn o_fed_qry_6406_an_exclusive_registration_adds_its_id_and_attribute_to_a_type_only_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let source_entities = [];
    source
        .answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    let forwarded = source.received_on(Method::GET, "/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(query_values(forwarded_one, "type"), nth(0, eq(format!("Vehicle_{namespace}").as_str())));
    expect_that!(
        query_values(forwarded_one, "id"),
        nth(0, eq(format!("urn:ngsi-ld:Vehicle:{namespace}-1").as_str()))
    );
    expect_that!(query_values(forwarded_one, "attrs"), nth(0, eq("speed")));
    Ok(())
}
