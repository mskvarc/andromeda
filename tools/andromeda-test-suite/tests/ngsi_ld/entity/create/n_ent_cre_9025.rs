//! A property value may mix a scalar with an object.

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

/// Pins: Create Entity with a Property whose value is an array holding a number and an object answers 201, and the
/// current value and the one recorded instance hold that array
/// Clause: 5.2.5, 5.6.1, 4.5.8
#[gtest]
#[tokio::test]
async fn n_ent_cre_9025_a_property_value_may_mix_a_scalar_with_an_object() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let array_of_objects_mixed_id = namespace.urn("ArrayOfObjects", "mixed")?;
    let array_of_objects_type = namespace.entity_type("ArrayOfObjects")?;
    let array_of_objects_mixed = Entity::builder()
        .id(array_of_objects_mixed_id.clone())
        .entity_type(array_of_objects_type)
        .property("mixed", Value::Array(vec![Value::from(1), object([("a", Value::from(1))])]))
        .build()?;
    broker.entities().create(&array_of_objects_mixed).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&array_of_objects_mixed_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("mixed", any_attribute().length_at([], 2).value_member(["0"], 1).value_member(["1", "a"], 1))
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&array_of_objects_mixed_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .attrs(vec!["mixed".parse()?])
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        evolution,
        has_evolution(
            "mixed",
            all!(
                len(eq(1)),
                nth(0, any_attribute().length_at([], 2).value_member(["0"], 1).value_member(["1", "a"], 1))
            )
        )
    );
    Ok(())
}
