//! A subscription q equality includes a declared value held among several values by the data type its declaration
//! gives it.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    value::{xsd, xsd_typed},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{AttributeDraft, Entity, EntityId, EntityTypeName, Input, ListProperty, Lossless, NgsiLdMediaType, Property, Value},
    query::q::{self, builders::QueryBuilder},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// The array of `values`.
fn array<const N: usize>(values: [Value; N]) -> Value {
    Value::Array(values.into())
}

/// The Sensor `id` whose Property `name` is `property`.
fn sensor(id: &EntityId, sensor_type: &EntityTypeName, name: &str, property: impl AttributeDraft<Property<Input>>) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(id.clone())
        .entity_type(sensor_type.clone())
        .property(name, property)
        .build()?)
}

/// `ids` as sorted text.
fn sorted(ids: &[&EntityId]) -> Vec<String> {
    let mut ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
    ids.sort();
    ids
}

/// Pins: over Sensors whose `reading` holds `["4","7"]` declared xsd:integer by each @type, `["4","7"]` declared
/// xsd:integer by the valueType, `[4,7]`, the plain `["4","7"]`, `["true","false"]` declared xsd:boolean, and a
/// `ListProperty` whose `valueList` is `["4"]` declared xsd:integer, and a Sensor whose `level` holds the sub-attribute
/// `raw` `["4","7"]` declared xsd:integer by the valueType, a subscription with `q=reading==4` is notified of both
/// declared integers, the numbers and the list, one with `reading=="4"` of the plain strings, one with `reading!=4`
/// of the plain strings and the Booleans, one with `reading==true` of the Booleans, and one with `level.raw==4` of
/// the Sensor holding `raw`, exactly the Entities Query Entities selects with the same q
/// Clause: 4.9, 4.5.2, 5.8.6, C.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9032_a_subscription_q_equality_includes_a_declared_value_held_among_several_values() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let typed_integers_id = namespace.urn("Sensor", "typed-integers")?;
    let value_type_integers_id = namespace.urn("Sensor", "value-type-integers")?;
    let numbers_id = namespace.urn("Sensor", "numbers")?;
    let strings_id = namespace.urn("Sensor", "strings")?;
    let typed_booleans_id = namespace.urn("Sensor", "typed-booleans")?;
    let list_id = namespace.urn("Sensor", "list")?;
    let raw_id = namespace.urn("Sensor", "raw")?;
    let reading = || q::attr("reading");
    let expectations: [(QueryBuilder, Vec<&EntityId>); 5] = [
        (reading().eq(4), vec![&typed_integers_id, &value_type_integers_id, &numbers_id, &list_id]),
        (reading().eq("4"), vec![&strings_id]),
        (reading().ne(4), vec![&strings_id, &typed_booleans_id]),
        (reading().eq(true), vec![&typed_booleans_id]),
        (q::attr("level").sub_attr("raw").eq(4), vec![&raw_id]),
    ];
    for (endpoint, (q, _)) in (1..).zip(&expectations) {
        let subscription = Subscription::builder()
            .id(namespace.subscription_id(endpoint)?)
            .entities([sensor_type.as_str()])
            .notification_triggers([NotificationTrigger::EntityCreated])
            .q(q.clone().build()?)
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    let entities = [
        sensor(
            &typed_integers_id,
            &sensor_type,
            "reading",
            Property::builder(array([xsd_typed("integer", "4")?, xsd_typed("integer", "7")?])),
        )?,
        sensor(
            &value_type_integers_id,
            &sensor_type,
            "reading",
            Property::builder(array([Value::from("4"), Value::from("7")])).value_type(xsd("integer")?),
        )?,
        sensor(&numbers_id, &sensor_type, "reading", Property::builder(array([Value::from(4), Value::from(7)])))?,
        sensor(
            &strings_id,
            &sensor_type,
            "reading",
            Property::builder(array([Value::from("4"), Value::from("7")])),
        )?,
        sensor(
            &typed_booleans_id,
            &sensor_type,
            "reading",
            Property::builder(array([xsd_typed("boolean", "true")?, xsd_typed("boolean", "false")?])),
        )?,
        Entity::builder()
            .id(list_id.clone())
            .entity_type(sensor_type.clone())
            .list_property("reading", ListProperty::builder(vec![xsd_typed("integer", "4")?]))
            .build()?,
        sensor(
            &raw_id,
            &sensor_type,
            "level",
            Property::builder(1).property(
                "raw",
                Property::builder(array([Value::from("4"), Value::from("7")])).value_type(xsd("integer")?),
            ),
        )?,
    ];
    for entity in &entities {
        broker.entities().create(entity).send().await?.done()?;
    }
    for (endpoint, (_, expected)) in (1..).zip(&expectations) {
        sink.wait_for_at_least(endpoint, expected.len()).await?;
    }
    for (endpoint, (_, expected)) in (1..).zip(&expectations) {
        let mut notified = Vec::new();
        for notification in sink.received(endpoint).await? {
            notified.extend(notification.entities(Lossless::Normalized)?.data().iter().map(|entity| entity.id.to_string()));
        }
        notified.sort();
        expect_that!(notified, eq(&sorted(expected)), "subscription {endpoint}");
    }
    Ok(())
}
