//! Partial attribute update is matched to an exclusive registration by expanded name.

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
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::{Accept, Link},
    },
    model::{Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Partial Attribute Update is matched against an exclusive registration's `propertyNames` by the expanded name
/// of the attribute in the path: a name that expands to the registered IRI is forwarded under whatever short form or
/// @context the request writes it, and a name of the same short form that expands to another IRI is updated locally and
/// not forwarded
/// Clause: 5.12, 4.3.6.3, 5.5.7, 5.6.4.4
/// Tenant: private (it counts every request its context source receives, and the discovery and unscoped requests of
/// other tests reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_fed_wrt_5003_partial_attribute_update_is_matched_to_an_exclusive_registration_by_expanded_name() -> Result<()> {
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
    // A typeless attribute fragment holding only a value is not the form the model writes, so the bodies are sent as written.
    let linked = |url: &str| -> Result<Link> { Ok(Link::try_json_ld_context(url.parse::<IriRefBuf>()?)?) };
    // the registered IRI under another term: forwarded
    let request = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_id}/attrs/velocity"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .typed_header(linked(ctx_alias.url.as_str())?)
        .body(Bytes::from_static(br#"{"value": 111}"#));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(1)));
    // the short name speed under the default vocabulary: another IRI, updated locally
    let request_2 = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from_static(br#"{"value": 222}"#));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(1)));
    // the short name speed under a context that maps it to another IRI: updated locally
    let request_3 = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .typed_header(linked(ctx_other.url.as_str())?)
        .body(Bytes::from_static(br#"{"value": 333}"#));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, answered(StatusCode::NO_CONTENT));
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(1)));
    // the registered IRI under another term, in application/ld+json with the context in the body: forwarded
    let request_4 = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_id}/attrs/velocity"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(format!(r#"{{"value": 444, "@context": "{}"}}"#, ctx_alias.url)));
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, answered(StatusCode::NO_CONTENT));
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(2)));
    // the registered short name under the registration's own context: forwarded
    let request_5 = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .typed_header(linked(ctx.url.as_str())?)
        .body(Bytes::from_static(br#"{"value": 555}"#));
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, answered(StatusCode::NO_CONTENT));
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(3)));
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
