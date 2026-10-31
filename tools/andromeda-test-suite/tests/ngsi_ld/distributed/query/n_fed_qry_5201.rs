//! Query entities queries a registration by the expanded name of attrs.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    hosted_context::HostedContext,
    namespace::Namespace,
    outcome::Done,
    source_answer,
    source_request::SourceRequest,
    teardown::AnyAnswer,
};
use googletest::prelude::*;
use http::Method;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType},
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities with an attrs parameter queries an inclusive registration only when an attribute the
/// parameter names expands to a registered `propertyNames` IRI: another term for the registered IRI queries it, the
/// same short name under the default vocabulary or another context does not
/// Clause: 5.12, 4.3.6.2, 5.5.7, 5.7.2.4
/// Tenant: private (it counts every request its context source receives, and the discovery and unscoped requests of
/// other tests reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_fed_qry_5201_query_entities_queries_a_registration_by_the_expanded_name_of_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let context = Context::Multiple(vec![
        ContextEntry::Inline(format!(r#"{{"speed": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
        ContextEntry::Reference(IriRefBuf::new("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".to_owned())?),
    ]);
    let ctx = HostedContext::add(&broker, &context).await?;
    let context_2 = Context::Multiple(vec![
        ContextEntry::Inline(format!(r#"{{"speed": "https://other.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
        ContextEntry::Reference(IriRefBuf::new("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".to_owned())?),
    ]);
    let ctx_other = HostedContext::add(&broker, &context_2).await?;
    let context_3 = Context::Multiple(vec![
        ContextEntry::Inline(format!(r#"{{"velocity": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
        ContextEntry::Reference(IriRefBuf::new("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".to_owned())?),
    ]);
    let ctx_alias = HostedContext::add(&broker, &context_3).await?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let source_entities = [];
    source
        .answer_when(SourceRequest::any_matching("/.*").priority(1), source_answer::entities(&source_entities))
        .await?;
    let body_context = broker
        .with_context(Context::Single(ctx.entry()?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::RetrieveOps])
        .build()?;
    body_context.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&body_context).registration(&registration_id);
    // the registered IRI under another term: queried
    let linked = broker.with_context(Context::Single(ctx_alias.entry()?))?;
    linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .attrs(vec!["velocity".parse()?])
        .send()
        .await
        .any_answer();
    let forwarded_request = SourceRequest::matching(Method::GET, "/ngsi-ld/v1/entit.*");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    // the short name speed under the default vocabulary: another IRI, not queried
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?])
        .send()
        .await
        .any_answer();
    let forwarded_request_2 = SourceRequest::matching(Method::GET, "/ngsi-ld/v1/entit.*");
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(1)));
    // the short name speed under a context that maps it to another IRI: not queried
    let linked_2 = broker.with_context(Context::Single(ctx_other.entry()?))?;
    linked_2
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?])
        .send()
        .await
        .any_answer();
    let forwarded_request_3 = SourceRequest::matching(Method::GET, "/ngsi-ld/v1/entit.*");
    let forwarded_3 = source.received_matching(&forwarded_request_3).await?;
    expect_that!(forwarded_3, len(eq(1)));
    // the registered short name under the registration's own context: queried
    let linked_3 = broker.with_context(Context::Single(ctx.entry()?))?;
    linked_3
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?])
        .send()
        .await
        .any_answer();
    let forwarded_request_4 = SourceRequest::matching(Method::GET, "/ngsi-ld/v1/entit.*");
    let forwarded_4 = source.received_matching(&forwarded_request_4).await?;
    expect_that!(forwarded_4, len(eq(2)));
    Ok(())
}
