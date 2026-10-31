//! A partial attribute update records a sub property inside its attribute.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    evolution_matcher::{has_evolution, lacks_evolution},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, NgsiLdMediaType, Property, Value},
    query::{Locality, q, temporal_query::TemporalProperty},
};

/// Pins: Partial Attribute Update of temperature with value 26 and a sub-Property accuracy 0.5 answers 204; the
/// Entity holds accuracy inside temperature and not as an attribute of its own, the recorded instance holding 26
/// carries accuracy 0.5, the Temporal Evolution has no accuracy attribute, and q selects temperature==26
/// Clause: 5.6.4, 4.5.2, 4.5.7, 4.3.1, 4.9
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9004_a_partial_attribute_update_records_a_sub_property_inside_its_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_partial_sub_property_id = namespace.urn("Sensor", "partial-sub-property")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_partial_sub_property = Entity::builder()
        .id(sensor_partial_sub_property_id.clone())
        .entity_type(sensor_type.clone())
        .property("temperature", 25)
        .build()?;
    broker.entities().create(&sensor_partial_sub_property).send().await?.done()?;
    let patch = Attribute::from(Property::builder(26).property("accuracy", Value::Number("0.5".parse()?)).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&sensor_partial_sub_property_id, &"temperature".parse::<AttributeName>()?, patch)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_partial_sub_property_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("accuracy"));
    expect_that!(
        entity,
        has_attribute(
            "temperature",
            any_attribute().sub_attribute("accuracy", any_attribute().value(Value::Number("0.5".parse()?)))
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_partial_sub_property_id)
        .time_property(TemporalProperty::ModifiedAt)
        .attrs(vec!["temperature".parse()?])
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        evolution,
        has_evolution(
            "temperature",
            all!(
                contains(
                    any_attribute()
                        .value(26)
                        .sub_attribute("accuracy", any_attribute().value(Value::Number("0.5".parse()?)))
                ),
                contains(any_attribute().value(26)).times(eq(1))
            )
        )
    );
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_partial_sub_property_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    let evolution_2 = retrieved_3.body();
    expect_that!(evolution_2, has_evolution("temperature", anything()));
    expect_that!(evolution_2, lacks_evolution("accuracy"));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .q(q::attr("temperature").eq(26))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&sensor_partial_sub_property_id)));
    Ok(())
}
