//! A geojson notification names the context on the collection only.

use andromeda_test_suite::{
    broker,
    context_terms::listed_entries,
    core_context::{core_context, core_entry},
    header_condition::HeaderCondition,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Entity, EntityTypeName, FeatureCollection, Lossless, NgsiLdMediaType, Output},
    subscription::{Endpoint, Notification, NotificationParams, NotificationTrigger, Subscription},
};
use std::str;

/// Pins: An application/geo+json notification names its @context on the `FeatureCollection` only, not on the
/// Notification nor on any Feature, and with receiverInfo Prefer body=json names none in the body and links the
/// jsonldContext once
/// Clause: Table 5.3.1-1, 4.5.16.3, 5.8.1.4, 6.3.8
/// Recorded: clause 5.2.3 has an application/ld+json body name the user @context and the core @context; Andromeda
/// names jsonldContext followed by the v1.9 core unless jsonldContext leaves the core in force last (Annex C.8)
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0046_a_geojson_notification_names_the_context_on_the_collection_only() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let contexts = RemoteContexts::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let device_iri = format!("https://test.andromeda/{namespace}/Device");
    let speed_iri = format!("https://test.andromeda/{namespace}/speed");
    let notified_id = namespace.urn("Device", "notified")?;
    let terms = contexts.url("jc/user/terms.jsonld")?;
    let body_core = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    contexts
        .serve(
            "jc/user/terms.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Device_{namespace}": "{device_iri}",
        "speed": "{speed_iri}"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([device_iri.clone()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::GeoJson).build()?).build()?)
        .jsonld_context(terms.clone())
        .build()?;
    body_core.subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([device_iri.clone()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint(2)?)
                    .accept(NgsiLdMediaType::GeoJson)
                    .receiver_info("Prefer", "body=json")
                    .build()?,
            )
            .build()?,
        )
        .jsonld_context(terms.clone())
        .build()?;
    body_core.subscriptions().create(&subscription_2).send().await?;
    let notified = Entity::builder()
        .id(notified_id.clone())
        .entity_type(device_iri.parse::<EntityTypeName>()?)
        .property(speed_iri.as_str(), 3)
        .build()?;
    broker.entities().create(&notified).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let received = sink.received(1).await?;
    expect_that!(received, len(eq(1)));
    let notification = Notification::<FeatureCollection<Entity<Output>>>::parse(str::from_utf8(&received.first().or_fail()?.body)?, Lossless::Normalized)?;
    let collection = notification.data();
    let feature = collection.features.first().or_fail()?;
    expect_that!(feature.entity.id, eq(&notified_id));
    expect_that!(feature.context.as_ref(), none());
    expect_that!(notification.context(), none());
    expect_that!(
        collection.context.as_ref().and_then(listed_entries),
        some(elements_are![eq(&contexts.reference("jc/user/terms.jsonld")?), eq(&core_entry()?)])
    );
    sink.wait_for_at_least(2, 1).await?;
    let link_once = HeaderCondition::exactly(LINK, &format!("<{}>;[^,]*json-ld#context[^,]*", regex::escape(terms.as_str())))?;
    let received_2 = sink.received_matching(2, &[], &[link_once]).await?;
    expect_that!(received_2, len(eq(1)));
    let notification_2 = Notification::<FeatureCollection<Entity<Output>>>::parse(str::from_utf8(&received_2.first().or_fail()?.body)?, Lossless::Normalized)?;
    let collection_2 = notification_2.data();
    expect_that!(notification_2.context(), none());
    expect_that!(collection_2.context.as_ref(), none());
    expect_that!(collection_2.features.first().or_fail()?.context.as_ref(), none());
    Ok(())
}
