//! A batch creation accepts an array of objects for every entity.

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

/// Pins: Batch Entity Creation of two Entities whose Property holds an array of objects and a mixed array answers
/// 201, and each reads back its own array
/// Clause: 5.2.5, 5.6.7
#[gtest]
#[tokio::test]
async fn n_bat_cre_9006_a_batch_creation_accepts_an_array_of_objects_for_every_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let array_of_objects_batch_1_id = namespace.urn("ArrayOfObjects", "batch-1")?;
    let array_of_objects_type = namespace.entity_type("ArrayOfObjects")?;
    let array_of_objects_batch_2_id = namespace.urn("ArrayOfObjects", "batch-2")?;
    let entities = [
        Entity::builder()
            .id(array_of_objects_batch_1_id.clone())
            .entity_type(array_of_objects_type.clone())
            .property(
                "timezones",
                Value::Array(vec![
                    object([("zoneName", Value::from("Europe/Andorra")), ("gmtOffset", Value::from(3600))]),
                    object([("zoneName", Value::from("Asia/Dubai")), ("gmtOffset", Value::from(14400))]),
                ]),
            )
            .build()?,
        Entity::builder()
            .id(array_of_objects_batch_2_id.clone())
            .entity_type(array_of_objects_type.clone())
            .property("timezones", Value::Array(vec![Value::from(1), object([("a", Value::from(1))])]))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&array_of_objects_batch_1_id)
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
        .retrieve(&array_of_objects_batch_1_id)
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
                len(eq(1)),
                nth(
                    0,
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
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&array_of_objects_batch_2_id)
        .send()
        .await?;
    let entity_2 = retrieved_3.body();
    expect_that!(
        entity_2,
        has_attribute("timezones", any_attribute().length_at([], 2).value_member(["0"], 1).value_member(["1", "a"], 1))
    );
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&array_of_objects_batch_2_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .attrs(vec!["timezones".parse()?])
        .send()
        .await?;
    let evolution_2 = retrieved_4.body();
    expect_that!(
        evolution_2,
        has_evolution(
            "timezones",
            all!(
                len(eq(1)),
                nth(0, any_attribute().length_at([], 2).value_member(["0"], 1).value_member(["1", "a"], 1))
            )
        )
    );
    Ok(())
}
