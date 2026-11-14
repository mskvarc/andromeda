//! A projection leaves Query Entities and a notification carrying the same members.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Completeness, Entity, Lossless, NgsiLdMediaType, Output, Projected, Representation},
    query::{projection::Projection, type_selection},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::collections::BTreeSet;

/// The projection a case asks.
#[derive(Clone, Copy)]
enum Asked {
    /// `pick` with this projection.
    Pick(&'static str),
    /// `omit` with this projection.
    Omit(&'static str),
}

impl Asked {
    /// The `pick` and the `omit` member of the request.
    fn members(self) -> Result<(Option<Projection>, Option<Projection>)> {
        Ok(match self {
            Asked::Pick(projection) => (Some(projection.parse()?), None),
            Asked::Omit(projection) => (None, Some(projection.parse()?)),
        })
    }
}

/// The names of the Attributes `entity` carries.
fn attribute_names<C: Completeness>(entity: &Entity<Output, C>) -> BTreeSet<String> {
    entity.attributes.iter().map(|(name, _)| name.as_str().to_owned()).collect()
}

/// The members each projected Entity of `answer` carries, `id` and `type` among them when present, sorted.
fn projected_members(answer: &[Entity<Output, Projected>]) -> Vec<Vec<String>> {
    let mut carried: Vec<Vec<String>> = answer
        .iter()
        .map(|entity| {
            let mut members = attribute_names(entity);
            members.extend(entity.id.as_ref().map(|_| "id".to_owned()));
            members.extend(entity.entity_type.as_ref().map(|_| "type".to_owned()));
            members.into_iter().collect()
        })
        .collect();
    carried.sort();
    carried
}

/// `expected` as sorted member lists.
fn sorted(expected: &[&[&str]]) -> Vec<Vec<String>> {
    let mut sorted: Vec<Vec<String>> = expected
        .iter()
        .map(|members| {
            let mut members: Vec<String> = members.iter().map(|member| (*member).to_owned()).collect();
            members.sort();
            members
        })
        .collect();
    sorted.sort();
    sorted
}

/// Creates a Building `x` with `n` 10 and `s` "abc" and a Building `y` with `s` "abd" alone, and expects Query
/// Entities with `asked` and the notifications of a Subscription asking it alike to carry exactly the members
/// `expected` lists, one list per Entity.
async fn expect_projection_parity(asked: Asked, expected: &[&[&str]]) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let endpoint = Endpoint::builder(sink.endpoint(0)?).accept(NgsiLdMediaType::Json).build()?;
    let (pick, omit) = asked.members()?;
    let params = NotificationParams::builder(endpoint)
        .format(Representation::Normalized)
        .maybe_pick(pick)
        .maybe_omit(omit)
        .build()?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id("projection")?)
        .entities([building_type.as_str()])
        .notification(params)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let x = Entity::builder()
        .id(namespace.urn("Building", "x")?)
        .entity_type(building_type.clone())
        .property("n", 10)
        .property("s", "abc")
        .build()?;
    let y = Entity::builder()
        .id(namespace.urn("Building", "y")?)
        .entity_type(building_type.clone())
        .property("s", "abd")
        .build()?;
    broker.entities().create(&x).send().await?.done()?;
    broker.entities().create(&y).send().await?.done()?;
    let (pick, omit) = asked.members()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .maybe_pick(pick)
        .maybe_omit(omit)
        .send()
        .await?;
    expect_that!(projected_members(page.items()), eq(&sorted(expected)), "Query Entities");
    sink.wait_for_at_least(0, expected.len()).await?;
    let mut notified = Vec::new();
    for notification in sink.received(0).await? {
        notified.extend(notification.projected_entities(Lossless::Normalized)?.data().iter().cloned());
    }
    expect_that!(projected_members(&notified), eq(&sorted(expected)), "the notifications");
    Ok(())
}

/// Pins: over a Building with `n` and `s` and one with `s` alone, Query Entities and a notification with pick
/// `id,n` carry `id` and `n` of the first and `id` alone of the second; every Entity is reduced to the members the
/// projection lists
/// Clause: 4.21, 5.7.2.5, 5.8.6
#[gtest]
#[tokio::test]
async fn n_qry_prj_9013_a_pick_reduces_each_entity_to_the_members_it_lists_alike() -> Result<()> {
    expect_projection_parity(Asked::Pick("id,n"), &[&["id", "n"], &["id"]]).await
}

/// Pins: over the same Buildings, omit `n` carries `id`, `type` and `s` of each, in a query and a notification alike
/// Clause: 4.21, 5.7.2.5, 5.8.6
#[gtest]
#[tokio::test]
async fn n_qry_prj_9014_an_omit_removes_the_members_it_lists_alike() -> Result<()> {
    expect_projection_parity(Asked::Omit("n"), &[&["id", "type", "s"], &["id", "type", "s"]]).await
}

/// Pins: over the same Buildings, pick `type` carries `type` alone of each, in a query and a notification alike; every
/// Entity has a type, so none is emptied
/// Clause: 4.21, 5.7.2.5, 5.8.6
#[gtest]
#[tokio::test]
async fn n_qry_prj_9015_a_pick_of_the_type_keeps_every_entity_with_its_type_alone() -> Result<()> {
    expect_projection_parity(Asked::Pick("type"), &[&["type"], &["type"]]).await
}

/// Pins: over the same Buildings, Query Entities with attrs `n` answers the Building holding `n` with `id`, `type` and
/// `n`, and not the Building without it; attrs is the pick of the listed Attributes with a q requiring one of them
/// Clause: 5.2.23, 6.4.3.2, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_prj_9016_attrs_answers_only_entities_holding_a_listed_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let x = Entity::builder()
        .id(namespace.urn("Building", "x")?)
        .entity_type(building_type.clone())
        .property("n", 10)
        .property("s", "abc")
        .build()?;
    let y = Entity::builder()
        .id(namespace.urn("Building", "y")?)
        .entity_type(building_type.clone())
        .property("s", "abd")
        .build()?;
    broker.entities().create(&x).send().await?.done()?;
    broker.entities().create(&y).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(type_selection::entity_type(&building_type).build()?)
        .attrs(vec!["n".parse::<AttributeName>()?])
        .send()
        .await?;
    let carried: Vec<Vec<String>> = page
        .items()
        .iter()
        .map(|entity| {
            let mut members = attribute_names(entity);
            members.extend(["id".to_owned(), "type".to_owned()]);
            members.into_iter().collect()
        })
        .collect();
    expect_that!(carried, eq(&sorted(&[&["id", "type", "n"]])));
    Ok(())
}
