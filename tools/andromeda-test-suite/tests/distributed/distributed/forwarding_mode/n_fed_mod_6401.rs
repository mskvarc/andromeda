//! Two inclusive sources are each queried once and both contribute.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
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
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type over two inclusive registrations of the type is forwarded once to each source,
/// carrying the type, and answers the Entity each source returns
/// Clause: 4.3.6.2, 5.7.2.4, 5.12
#[gtest]
#[tokio::test]
async fn n_fed_mod_6401_two_inclusive_sources_are_each_queried_once_and_both_contribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let source_entities = [Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 11)
        .build()?];
    source
        .answer(Method::GET, "/1/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let source_entities_2 = [Entity::output_builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 22)
        .build()?];
    source
        .answer(Method::GET, "/2/ngsi-ld/v1/entities", source_answer::entities(&source_entities_2))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.endpoint("2")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&vehicle_id)));
    expect_that!(page.items(), contains(has_id(&vehicle_2_id)));
    let forwarded = source.received_on(Method::GET, "/1/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(query_values(forwarded_one, "type"), nth(0, contains_substring("Vehicle_")));
    let forwarded_2 = source.received_on(Method::GET, "/2/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded_2, len(eq(1)));
    let forwarded_one_2 = forwarded_2.first().or_fail()?;
    expect_that!(query_values(forwarded_one_2, "type"), nth(0, contains_substring("Vehicle_")));
    Ok(())
}
