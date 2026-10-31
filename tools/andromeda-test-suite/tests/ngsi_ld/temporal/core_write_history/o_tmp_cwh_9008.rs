//! An append without overwrite records only the new attribute.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::PartialSuccess,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::Overwrite},
    model::{Entity, EntityFragment, NgsiLdMediaType},
    query::{Locality, q, temporal_query::TemporalProperty},
};

/// Pins: Append Attributes with `options=noOverwrite` naming the held temperature with 99 and a new name answers
/// 207; the Entity keeps temperature 25, the Temporal Evolution of temperature keeps its one instance, q selects
/// temperature==25 and not temperature==99, and name has one recorded instance
/// Clause: 5.6.3.4, 4.5.7, 4.3.1, 4.9
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9008_an_append_without_overwrite_records_only_the_new_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_append_no_overwrite_id = namespace.urn("Sensor", "append-no-overwrite")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_append_no_overwrite = Entity::builder()
        .id(sensor_append_no_overwrite_id.clone())
        .entity_type(sensor_type.clone())
        .property("temperature", 25)
        .build()?;
    broker.entities().create(&sensor_append_no_overwrite).send().await?.done()?;
    let fragment = EntityFragment::builder().property("temperature", 99).property("name", "lobby").build()?;
    broker
        .entities()
        .append_attributes(&sensor_append_no_overwrite_id, &fragment)
        .overwrite(Overwrite::Keep)
        .send()
        .await?
        .partial()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_append_no_overwrite_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", any_attribute().value(25)));
    expect_that!(entity, has_attribute("name", any_attribute().value("lobby")));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_append_no_overwrite_id)
        .time_property(TemporalProperty::ModifiedAt)
        .attrs(vec!["temperature".parse()?])
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(evolution, has_evolution("temperature", all!(len(eq(1)), nth(0, any_attribute().value(25)))));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .q(q::attr("temperature").eq(25))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&sensor_append_no_overwrite_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .q(q::attr("temperature").eq(99))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_append_no_overwrite_id)
        .time_property(TemporalProperty::ModifiedAt)
        .attrs(vec!["name".parse()?])
        .send()
        .await?;
    let evolution_2 = retrieved_3.body();
    expect_that!(evolution_2, has_evolution("name", all!(len(eq(1)), nth(0, any_attribute().value("lobby")))));
    Ok(())
}
