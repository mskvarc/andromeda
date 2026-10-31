//! A csf forwards a query only to the matching registration.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{Entity, RawJson},
    query::q,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Entities with csf `sourceGroup=="alpha"` over two `queryEntity` registrations of the type, one whose
/// `sourceGroup` is alpha and one beta, is forwarded once to the alpha source and never to the beta source, and answers
/// the Entity the alpha source returns
/// Clause: 4.9, 5.7.2, 6.4.3.2, 5.12
#[gtest]
#[tokio::test]
async fn n_fed_qry_0002_a_csf_forwards_a_query_only_to_the_matching_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let device_type = namespace.entity_type("Device")?;
    let device_alpha_id = namespace.urn("Device", "alpha")?;
    let device_beta_id = namespace.urn("Device", "beta")?;
    let source_entities = [Entity::output_builder().id(device_alpha_id.clone()).entity_type(device_type.clone()).build()?];
    source
        .answer(Method::GET, "/alpha/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let source_entities_2 = [Entity::output_builder().id(device_beta_id).entity_type(device_type.clone()).build()?];
    source
        .answer(Method::GET, "/beta/ngsi-ld/v1/entities", source_answer::entities(&source_entities_2))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("alpha")?)
        .id(namespace.registration_id("alpha")?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(device_type.clone()).with_id_pattern(namespace.id_pattern("Device")?)])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    // The model refuses it (a registration property), so `sourceGroup` is written as text.
    let property_alpha: RawJson = r#"{"type": "Property", "value": "alpha"}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("sourceGroup", &property_alpha);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let registration_2 = CSourceRegistration::builder(source.endpoint("beta")?)
        .id(namespace.registration_id("beta")?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(device_type.clone()).with_id_pattern(namespace.id_pattern("Device")?)])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    // The model refuses it (a registration property), so `sourceGroup` is written as text.
    let property_beta: RawJson = r#"{"type": "Property", "value": "beta"}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_2.render())?.set_member("sourceGroup", &property_beta);
    let request_2 = broker
        .unchecked(&RegisterContextSource { registration: &registration_2 })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&device_type)
        .csf(q::attr("sourceGroup").eq("alpha"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&device_alpha_id)));
    let forwarded = SourceRequest::new(Method::GET, "/alpha/ngsi-ld/v1/entities");
    expect_that!(source.received_matching(&forwarded).await?, len(eq(1)));
    let forwarded_2 = SourceRequest::new(Method::GET, "/beta/ngsi-ld/v1/entities");
    expect_that!(source.received_matching(&forwarded_2).await?, len(eq(0)));
    Ok(())
}
