//! Update attributes records the whole new instance without the old unit code.

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
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
    query::{Locality, q, temporal_query::TemporalProperty},
};

/// Pins: Update Attributes with temperature 100 over temperature 25 CEL answers 204; the Entity holds temperature
/// without `unitCode`, the recorded instance holding 100 carries no `unitCode`, and q selects temperature==100
/// Clause: 5.6.2, 5.5.8 (EXAMPLE 2), 4.5.7, 4.3.1, 4.9
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9007_update_attributes_records_the_whole_new_instance_without_the_old_unit_code() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_update_attributes_id = namespace.urn("Sensor", "update-attributes")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_update_attributes = Entity::builder()
        .id(sensor_update_attributes_id.clone())
        .entity_type(sensor_type.clone())
        .property("temperature", Property::builder(25).unit_code(UnitCode::Cel))
        .build()?;
    broker.entities().create(&sensor_update_attributes).send().await?.done()?;
    let fragment = EntityFragment::builder().property("temperature", 100).build()?;
    broker
        .entities()
        .update_attributes(&sensor_update_attributes_id, &fragment)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_update_attributes_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", any_attribute().value(100).without(Member::UnitCode)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_update_attributes_id)
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
                contains(any_attribute().value(100)),
                not(contains(any_attribute().value(100).with_member(Member::UnitCode)))
            )
        )
    );
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .q(q::attr("temperature").eq(100))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&sensor_update_attributes_id)));
    Ok(())
}
