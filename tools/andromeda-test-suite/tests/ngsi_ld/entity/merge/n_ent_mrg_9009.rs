//! A merge writes an array of objects over a scalar.

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
    model::{Entity, EntityFragment, NgsiLdMediaType, Value},
    query::temporal_query::{self, TemporalProperty},
};

/// Pins: Merge Entity replacing a string value with an array of objects answers 204, and the current value and the
/// last of two instances hold the array
/// Clause: 5.2.5, 5.6.17
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9009_a_merge_writes_an_array_of_objects_over_a_scalar() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let array_of_objects_merge_id = namespace.urn("ArrayOfObjects", "merge")?;
    let array_of_objects_type = namespace.entity_type("ArrayOfObjects")?;
    let array_of_objects_merge = Entity::builder()
        .id(array_of_objects_merge_id.clone())
        .entity_type(array_of_objects_type)
        .property("timezones", "none")
        .build()?;
    broker.entities().create(&array_of_objects_merge).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property(
            "timezones",
            Value::Array(vec![
                object([("zoneName", Value::from("Europe/Andorra")), ("gmtOffset", Value::from(3600))]),
                object([("zoneName", Value::from("Asia/Dubai")), ("gmtOffset", Value::from(14400))]),
            ]),
        )
        .build()?;
    broker.entities().merge(&array_of_objects_merge_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&array_of_objects_merge_id)
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
        .retrieve(&array_of_objects_merge_id)
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
