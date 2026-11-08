//! Each tenant forwards in the tenant its own registration names.

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

/// Pins: Query Entities in a tenant whose registration names the tenant remote-ns is forwarded with NGSILD-Tenant
/// remote-ns, and the same query in the default tenant over a registration naming no tenant is forwarded without
/// NGSILD-Tenant
/// Clause: 4.3.6.4, 4.14, 5.2.9
/// Tenant: private (clause 4.3.6.4 makes each tenant federate on its own, and the subject is a registration of a
/// tenant other than the default one)
#[gtest]
#[tokio::test]
async fn n_fed_qry_6419_each_tenant_forwards_in_the_tenant_its_own_registration_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let source_entities = [];
    source
        .answer(Method::GET, "/t/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let source_entities_2 = [];
    source
        .answer(Method::GET, "/d/ngsi-ld/v1/entities", source_answer::entities(&source_entities_2))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("t")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    // The model refuses it (a registration tenant), so `tenant` is written as text.
    let tenant_2: RawJson = format!(r#""remote{namespace}""#).parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("tenant", &tenant_2);
    let request = tenant.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = tenant.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let _registration_cleanup = Cleanup::of(&tenant).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.endpoint("d")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    tenant.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    let forwarded_request = SourceRequest::new(Method::GET, "/t/ngsi-ld/v1/entities")
        .header(HeaderCondition::Is(HeaderName::from_static("ngsild-tenant"), format!("remote{namespace}")));
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_2 = source.received_on(Method::GET, "/d/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded_2, len(eq(1)));
    let forwarded_request_2 =
        SourceRequest::new(Method::GET, "/d/ngsi-ld/v1/entities").header(HeaderCondition::Absent(HeaderName::from_static("ngsild-tenant")));
    let forwarded_3 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_3, len(eq(1)));
    Ok(())
}
