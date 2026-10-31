//! A context source info value urn ngsi ld request copies the header of the triggering request.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    header_condition::HeaderCondition,
    namespace::Namespace,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::QueryEntities},
    model::Lossless,
    query::builders::SelectorTypeBuilder,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities carrying the header X-User abcd over a registration whose `contextSourceInfo` maps X-User
/// to urn:ngsi-ld:request forwards the query with X-User abcd
/// Clause: 6.3.19, 4.3.6.5
#[gtest]
#[tokio::test]
async fn n_fed_qry_7207_a_context_source_info_value_urn_ngsi_ld_request_copies_the_header_of_the_triggering_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let source_entities = [];
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .context_source_info(("X-User", "urn:ngsi-ld:request"))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let selector = SelectorTypeBuilder::from(&vehicle_type).build()?;
    // The client sends no header of its own, so the `X-User` field is added to the request the operation builds.
    let request = broker
        .with_accept(Accept::any())
        .unchecked(&QueryEntities::builder().entity_type(&selector).format(Lossless::Normalized).build())?
        .append_header(HeaderName::from_static("x-user"), HeaderValue::from_static("abcd"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let forwarded_request =
        SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities").header(HeaderCondition::Is(HeaderName::from_static("x-user"), "abcd".to_owned()));
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
