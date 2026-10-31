//! Temporal instances may hold arrays of objects.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    value::object,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Create Temporal Evolution of an Entity with two instances holding a mixed array and an array of objects
/// answers 201, and each instance reads back its own array
/// Clause: 5.2.5, 5.6.11, 4.5.8
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9010_temporal_instances_may_hold_arrays_of_objects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let array_of_objects_temporal_id = namespace.urn("ArrayOfObjects", "temporal")?;
    let array_of_objects_type = namespace.entity_type("ArrayOfObjects")?;
    let array_of_objects_temporal_evolution = TemporalEntity::builder()
        .id(array_of_objects_temporal_id.clone())
        .entity_type(array_of_objects_type)
        .property(
            "timezones",
            Property::builder(Value::Array(vec![Value::from(1), object([("a", Value::from(1))])])).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            "timezones",
            Property::builder(Value::Array(vec![
                object([("zoneName", Value::from("Europe/Andorra")), ("gmtOffset", Value::from(3600))]),
                object([("zoneName", Value::from("Asia/Dubai")), ("gmtOffset", Value::from(14400))]),
            ]))
            .observed_at("2024-01-02T00:00:00Z".parse()?),
        )
        .build()?;
    broker
        .temporal()
        .create_or_update(&array_of_objects_temporal_evolution)
        .send()
        .await?
        .created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&array_of_objects_temporal_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "timezones",
            all!(
                len(eq(2)),
                nth(0, any_attribute().length_at([], 2).value_member(["0"], 1).value_member(["1", "a"], 1)),
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
