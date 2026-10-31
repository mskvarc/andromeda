//! A posted query of several selectors reaches a batch source with the selectors it covers.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::NgsiLdMediaType,
    query::{EntitySelector, builders::SelectorTypeBuilder},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities posted with selectors of two types reaches a `queryBatch` source registered for one of them
/// through one forwarded POST whose entities hold only the selector of the type it covers
/// Clause: 5.7.2.4, 5.12, 5.2.33, 4.3.6.1
/// Tenant: private (it counts every request its context source receives, and the broker forwards a query of several
/// selectors to registrations of other types too, so the shared tenant's other such queries reach its source)
#[gtest]
#[tokio::test]
async fn n_fed_qry_9021_a_posted_query_of_several_selectors_reaches_a_batch_source_with_the_selectors_it_covers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let alpha_type = namespace.entity_type("Alpha")?;
    let beta_type = namespace.entity_type("Beta")?;
    let registration_id = namespace.registration_id(1)?;
    let source_entities = [];
    source
        .answer(Method::POST, "/b/ngsi-ld/v1/entityOperations/query", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("b")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(beta_type.clone())]).build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryBatch])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let selectors = [
        EntitySelector::new(SelectorTypeBuilder::from(&alpha_type).build()?),
        EntitySelector::new(SelectorTypeBuilder::from(&beta_type).build()?),
    ];
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .selectors(selectors)
        .send()
        .await?;
    let forwarded_request = SourceRequest::new(Method::POST, "/b/ngsi-ld/v1/entityOperations/query");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_body = forwarded.first().or_fail()?.body.as_slice();
    expect_that!(BodyPattern::Selects("$.entities[0]".parse()?).holds(forwarded_body), eq(true));
    expect_that!(BodyPattern::Selects("$.entities[1]".parse()?).holds(forwarded_body), eq(false));
    expect_that!(
        BodyPattern::Selects(format!("$.entities[?(@.type == 'Beta_{namespace}')]").parse()?).holds(forwarded_body),
        eq(true)
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
