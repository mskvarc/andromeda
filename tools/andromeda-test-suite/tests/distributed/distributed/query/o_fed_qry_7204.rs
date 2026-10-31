//! The context source filter is forwarded unchanged to a matched source.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, source_answer, source_request::SourceRequest};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    query::q,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type with csf mode=="inclusive" over a matching inclusive registration forwards the
/// query once with the same csf
/// Clause: 5.7.2.4, 6.3.18
/// Recorded: the specification defines csf as a filter over Context Source Registrations and does not say whether
/// it travels to the matched source
#[gtest]
#[tokio::test]
async fn o_fed_qry_7204_the_context_source_filter_is_forwarded_unchanged_to_a_matched_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let source_entities = [];
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .csf(q::attr("mode").eq("inclusive"))
        .send()
        .await?;
    let forwarded_request = SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities").query("csf", r#"mode=="inclusive""#);
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
