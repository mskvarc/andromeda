//! Replace entity records every attribute of the replacement.

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
    model::{Entity, NgsiLdMediaType, Property},
    query::{Locality, q, temporal_query::TemporalProperty},
};

/// Pins: Replace Entity of temperature 25 CEL with temperature 40 and name lobby answers 204; the recorded instance
/// holding 40 carries no `unitCode`, name has one recorded instance, and q selects temperature==40 and
/// name=="lobby"
/// Clause: 5.6.18, 4.5.7, 4.3.1, 4.9
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9011_replace_entity_records_every_attribute_of_the_replacement() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_replace_entity_id = namespace.urn("Sensor", "replace-entity")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_replace_entity = Entity::builder()
        .id(sensor_replace_entity_id.clone())
        .entity_type(sensor_type.clone())
        .property("temperature", Property::builder(25).unit_code(UnitCode::Cel))
        .build()?;
    broker.entities().create(&sensor_replace_entity).send().await?.done()?;
    let sensor_replace_entity_replacement = Entity::builder()
        .id(sensor_replace_entity_id.clone())
        .entity_type(sensor_type.clone())
        .property("temperature", 40)
        .property("name", "lobby")
        .build()?;
    broker.entities().replace(&sensor_replace_entity_replacement).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_replace_entity_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", any_attribute().value(40).without(Member::UnitCode)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_replace_entity_id)
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
                contains(any_attribute().value(40)),
                not(contains(any_attribute().value(40).with_member(Member::UnitCode)))
            )
        )
    );
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_replace_entity_id)
        .time_property(TemporalProperty::ModifiedAt)
        .attrs(vec!["name".parse()?])
        .send()
        .await?;
    let evolution_2 = retrieved_3.body();
    expect_that!(evolution_2, has_evolution("name", len(eq(1))));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .q(q::attr("temperature").eq(40))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&sensor_replace_entity_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .q(q::attr("name").eq("lobby"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), contains(has_id(&sensor_replace_entity_id)));
    Ok(())
}
