//! Purge is forwarded to every matching registration advertising it.

use andromeda_test_suite::{broker, context_source::ContextSource, forwarded_query::query_values, namespace::Namespace, outcome::Done, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode};

/// Pins: Purge Entities by type under two redirect registrations advertising `purgeEntity` answers 204 and forwards
/// one purge carrying the type selector to each source
/// Clause: 5.6.21.4, 4.3.6.3
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6249_purge_is_forwarded_to_every_matching_registration_advertising_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::PurgeEntity, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("s2")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::PurgeEntity, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    source
        .answer(Method::DELETE, "/s1/ngsi-ld/v1/entities", source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    source
        .answer(Method::DELETE, "/s2/ngsi-ld/v1/entities", source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    broker.entities().purge().entity_type(&vehicle_type).send().await?.done()?;
    let forwarded = source.received_on(Method::DELETE, "/s1/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(query_values(forwarded_one, "type"), not(is_empty()));
    let forwarded_2 = source.received_on(Method::DELETE, "/s2/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded_2, len(eq(1)));
    let forwarded_one_2 = forwarded_2.first().or_fail()?;
    expect_that!(query_values(forwarded_one_2, "type"), not(is_empty()));
    Ok(())
}
