//! Replace entity is matched to a registration by expanded name.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    hosted_context::HostedContext,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::EntityIdMember},
    model::{Context, ContextEntry, Entity, InlineContext, Input, NgsiLdMediaType},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Replace Entity is matched against an inclusive registration's (an exclusive one must name each entity by
/// id, so the pattern needs the inclusive mode) `propertyNames` by the expanded name of each attribute: an
/// attribute that expands to the registered IRI is forwarded under whatever short form or @context the request
/// writes it, and an attribute of the same short name that expands to another IRI replaces the entity locally and
/// is not forwarded
/// Clause: 5.12, 4.3.6.3, 5.5.7, 5.6.18.4
/// Tenant: private (it counts every request its context source receives, and the discovery and unscoped requests of
/// other tests reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_fed_wrt_5204_replace_entity_is_matched_to_a_registration_by_expanded_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
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
    let entities = (1..=5)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", n)?)
                .entity_type(vehicle_type.clone())
                .property("brandName", "Mercedes")
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    source
        .answer_when(SourceRequest::any_matching("/.*").priority(1), source_answer::status(StatusCode::NO_CONTENT))
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
        .operations([OperationGroup::RedirectionOps])
        .build()?;
    body_context.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&body_context).registration(&registration_id);
    // the registered IRI under another term: forwarded
    let linked = broker.with_context(Context::Single(ctx_alias.entry()?))?;
    let replacement = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("velocity", 111)
        .build()?;
    let answer = linked
        .with_responses()
        .entities()
        .replace(&replacement)
        .id_member(EntityIdMember::Omitted)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let forwarded_request = SourceRequest::any_matching("/ngsi-ld/v1/entit.*");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    // the short name speed under the default vocabulary: another IRI, replaced locally
    let replacement_2 = Entity::builder()
        .id(namespace.urn("Vehicle", "2")?)
        .entity_type(vehicle_type.clone())
        .property("speed", 222)
        .build()?;
    let answer_2 = broker
        .with_responses()
        .entities()
        .replace(&replacement_2)
        .id_member(EntityIdMember::Omitted)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    let forwarded_request_2 = SourceRequest::any_matching("/ngsi-ld/v1/entit.*");
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(1)));
    // the short name speed under a context that maps it to another IRI: replaced locally
    let linked_2 = broker.with_context(Context::Single(ctx_other.entry()?))?;
    let replacement_3 = Entity::builder()
        .id(namespace.urn("Vehicle", "3")?)
        .entity_type(vehicle_type.clone())
        .property("speed", 333)
        .build()?;
    let answer_3 = linked_2
        .with_responses()
        .entities()
        .replace(&replacement_3)
        .id_member(EntityIdMember::Omitted)
        .send()
        .await;
    expect_that!(&answer_3, answered(StatusCode::NO_CONTENT));
    let forwarded_request_3 = SourceRequest::any_matching("/ngsi-ld/v1/entit.*");
    let forwarded_3 = source.received_matching(&forwarded_request_3).await?;
    expect_that!(forwarded_3, len(eq(1)));
    // the registered IRI under another term, in application/ld+json with the context in the body: forwarded
    let body_context_2 = broker
        .with_context(Context::Single(ctx_alias.entry()?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let replacement_4 = Entity::builder()
        .id(namespace.urn("Vehicle", "4")?)
        .entity_type(vehicle_type.clone())
        .property("velocity", 444)
        .build()?;
    let answer_4 = body_context_2
        .with_responses()
        .entities()
        .replace(&replacement_4)
        .id_member(EntityIdMember::Omitted)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NO_CONTENT));
    let forwarded_request_4 = SourceRequest::any_matching("/ngsi-ld/v1/entit.*");
    let forwarded_4 = source.received_matching(&forwarded_request_4).await?;
    expect_that!(forwarded_4, len(eq(2)));
    // the registered IRI written in full: forwarded
    let replacement_5 = Entity::builder()
        .id(namespace.urn("Vehicle", "5")?)
        .entity_type(vehicle_type.clone())
        .property(format!("https://test.andromeda/{namespace}/speed").as_str(), 555)
        .build()?;
    let answer_5 = broker
        .with_responses()
        .entities()
        .replace(&replacement_5)
        .id_member(EntityIdMember::Omitted)
        .send()
        .await;
    expect_that!(&answer_5, answered(StatusCode::NO_CONTENT));
    let forwarded_request_5 = SourceRequest::any_matching("/ngsi-ld/v1/entit.*");
    let forwarded_5 = source.received_matching(&forwarded_request_5).await?;
    expect_that!(forwarded_5, len(eq(3)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&vehicle_2_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(222)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&vehicle_3_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(
        entity_2,
        has_attribute(&format!("https://other.andromeda/{namespace}/speed"), any_attribute().value(333))
    );
    Ok(())
}
