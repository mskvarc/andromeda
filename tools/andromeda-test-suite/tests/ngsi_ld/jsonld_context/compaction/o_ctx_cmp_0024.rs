//! Every resource the versionless URL alone is answered as the current core context.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    core_context::{core_context, core_entry},
    namespace::Namespace,
    outcome::Done,
    received_status::ExpectedStatus,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::Accept,
        operation::{EntityMapRequest, QueryMethod},
    },
    model::{Context, ContextEntry, Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Retrieve Entity, Retrieve Temporal Evolution of an Entity, Retrieve Subscription, Retrieve Context Source
/// Registration Subscription, Retrieve Context Source Registration, Retrieve Available Entity Types, Retrieve
/// Available Attributes, Retrieve Entity Map and Query Entities as application/ld+json under a Link naming the
/// versionless core URL alone each name the core @context as one string in the body
/// Clause: 5.2.3, 4.4, 6.3.6, Annex C.8
/// Tenant: private (the case renders the tenant-wide entity type and attribute lists, and clause 4.14 keeps them
/// the tenant's own)
/// Recorded: clause 4.4 names one core @context; Andromeda reads every embedded older release and the versionless
/// URL as it and names only v1.9
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0024_every_resource_the_versionless_url_alone_is_answered_as_the_current_core_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let tenant_linked = tenant.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let tenant_core_context_body = tenant.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let registered_thing_type = namespace.entity_type("RegisteredThing")?;
    let registration_id = namespace.registration_id(1)?;
    let tenant_body_context = tenant
        .with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let tenant_linked_2 = tenant.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld".to_owned(),
    )?)))?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    contexts
        .serve(
            "jc/user/terms.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Device_{namespace}": "https://test.andromeda/{namespace}/Device",
        "speed": "https://test.andromeda/{namespace}/speed"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 3)
        .build()?;
    tenant_linked.entities().create(&device).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant_core_context_body.subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::registration_builder()
        .id(subscription_2_id.clone())
        .entities([registered_thing_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant_core_context_body.registration_subscriptions().create(&subscription_2).send().await?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([registered_thing_type.as_str()]).build()?])
        .build()?;
    tenant_core_context_body.registrations().register(&registration).send().await?;
    let _registration_cleanup = Cleanup::of(&tenant_core_context_body).registration(&registration_id);
    let entity_map_request = EntityMapRequest::returned();
    let page = tenant_body_context
        .with_heads()
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&device_type)
        .ids(vec![device_id.clone()])
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let retrieved = tenant_linked_2
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    let retrieved_2 = tenant_linked_2
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .retrieve(&device_id)
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(evolution.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    let subscription_3 = tenant_linked_2
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_3.context(), some(eq(&Context::Single(core_entry()?))));
    let subscription_4 = tenant_linked_2
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .registration_subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await?;
    expect_that!(subscription_4.context(), some(eq(&Context::Single(core_entry()?))));
    let registration_2 = tenant_linked_2
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(registration_2.context(), some(eq(&Context::Single(core_entry()?))));
    let types = tenant_linked_2
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entity_types()
        .list()
        .send()
        .await?;
    expect_that!(types.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    let attributes = tenant_linked_2
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .attributes()
        .list()
        .send()
        .await?;
    expect_that!(attributes.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    let map = page.entity_map().valid().or_fail()?;
    let map_id = map.id().or_fail()?;
    let map_2 = tenant_linked_2
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entity_maps()
        .retrieve(&map_id)
        .send()
        .await?;
    expect_that!(map_2.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    let page_2 = tenant_linked_2
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/Device").as_str())
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    let item = page_2.items().first().or_fail()?;
    expect_that!(item.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    expect_that!(page_2.items(), len(le(1)));
    Ok(())
}
