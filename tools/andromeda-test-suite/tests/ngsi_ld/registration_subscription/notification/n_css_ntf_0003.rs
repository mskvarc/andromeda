//! A csf path reads a compound context source property by its written key.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, notification_sink::NotificationSink, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{NgsiLdMediaType, Output, RawJson},
    query::q,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, TriggerReason},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Context Source Registration Subscription whose csf is coverage`[region]`=="north" reads back with that
/// csf, is not notified of a registration whose coverage region is south, and is notified `newlyMatching` of one
/// whose region is north
/// Clause: 4.9, 5.11.2.4, 5.2.12
#[gtest]
#[tokio::test]
async fn n_css_ntf_0003_a_csf_path_reads_a_compound_context_source_property_by_its_written_key() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let registration_south_id = namespace.registration_id("south")?;
    let registration_north_id = namespace.registration_id("north")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .csf(q::attr("coverage").member("region").eq("north").build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(stored.csf().map(ToString::to_string), some(eq(r#"coverage[region]=="north""#)));
    let registration_south = CSourceRegistration::builder(source.endpoint("south")?)
        .id(registration_south_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .build()?;
    // The model refuses a registration property written as a Property, so `coverage` is written as text.
    let coverage_south: RawJson = r#"{"type": "Property", "value": {"region": "south"}}"#.parse()?;
    let document_south = UncheckedDocument::try_from(registration_south.render())?.set_member("coverage", &coverage_south);
    let request_south = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_south,
        })?
        .document(&document_south);
    let answer_south = broker.send_unchecked(request_south).await;
    expect_that!(&answer_south, answered(StatusCode::CREATED));
    let registration_north = CSourceRegistration::builder(source.endpoint("north")?)
        .id(registration_north_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .build()?;
    // The model refuses a registration property written as a Property, so `coverage` is written as text.
    let coverage_north: RawJson = r#"{"type": "Property", "value": {"region": "north"}}"#.parse()?;
    let document_north = UncheckedDocument::try_from(registration_north.render())?.set_member("coverage", &coverage_north);
    let request_north = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_north,
        })?
        .document(&document_north);
    let answer_north = broker.send_unchecked(request_north).await;
    expect_that!(&answer_north, answered(StatusCode::CREATED));
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let received = notifications.first().or_fail()?;
    // The reader requires `type` to be `ContextSourceNotification`, so the read pins it.
    let notification = received.registrations()?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification.data(), len(eq(1)));
    expect_that!(
        notification.data().first().map(CSourceRegistration::<Output>::id),
        some(eq(&registration_north_id))
    );
    Ok(())
}
