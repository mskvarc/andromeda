//! A member in both pick and omit is refused at every entry point.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, outcome::Done, problem::problem, raw_call::target, unchecked_call::exchange};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{CreateSubscription, QueryMethod},
    },
    model::{Entity, ErrorType, Input, NgsiLdMediaType, Property},
    query::projection::Projection,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: pick and omit naming one member answer 400 `BadRequestData` on Retrieve Entity, Query Entities, both
/// temporal reads, a POST query, Create Subscription, Update Subscription and Create Context Source Registration
/// Subscription
/// Clause: 4.21, 5.7.1.4, 5.7.2.4, 5.7.3.4, 5.7.4.4, 5.8.1.4, 5.8.2.4, 5.11.2.3
/// Recorded: no clause forbids combining pick and omit; a member named in both is refused as a self-contradictory
/// projection with `BadRequestData`, ruled by the user (2026-10-11)
#[gtest]
#[tokio::test]
async fn o_qry_prj_9009_a_member_in_both_pick_and_omit_is_refused_at_every_entry_point() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let vehicle_id = namespace.urn("Vehicle", "selectors")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let refusal = || problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData);
    let speed = || "speed".parse::<Projection>();
    let json = broker.with_responses().with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json.entities().retrieve_projected(&vehicle_id).pick(speed()?).omit(speed()?).send().await;
    expect_that!(&retrieved, refusal());
    let queried = json
        .entities()
        .query_projected()
        .entity_type(&vehicle_type)
        .pick(speed()?)
        .omit(speed()?)
        .send()
        .await;
    expect_that!(&queried, refusal());
    let posted = json
        .entities()
        .query_projected()
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .pick(speed()?)
        .omit(speed()?)
        .send()
        .await;
    expect_that!(&posted, refusal());
    // The temporal reads take no projection from the client, so they go out as written.
    let type_text = vehicle_type.to_string();
    let temporal = [("timerel", "after"), ("timeAt", "2000-01-01T00:00:00Z"), ("pick", "speed"), ("omit", "speed")];
    let by_id = target(&format!("temporal/entities/{vehicle_id}"), &temporal);
    let mut by_type_parameters = vec![("type", type_text.as_str())];
    by_type_parameters.extend(temporal);
    let by_type = target("temporal/entities", &by_type_parameters);
    for read in [by_id, by_type] {
        let answer = broker
            .send_unchecked(exchange(&broker, Method::GET, &read, &[("Accept", "application/json")], None)?)
            .await;
        expect_that!(&answer, refusal());
    }
    let notification = || -> Result<NotificationParams<Input>> {
        Ok(NotificationParams::builder(
            Endpoint::builder(format!("{}notify", config.unreachable_origin()))
                .accept(NgsiLdMediaType::Json)
                .build()?,
        )
        .pick("id".parse::<Projection>()?)
        .omit("id".parse::<Projection>()?)
        .build()?)
    };
    let subscription = Subscription::builder()
        .id(namespace.subscription_id("same-member")?)
        .entities([vehicle_type.as_str()])
        .notification(notification()?)
        .build()?;
    let created = broker
        .send_unchecked(broker.unchecked(&CreateSubscription { subscription: &subscription })?)
        .await;
    expect_that!(&created, refusal());
    let update_id = namespace.subscription_id("same-member-update")?;
    let to_update = Subscription::builder()
        .id(update_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(format!("{}notify", config.unreachable_origin()))
                    .accept(NgsiLdMediaType::Json)
                    .build()?,
            )
            .pick("id".parse::<Projection>()?)
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&to_update).send().await?;
    // The Update Subscription fragment has no reader for a notification with both members, so it goes out as written.
    let patch = format!(
        r#"{{"notification": {{"endpoint": {{"uri": "{}notify", "accept": "application/json"}}, "pick": ["id"], "omit": ["id"]}}}}"#,
        config.unreachable_origin()
    );
    let patched = broker
        .send_unchecked(exchange(
            &broker,
            Method::PATCH,
            &format!("subscriptions/{update_id}"),
            &[("Content-Type", "application/json")],
            Some(patch.as_str()),
        )?)
        .await;
    expect_that!(&patched, refusal());
    broker.subscriptions().delete(&update_id).send().await?;
    // A Context Source Registration Subscription is created by a document of its own kind.
    let registration_subscription = format!(
        r#"{{"id": "{}", "type": "Subscription", "entities": [{{"type": "{vehicle_type}"}}], "notification": {{"endpoint": {{"uri": "{}notify", "accept": "application/json"}}, "pick": ["id"], "omit": ["id"]}}}}"#,
        namespace.subscription_id("same-member-csource")?,
        config.unreachable_origin()
    );
    let csource = broker
        .send_unchecked(exchange(
            &broker,
            Method::POST,
            "csourceSubscriptions",
            &[("Content-Type", "application/json")],
            Some(registration_subscription.as_str()),
        )?)
        .await;
    expect_that!(&csource, refusal());
    Ok(())
}
