//! A batch upsert creates and updates an array of objects.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    value::object,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Value},
    query::temporal_query::{self, TemporalProperty},
};

/// Pins: Batch Entity Upsert creating an Entity whose Property holds a mixed array answers 201, a second one
/// replacing it with an array of objects answers 204, and the current value and the last of two instances hold the
/// second array
/// Clause: 5.2.5, 5.6.8
#[gtest]
#[tokio::test]
async fn n_bat_ups_9012_a_batch_upsert_creates_and_updates_an_array_of_objects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let array_of_objects_upsert_id = namespace.urn("ArrayOfObjects", "upsert")?;
    let array_of_objects_type = namespace.entity_type("ArrayOfObjects")?;
    let entities = [Entity::builder()
        .id(array_of_objects_upsert_id.clone())
        .entity_type(array_of_objects_type.clone())
        .property("timezones", Value::Array(vec![Value::from(1), object([("a", Value::from(1))])]))
        .build()?];
    broker.entities().batch_upsert(&entities).send().await?.done()?;
    let entities_2 = [Entity::builder()
        .id(array_of_objects_upsert_id.clone())
        .entity_type(array_of_objects_type.clone())
        .property(
            "timezones",
            Value::Array(vec![
                object([("zoneName", Value::from("Europe/Andorra")), ("gmtOffset", Value::from(3600))]),
                object([("zoneName", Value::from("Asia/Dubai")), ("gmtOffset", Value::from(14400))]),
            ]),
        )
        .build()?];
    broker.entities().batch_upsert(&entities_2).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&array_of_objects_upsert_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "timezones",
            any_attribute()
                .length_at([], 2)
                .value_member(["0", "zoneName"], "Europe/Andorra")
                .value_member(["0", "gmtOffset"], 3600)
                .value_member(["1", "zoneName"], "Asia/Dubai")
                .value_member(["1", "gmtOffset"], 14400)
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&array_of_objects_upsert_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .attrs(vec!["timezones".parse()?])
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        evolution,
        has_evolution(
            "timezones",
            all!(
                len(eq(2)),
                nth(
                    1,
                    any_attribute()
                        .length_at([], 2)
                        .value_member(["0", "zoneName"], "Europe/Andorra")
                        .value_member(["0", "gmtOffset"], 3600)
                        .value_member(["1", "zoneName"], "Asia/Dubai")
                        .value_member(["1", "gmtOffset"], 14400)
                )
            )
        )
    );
    Ok(())
}
