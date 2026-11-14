//! A datasetId narrows the instances Query Entities and a notification carry alike.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_instance, has_instances, lacks_attribute},
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DatasetId, Entity, EntityId, Lossless, NgsiLdMediaType, Output, Property},
    query::{q::Query, type_selection},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// What the answers carry of the Meter's `n`.
#[derive(Clone, Copy)]
enum Carried {
    /// The one instance holding this value.
    Instance(i64),
    /// No instance at all.
    Nothing,
}

/// The dataset a request names.
#[derive(Clone, Copy)]
enum Dataset {
    /// The default instances, `@none`.
    Default,
    /// The dataset of the Meter's second instance of `n`.
    Second,
    /// A dataset no instance has.
    Unused,
}

/// Creates a Meter whose `n` is 10 as its default instance and 60 in a dataset of its own, and expects Query Entities
/// with `q` and the `datasetId` of `dataset`, and the notification of a Subscription with the same members, to select
/// the Meter and to carry what `carried` says of `n`.
async fn expect_dataset_parity(q: Option<&str>, dataset: Dataset, carried: Carried) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let meter_type = namespace.entity_type("Meter")?;
    let meter_id = namespace.urn("Meter", 1)?;
    let second = namespace.dataset_id("Dataset", 1)?;
    let named = match dataset {
        Dataset::Default => "@none".parse::<DatasetId>()?,
        Dataset::Second => second.clone(),
        Dataset::Unused => namespace.dataset_id("Dataset", 9)?,
    };
    let query = q.map(str::parse::<Query>).transpose()?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id("dataset")?)
        .entities([meter_type.as_str()])
        .maybe_q(query.clone())
        .dataset_ids([named.clone()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(0)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let meter = Entity::builder()
        .id(meter_id.clone())
        .entity_type(meter_type.clone())
        .property("n", 10)
        .property("n", Property::builder(60).dataset_id(second))
        .build()?;
    broker.entities().create(&meter).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(type_selection::entity_type(&meter_type).build()?)
        .maybe_q(query)
        .dataset_ids(vec![named])
        .send()
        .await?;
    expect_carried(page.items(), &meter_id, carried, "Query Entities");
    sink.wait_for_at_least(0, 1).await?;
    let mut notified = Vec::new();
    for notification in sink.received(0).await? {
        notified.extend(notification.entities(Lossless::Normalized)?.data().iter().cloned());
    }
    expect_carried(&notified, &meter_id, carried, "the notification");
    Ok(())
}

/// Expects `answer` to hold the Meter alone, carrying what `carried` says of `n`.
fn expect_carried(answer: &[Entity<Output>], meter_id: &EntityId, carried: Carried, by: &str) {
    expect_that!(answer, len(eq(1)), "{by}");
    expect_that!(answer, nth(0, has_id(meter_id)), "{by}");
    match carried {
        Carried::Instance(value) => {
            expect_that!(answer, nth(0, has_instances("n", 1)), "{by}");
            expect_that!(answer, nth(0, has_instance("n", property().value(value))), "{by}");
        }
        Carried::Nothing => expect_that!(answer, nth(0, lacks_attribute("n")), "{by}"),
    }
}

/// Pins: for a Meter whose `n` is 10 as its default instance and 60 in a dataset, Query Entities and the notification
/// of a Subscription with that `datasetId` carry the 60 instance alone; only the Attribute instances matching the
/// datasetId are included
/// Clause: 4.5.5, 5.7.2.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_qry_ent_9014_a_dataset_id_carries_the_instance_of_that_dataset_alone() -> Result<()> {
    expect_dataset_parity(None, Dataset::Second, Carried::Instance(60)).await
}

/// Pins: for the same Meter, `datasetId=@none` carries the default instance 10 alone, in a query and a notification
/// alike; `@none` names the default Attribute instances
/// Clause: 4.5.5, 5.7.2.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_qry_ent_9015_a_none_dataset_id_carries_the_default_instance_alone() -> Result<()> {
    expect_dataset_parity(None, Dataset::Default, Carried::Instance(10)).await
}

/// Pins: for the same Meter, a `datasetId` no instance has selects the Meter and carries no instance of `n`, in a
/// query and a notification alike; the dataset narrows the instances, not the Entities
/// Clause: 4.5.5, 5.7.2.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_qry_ent_9016_an_unused_dataset_id_selects_the_entity_and_carries_no_instance() -> Result<()> {
    expect_dataset_parity(None, Dataset::Unused, Carried::Nothing).await
}

/// Pins: for the same Meter, `q=n>50` with `datasetId=@none` selects the Meter through its 60 instance and carries the
/// default instance 10 alone, in a query and a notification alike; q reads every instance and the datasetId narrows
/// what the selected Entity carries afterwards
/// Clause: 4.9, 5.7.2.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_qry_ent_9017_q_reads_every_instance_before_a_dataset_id_narrows_the_answer() -> Result<()> {
    expect_dataset_parity(Some("n>50"), Dataset::Default, Carried::Instance(10)).await
}
