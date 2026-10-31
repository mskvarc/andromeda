//! A posted query of several selectors reaches a get source once per selector it covers.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities posted with selectors of two types reaches a queryEntity source registered for both through one forwarded GET per type, and an Entity both answers is returned once
/// Clause: 5.7.2.4, 5.12, 5.2.33, 4.3.6.1
/// Tenant: private (the broker forwards a POST query of several selectors to registrations of other types too, so in
/// the shared tenant other files' sources answer it with their Entities)
#[gtest]
#[tokio::test]
async fn n_fed_qry_9022_a_posted_query_of_several_selectors_reaches_a_get_source_once_per_selector_it_covers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let both_id = namespace.urn("Both", 1)?;
    let alpha_type = namespace.entity_type("Alpha")?;
    let beta_type = namespace.entity_type("Beta")?;
    let registration_id = namespace.registration_id(1)?;
    let source_entities = [Entity::output_builder()
        .id(both_id.clone())
        .entity_type(alpha_type.clone())
        .property("name", "both")
        .build()?];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/c/ngsi-ld/v1/entities").query_containing("type", &alpha_type),
            source_answer::entities(&source_entities),
        )
        .await?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/c/ngsi-ld/v1/entities").query_containing("type", &beta_type),
            source_answer::entities(&source_entities),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("c")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(alpha_type.clone()), EntityInfo::new(beta_type.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let selectors = [
        EntitySelector::new(SelectorTypeBuilder::from(&alpha_type).build()?),
        EntitySelector::new(SelectorTypeBuilder::from(&beta_type).build()?),
    ];
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .selectors(selectors)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&both_id)));
    let alpha_requests = source
        .received_matching(&SourceRequest::new(Method::GET, "/c/ngsi-ld/v1/entities").query_containing("type", &alpha_type))
        .await?;
    expect_that!(alpha_requests, len(eq(1)));
    let beta_requests = source
        .received_matching(&SourceRequest::new(Method::GET, "/c/ngsi-ld/v1/entities").query_containing("type", &beta_type))
        .await?;
    expect_that!(beta_requests, len(eq(1)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
