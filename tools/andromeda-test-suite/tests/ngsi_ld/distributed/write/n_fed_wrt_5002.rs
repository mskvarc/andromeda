//! Update attributes is matched to an exclusive registration by expanded name.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    core_context::core_entry,
    entity_matcher::{has_attribute, lacks_attribute},
    hosted_context::HostedContext,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
    wire::send_to,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::UpdateAttributes},
    model::{Context, ContextEntry, Entity, EntityFragment, InlineContext, NgsiLdMediaType},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Update Attributes is matched against an exclusive registration's `propertyNames` by the expanded name of each
/// attribute: an attribute that expands to the registered IRI is forwarded under whatever short form or @context the
/// request writes it, and an attribute of the same short name that expands to another IRI is updated locally and not
/// forwarded
/// Clause: 5.12, 4.3.6.3, 5.5.7, 5.6.2.4
/// Tenant: private (it counts every request its context source receives, and the discovery and unscoped requests of
/// other tests reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_fed_wrt_5002_update_attributes_is_matched_to_an_exclusive_registration_by_expanded_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let ctx = HostedContext::add(
        &broker,
        &Context::Multiple(vec![
            ContextEntry::Inline(format!(r#"{{"speed": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
            core_entry()?,
        ]),
    )
    .await?;
    let ctx_other = HostedContext::add(
        &broker,
        &Context::Multiple(vec![
            ContextEntry::Inline(format!(r#"{{"speed": "https://other.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
            core_entry()?,
        ]),
    )
    .await?;
    let ctx_alias = HostedContext::add(
        &broker,
        &Context::Multiple(vec![
            ContextEntry::Inline(format!(r#"{{"velocity": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
            core_entry()?,
        ]),
    )
    .await?;
    let in_body = broker
        .with_context(Context::Single(ctx.entry()?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    // the entity holds two attributes that a short name speed can stand for, under the default vocabulary and under another IRI, and not the one the registration will cover
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("https://uri.etsi.org/ngsi-ld/default-context/speed", 2)
        .property(format!("https://other.andromeda/{namespace}/speed").as_str(), 3)
        .build()?;
    in_body.entities().create(&vehicle).send().await?.done()?;
    source
        .answer_when(SourceRequest::any_matching("/.*").priority(1), source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .operations([OperationGroup::RedirectionOps])
        .mode(RegistrationMode::Exclusive)
        .build()?;
    in_body.registrations().register(&registration).send().await?;
    let forwarded_request = SourceRequest::any_matching("/ngsi-ld/v1/entit.*");
    // the registered IRI under another term: forwarded
    let fragment = EntityFragment::builder().property("velocity", 111).build()?;
    let linked_alias = broker.with_context(Context::Single(ctx_alias.entry()?))?;
    let answer = send_to(
        &linked_alias,
        &UpdateAttributes::builder().id(&vehicle_id).fragment(&fragment).build(),
        &format!("entities/{vehicle_id}/attrs/"),
    )
    .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(1)));
    // the short name speed under the default vocabulary: another IRI, updated locally
    let fragment_2 = EntityFragment::builder().property("speed", 222).build()?;
    let answer_2 = send_to(
        &broker,
        &UpdateAttributes::builder().id(&vehicle_id).fragment(&fragment_2).build(),
        &format!("entities/{vehicle_id}/attrs/"),
    )
    .await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(1)));
    // the short name speed under a context that maps it to another IRI: updated locally
    let fragment_3 = EntityFragment::builder().property("speed", 333).build()?;
    let linked_other = broker.with_context(Context::Single(ctx_other.entry()?))?;
    let answer_3 = send_to(
        &linked_other,
        &UpdateAttributes::builder().id(&vehicle_id).fragment(&fragment_3).build(),
        &format!("entities/{vehicle_id}/attrs/"),
    )
    .await;
    expect_that!(&answer_3, answered(StatusCode::NO_CONTENT));
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(1)));
    // the registered IRI under another term, in application/ld+json with the context in the body: forwarded
    let fragment_4 = EntityFragment::builder().property("velocity", 444).build()?;
    let alias_in_body = broker
        .with_context(Context::Single(ctx_alias.entry()?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let answer_4 = send_to(
        &alias_in_body,
        &UpdateAttributes::builder().id(&vehicle_id).fragment(&fragment_4).build(),
        &format!("entities/{vehicle_id}/attrs/"),
    )
    .await;
    expect_that!(&answer_4, answered(StatusCode::NO_CONTENT));
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(2)));
    // the registered IRI written in full: forwarded
    let fragment_5 = EntityFragment::builder()
        .property(format!("https://test.andromeda/{namespace}/speed").as_str(), 555)
        .build()?;
    let answer_5 = send_to(
        &broker,
        &UpdateAttributes::builder().id(&vehicle_id).fragment(&fragment_5).build(),
        &format!("entities/{vehicle_id}/attrs/"),
    )
    .await;
    expect_that!(&answer_5, answered(StatusCode::NO_CONTENT));
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(3)));
    // the local entity: the registered IRI never written here, the two other IRIs written by the requests that did not match
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
