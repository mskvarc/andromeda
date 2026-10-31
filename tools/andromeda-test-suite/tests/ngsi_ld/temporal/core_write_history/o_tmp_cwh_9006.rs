//! Replace attribute records the replacement without the old unit code.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, NgsiLdMediaType, Property},
    query::{Locality, q, temporal_query::TemporalProperty},
};

/// Pins: Replace Attribute of temperature 25 CEL with value 30 answers 204; the Entity holds temperature without
/// `unitCode`, the Temporal Evolution holds two instances and the one holding 30 carries no `unitCode`, and q
/// selects temperature==30 and no longer temperature==25
/// Clause: 5.6.19.4, 4.5.7, 4.3.1, 4.9
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9006_replace_attribute_records_the_replacement_without_the_old_unit_code() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_replace_attribute_id = namespace.urn("Sensor", "replace-attribute")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_replace_attribute = Entity::builder()
        .id(sensor_replace_attribute_id.clone())
        .entity_type(sensor_type.clone())
        .property("temperature", Property::builder(25).unit_code(UnitCode::Cel))
        .build()?;
    broker.entities().create(&sensor_replace_attribute).send().await?.done()?;
    let attribute = Attribute::from(Property::builder(30).build()?);
    broker
        .entities()
        .replace_attribute(&sensor_replace_attribute_id, &"temperature".parse::<AttributeName>()?, &attribute)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_replace_attribute_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", any_attribute().value(30).without(Member::UnitCode)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_replace_attribute_id)
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
                len(eq(2)),
                contains(any_attribute().value(30)),
                not(contains(any_attribute().value(30).with_member(Member::UnitCode)))
            )
        )
    );
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .q(q::attr("temperature").eq(30))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&sensor_replace_attribute_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .q(q::attr("temperature").eq(25))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
