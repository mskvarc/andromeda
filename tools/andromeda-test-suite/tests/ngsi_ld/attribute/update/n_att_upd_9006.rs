//! Update and append write arrays of objects.

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

/// Pins: Update Attributes replacing a string with an array of objects and Append Attributes adding a mixed array
/// each answer 204, and both arrays read back current and recorded
/// Clause: 5.2.5, 5.6.2, 5.6.3
#[gtest]
#[tokio::test]
async fn n_att_upd_9006_update_and_append_write_arrays_of_objects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let array_of_objects_attrs_id = namespace.urn("ArrayOfObjects", "attrs")?;
    let array_of_objects_type = namespace.entity_type("ArrayOfObjects")?;
    let array_of_objects_attrs = Entity::builder()
        .id(array_of_objects_attrs_id.clone())
        .entity_type(array_of_objects_type)
        .property("timezones", "none")
        .build()?;
    broker.entities().create(&array_of_objects_attrs).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property(
            "timezones",
            Value::Array(vec![
                object([("zoneName", Value::from("Europe/Andorra")), ("gmtOffset", Value::from(3600))]),
                object([("zoneName", Value::from("Asia/Dubai")), ("gmtOffset", Value::from(14400))]),
            ]),
        )
        .build()?;
    broker
        .entities()
        .update_attributes(&array_of_objects_attrs_id, &fragment)
        .send()
        .await?
        .done()?;
    let fragment_2 = EntityFragment::builder()
        .property("mixed", Value::Array(vec![Value::from(1), object([("a", Value::from(1))])]))
        .build()?;
    broker
        .entities()
        .append_attributes(&array_of_objects_attrs_id, &fragment_2)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&array_of_objects_attrs_id)
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
        .retrieve(&array_of_objects_attrs_id)
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
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&array_of_objects_attrs_id)
        .send()
        .await?;
    let entity_2 = retrieved_3.body();
    expect_that!(
        entity_2,
        has_attribute("mixed", any_attribute().length_at([], 2).value_member(["0"], 1).value_member(["1", "a"], 1))
    );
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&array_of_objects_attrs_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .attrs(vec!["mixed".parse()?])
        .send()
        .await?;
    let evolution_2 = retrieved_4.body();
    expect_that!(
        evolution_2,
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
