//! A relative context reference is forwarded as the core context with fully qualified names.

use andromeda_test_suite::{
    body_json::json_equals,
    broker,
    config::SuiteConfig,
    context_source::ContextSource,
    forwarded_head,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
    relative_context::with_context_text,
    source_answer,
    wire::header_values,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::LINK};
use ngsi_ld::{
    http::{ContextDelivery, operation::CreateEntity},
    model::{Context, ContextEntry, Entity, EntityTypeName, InlineContext},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};
use serde_json::json;
use std::str;

/// Pins: Create Entity as application/ld+json naming a context hosted by the broker by its relative path, forwarded
/// to an application/ld+json source, carries the core @context inline with the type and attribute fully qualified
/// and no relative reference
/// Clause: 4.3.6.6, 6.3.5
/// Recorded: clause 6.3.5 does not say how a relative context reference reaches a Context Source, which would
/// resolve it against its own URL; Andromeda forwards the core context with fully qualified names
#[gtest]
#[tokio::test]
async fn o_fed_wrt_6603_a_relative_context_reference_is_forwarded_as_the_core_context_with_fully_qualified_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let core_context = SuiteConfig::get()?.core_context().as_str().to_owned();
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"spd": "https://test.andromeda/{namespace}/vocab/speed", "Veh_{namespace}": "https://test.andromeda/{namespace}/vocab/Vehicle"}}"#)
            .parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    source
        .answer(
            Method::POST,
            "/ngsi-ld/v1/entities",
            source_answer::text(StatusCode::CREATED, HeaderValue::from_static("application/json"), ""),
        )
        .await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_write4_id = namespace.urn("Vehicle", "write4")?;
    let veh_type = namespace.entity_type("Veh")?;
    let vehicle_type_iri = format!("https://test.andromeda/{namespace}/vocab/Vehicle");
    let speed_iri = format!("https://test.andromeda/{namespace}/vocab/speed");
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type_iri.parse::<EntityTypeName>()?).with_id(vehicle_write4_id.clone())])
            .property_names([speed_iri.as_str()])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntity])
        .context_source_info(("contentType", "application/ld+json"))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let body_context = broker.with_context_delivery(ContextDelivery::LdJsonBody);
    let vehicle = Entity::builder()
        .id(vehicle_write4_id.clone())
        .entity_type(veh_type.clone())
        .property("spd", 50)
        .build()?;
    // The client writes no relative reference into a body, so the @context of the body is replaced by the Location.
    let request_2 = with_context_text(body_context.unchecked(&CreateEntity::builder().entity(&vehicle).build())?, &json!(ctx.location))?;
    let created = body_context.send_unchecked(request_2).await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let forwarded = source.received_on(Method::POST, "/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    // The forwarded entity is read as the source received it: the `@context` it carries and the expanded names are
    // the bytes under test.
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(header_values(forwarded_head::head(forwarded_one).headers(), &LINK), is_empty());
    expect_that!(forwarded_one.body.as_slice(), json_equals("$['@context']", core_context.as_str()));
    expect_that!(forwarded_one.body.as_slice(), json_equals("$.type", vehicle_type_iri.as_str()));
    expect_that!(forwarded_one.body.as_slice(), json_equals(&format!("$['{speed_iri}'].value"), 50));
    expect_that!(str::from_utf8(&forwarded_one.body)?, not(contains_substring(ctx.location.as_str())));
    Ok(())
}
