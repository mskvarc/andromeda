//! A relative reference names the requesting tenants entry.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_record::{segment_of_location, stored},
    entity_matcher::has_attribute,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
    relative_context::with_context_text,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::CreateEntity},
    model::{Context, ContextEntry, Entity, InlineContext, Lossless, NgsiLdMediaType},
};
use serde_json::json;

/// Pins: In a tenant of its own, Create Entity naming as its @context the root-relative path Add @context answered
/// there expands through that tenant's stored @context, and the use is counted against that entry
/// Clause: 5.13.2.5, 5.13.3.5, 4.14, 6.3.14
/// Tenant: private (the subject is which tenant's stored @context a relative reference names)
/// Recorded: the specification does not say how a relative @context reference resolves; Andromeda resolves its own
/// Location in the tenant the request names
#[gtest]
#[tokio::test]
async fn o_ctx_exp_9027_a_relative_reference_names_the_requesting_tenants_entry() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"A1": "urn:ngsi-ld:attributes:{namespace}-A1"}}"#).parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&tenant, &context).await?;
    let body_context = tenant.with_context_delivery(ContextDelivery::LdJsonBody);
    let testing_id = namespace.urn("Testing", "tenant-relative")?;
    let testing = Entity::builder()
        .id(testing_id.clone())
        .entity_type(namespace.entity_type("Testing")?)
        .property("A1", "a value")
        .build()?;
    // The client writes no relative reference into a body, so the body's @context is replaced by the Location.
    let request = with_context_text(
        body_context.unchecked(&CreateEntity::builder().entity(&testing).form(Lossless::Concise).build())?,
        &json!([ctx.location]),
    )?;
    let created = body_context.send_unchecked(request).await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let retrieved = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&testing_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute(&format!("urn:ngsi-ld:attributes:{namespace}-A1"), any_attribute().value("a value"))
    );
    let record = stored(&tenant, segment_of_location(&ctx.location)?).await?;
    verify_that!(record.number_of_hits, some(eq(1)))
}
