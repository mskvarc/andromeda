//! A property value may be an array of several objects.

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

/// Pins: Create Entity with a Property whose value is an array of two objects answers 201, and the current value
/// and the one recorded instance hold that array
/// Clause: 5.2.5, 5.6.1, 4.5.8
#[gtest]
#[tokio::test]
async fn n_ent_cre_9024_a_property_value_may_be_an_array_of_several_objects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let array_of_objects_create_id = namespace.urn("ArrayOfObjects", "create")?;
    let array_of_objects_type = namespace.entity_type("ArrayOfObjects")?;
    let array_of_objects_create = Entity::builder()
        .id(array_of_objects_create_id.clone())
        .entity_type(array_of_objects_type)
        .property(
            "timezones",
            Value::Array(vec![
                object([("zoneName", Value::from("Europe/Andorra")), ("gmtOffset", Value::from(3600))]),
                object([("zoneName", Value::from("Asia/Dubai")), ("gmtOffset", Value::from(14400))]),
            ]),
        )
        .build()?;
    broker.entities().create(&array_of_objects_create).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&array_of_objects_create_id)
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
        .retrieve(&array_of_objects_create_id)
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
    Ok(())
}
