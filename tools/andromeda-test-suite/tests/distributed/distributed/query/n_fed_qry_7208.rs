//! A context source info entry cannot override the tenant or the content length of a forwarded request.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    header_condition::HeaderCondition,
    namespace::Namespace,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderName, Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::RawJson,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities over a registration whose `contextSourceInfo` names NGSILD-Tenant and Content-Length
/// forwards the query once with neither value
/// Clause: 6.3.19
#[gtest]
#[tokio::test]
async fn n_fed_qry_7208_a_context_source_info_entry_cannot_override_the_tenant_or_the_content_length_of_a_forwarded_request() -> Result<()> {
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
    // The model refuses it (a registration contextSourceInfo), so `contextSourceInfo` is written as text.
    let context_source_info: RawJson = r#"[{"key": "NGSILD-Tenant", "value": "evil-tenant"}, {"key": "Content-Length", "value": "999999"}]"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceInfo", &context_source_info);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_entities = [];
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_request = SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities")
        .header(HeaderCondition::Is(HeaderName::from_static("ngsild-tenant"), "evil-tenant".to_owned()));
    let forwarded_2 = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded_2, len(eq(0)));
    let forwarded_request_2 =
        SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities").header(HeaderCondition::Is(HeaderName::from_static("content-length"), "999999".to_owned()));
    let forwarded_3 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_3, len(eq(0)));
    Ok(())
}
