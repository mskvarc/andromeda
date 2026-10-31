//! A context source info accept sets the forwarded accept.

use andromeda_test_suite::{broker, context_source::ContextSource, forwarded_head, namespace::Namespace, source_answer};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities with Accept application/ld+json over a registration whose `contextSourceInfo` accept is
/// application/json is forwarded once with Accept application/json
/// Clause: 4.3.6.6, 6.3.19
#[gtest]
#[tokio::test]
async fn n_fed_qry_6414_a_context_source_info_accept_sets_the_forwarded_accept() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let source_entities = [];
    source
        .answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .context_source_info(("accept", "application/json"))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    let forwarded = source.received_on(Method::GET, "/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(
        forwarded_head::head(forwarded_one).typed::<Accept>().valid(),
        some(eq(&Accept::new(NgsiLdMediaType::Json)))
    );
    Ok(())
}
