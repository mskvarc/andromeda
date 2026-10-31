//! Equivalence classes collating symbols and a bracket in a list are executed.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Locality, q},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: Query Entities with a q pattern holding an equivalence class, a collating symbol or an opening bracket
/// inside a bracket expression answers 200 with the Entity selected as IEEE 1003.2 reads the pattern, and Create
/// Subscription with such a q answers 201
/// Clause: 4.9; IEEE 1003.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9025_equivalence_classes_collating_symbols_and_a_bracket_in_a_list_are_executed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_posix_bracket_id = namespace.urn("Building", "posix-bracket")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_posix_bracket_id = namespace.subscription_id("posix-bracket")?;
    let building_posix_bracket = Entity::builder()
        .id(building_posix_bracket_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Mercedes")
        .build()?;
    broker.entities().create(&building_posix_bracket).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_bracket_id.clone()])
        .q(q::attr("name").matches("\"M[[=e=]]rc\""))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_posix_bracket_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_bracket_id.clone()])
        .q(q::attr("name").matches("\"^[[=M=]]\""))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_posix_bracket_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_bracket_id.clone()])
        .q(q::attr("name").matches("\"[[.e.]]des$\""))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&building_posix_bracket_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_bracket_id.clone()])
        .q(q::attr("name").matches("\"[[=a=]]\""))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(0)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_bracket_id.clone()])
        .q(q::attr("name").matches("\"^[[=a=]]$\""))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(0)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_bracket_id.clone()])
        .q(q::attr("name").matches("\"[[.a.]]\""))
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(0)));
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_bracket_id.clone()])
        .q(q::attr("name").matches("\"[[abc]]\""))
        .send()
        .await?;
    expect_that!(page_7.items(), len(eq(0)));
    let subscription_posix_bracket = Subscription::builder()
        .id(subscription_posix_bracket_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated, NotificationTrigger::AttributeUpdated])
        .q(q::attr("name").matches("\"^[[=a=]]$\"").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("posix-bracket")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_posix_bracket).send().await?;
    broker.subscriptions().delete(&subscription_posix_bracket_id).send().await?;
    Ok(())
}
