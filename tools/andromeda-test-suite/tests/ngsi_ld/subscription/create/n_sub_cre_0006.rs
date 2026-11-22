//! A scope query whose group holds a scope ending in the multi-level wildcard reads back as written and notifies its
//! hierarchy.

use andromeda_test_suite::{
    body_json::json_equals,
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    problem::answered,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{Entity, Lossless, NgsiLdMediaType, RawJson, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::time::Duration;

/// How long the endpoint is watched for a notification beyond the one owed.
const QUIET_WINDOW: Duration = Duration::from_secs(3);

/// Pins: A Subscription created with the `scopeQ` `(/Madrid/#;/CompanyA)` reads back with that `scopeQ`, and of an
/// Entity created in `/Madrid/Gardens` alone, then one created in `/Madrid/Gardens` and `/CompanyA`, it is notified of
/// the second only
/// Clause: 4.19, 5.8.1.4, 5.8.3, 5.8.6
/// Recorded: clause 4.19 adds the `#` wildcard "at the end of a Scope specification", and a Scope inside a group is
/// one, though its grammar writes the group without the `/#`
#[gtest]
#[tokio::test]
async fn n_sub_cre_0006_a_scope_query_whose_group_holds_a_hierarchy_reads_back_as_written_and_notifies_its_hierarchy() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id("hierarchy-in-group")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("hierarchy")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    // The model refuses it (a subscription scopeQ), so `scopeQ` is written as text.
    let scope_q: RawJson = r#""(/Madrid/#;/CompanyA)""#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("scopeQ", &scope_q);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // The model refuses the scopeQ this Subscription holds, so the read goes out as written.
    let stored = broker
        .send_unchecked(exchange(
            &broker,
            Method::GET,
            &format!("subscriptions/{subscription_id}"),
            &[("Accept", "application/json")],
            None,
        )?)
        .await;
    expect_that!(&stored, answered(StatusCode::OK));
    expect_that!(stored?.body(), json_equals("$.scopeQ", "(/Madrid/#;/CompanyA)"));
    let outside = Entity::builder()
        .id(namespace.urn("Building", "outside")?)
        .entity_type(building_type.clone())
        .scope("/Madrid/Gardens")
        .build()?;
    broker.entities().create(&outside).send().await?.done()?;
    let inside_id = namespace.urn("Building", "inside")?;
    let inside = Entity::builder()
        .id(inside_id.clone())
        .entity_type(building_type.clone())
        .scope(["/Madrid/Gardens", "/CompanyA"])
        .build()?;
    broker.entities().create(&inside).send().await?.done()?;
    sink.wait_for_at_least("hierarchy", 1).await?;
    expect_that!(sink.count_after("hierarchy", QUIET_WINDOW).await?, eq(1));
    let received = sink.received("hierarchy").await?;
    let notification = received.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), elements_are![has_id(&inside_id)]);
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
