//! Replace attribute is matched to an exclusive registration by expanded name.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::{has_attribute, lacks_attribute},
    hosted_context::HostedContext,
    namespace::Namespace,
    outcome::Done,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Attribute, AttributeName, Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType, Property},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Replace Attribute is matched against an exclusive registration's `propertyNames` by the expanded name of
/// the attribute in the path: a name that expands to the registered IRI is forwarded under whatever short form or
/// @context the request writes it, and a name of the same short form that expands to another IRI is replaced
/// locally and not forwarded
/// Clause: 5.12, 4.3.6.3, 5.5.7, 5.6.19.4
/// Tenant: private (it counts every request its context source receives, and the discovery and unscoped requests of
/// other tests reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_fed_wrt_5004_replace_attribute_is_matched_to_an_exclusive_registration_by_expanded_name() -> Result<()> {
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
    let body_context = broker
        .with_context(Context::Single(ctx.entry()?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("https://uri.etsi.org/ngsi-ld/default-context/speed", 2)
        .property(format!("https://other.andromeda/{namespace}/speed").as_str(), 3)
        .build()?;
    body_context.entities().create(&vehicle).send().await?.done()?;
    source
        .answer_when(SourceRequest::any_matching("/.*").priority(1), source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([OperationGroup::RedirectionOps])
        .build()?;
    body_context.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&body_context).registration(&registration_id);
    // the registered IRI under another term: forwarded
    let linked = broker.with_context(Context::Single(ctx_alias.entry()?))?;
    let attribute = Attribute::from(Property::builder(111).build()?);
    linked
        .entities()
        .replace_attribute(&vehicle_id, &"velocity".parse::<AttributeName>()?, &attribute)
        .send()
        .await?
        .done()?;
    let forwarded_request = SourceRequest::any_matching("/ngsi-ld/v1/entit.*");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    // the short name speed under the default vocabulary: another IRI, replaced locally
    let attribute_2 = Attribute::from(Property::builder(222).build()?);
    broker
        .entities()
        .replace_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?, &attribute_2)
        .send()
        .await?
        .done()?;
    let forwarded_request_2 = SourceRequest::any_matching("/ngsi-ld/v1/entit.*");
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(1)));
    // the short name speed under a context that maps it to another IRI: replaced locally
    let linked_2 = broker.with_context(Context::Single(ctx_other.entry()?))?;
    let attribute_3 = Attribute::from(Property::builder(333).build()?);
    linked_2
        .entities()
        .replace_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?, &attribute_3)
        .send()
        .await?
        .done()?;
    let forwarded_request_3 = SourceRequest::any_matching("/ngsi-ld/v1/entit.*");
    let forwarded_3 = source.received_matching(&forwarded_request_3).await?;
    expect_that!(forwarded_3, len(eq(1)));
    // the registered IRI under another term, in application/ld+json with the context in the body: forwarded
    let body_context_2 = broker
        .with_context(Context::Single(ctx_alias.entry()?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let attribute_4 = Attribute::from(Property::builder(444).build()?);
    body_context_2
        .entities()
        .replace_attribute(&vehicle_id, &"velocity".parse::<AttributeName>()?, &attribute_4)
        .send()
        .await?
        .done()?;
    let forwarded_request_4 = SourceRequest::any_matching("/ngsi-ld/v1/entit.*");
    let forwarded_4 = source.received_matching(&forwarded_request_4).await?;
    expect_that!(forwarded_4, len(eq(2)));
    // the registered short name under the registration's own context: forwarded
    let linked_3 = broker.with_context(Context::Single(ctx.entry()?))?;
    let attribute_5 = Attribute::from(Property::builder(555).build()?);
    linked_3
        .entities()
        .replace_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?, &attribute_5)
        .send()
        .await?
        .done()?;
    let forwarded_request_5 = SourceRequest::any_matching("/ngsi-ld/v1/entit.*");
    let forwarded_5 = source.received_matching(&forwarded_request_5).await?;
    expect_that!(forwarded_5, len(eq(3)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute(&format!("https://test.andromeda/{namespace}/speed")));
    expect_that!(entity, has_attribute("speed", any_attribute().value(222)));
    expect_that!(
        entity,
        has_attribute(&format!("https://other.andromeda/{namespace}/speed"), any_attribute().value(333))
    );
    Ok(())
}
