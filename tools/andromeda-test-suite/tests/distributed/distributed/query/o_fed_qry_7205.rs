//! A query asking for key values is forwarded without a representation parameter.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, source_answer, source_request::SourceRequest};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type with options `keyValues` over a matching registration forwards the query once with
/// neither options nor format, so the source answers its default normalized representation
/// Clause: 6.3.7, 5.7.2.4
/// Recorded: clause 6.3.7 requires a source to support only the normalized representation by default and does not
/// say which representation a broker requests when merging
#[gtest]
#[tokio::test]
async fn o_fed_qry_7205_a_query_asking_for_key_values_is_forwarded_without_a_representation_parameter() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_entities = [];
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    broker
        .with_accept(Accept::any())
        .entities()
        .query_simplified()
        .entity_type(&vehicle_type)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let forwarded_request = SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities")
        .without_query("options")
        .without_query("format");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
