//! Q with an after window admits the instance in effect when it opens.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::{Done, Upserted},
};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, DateTime, Entity, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: an after window opens at its instant exactly as a between window does, so a temporal q admits the same entities
/// by the instances in effect when it opens and leaves out the entities whose value a deletion or an instance at the
/// window start replaced
/// Clause: 5.7.4.4, 4.11, 4.9, 4.5.7
/// Recorded: clause 5.7.4.4 checks q against the Attribute instances "during the interval defined by the temporal query"
/// and clause 4.11 makes them the values "valid within the specified timeframe", without saying whether the instance
/// observed before the window opens and not yet replaced is one of them; Andromeda reads it as one (as Annex C clause
/// C.5.5 evaluates a q on an attribute it does not render), evaluated by q but never rendered
#[gtest]
#[tokio::test]
async fn o_tmp_qry_0005_q_with_an_after_window_admits_the_instance_in_effect_when_it_opens() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let meter_type = namespace.entity_type("Meter")?;
    let meter_m1_id = namespace.urn("Meter", "M1")?;
    let meter_m2_id = namespace.urn("Meter", "M2")?;
    let meter_m3_id = namespace.urn("Meter", "M3")?;
    let meter_m4_id = namespace.urn("Meter", "M4")?;
    let meter_m5_id = namespace.urn("Meter", "M5")?;
    let meter_m6_id = namespace.urn("Meter", "M6")?;
    let now = Utc::now();
    let t30: DateTime = (now + TimeDelta::minutes(-15)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let t50: DateTime = (now + TimeDelta::minutes(5)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let t60: DateTime = (now + TimeDelta::minutes(15)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let t75: DateTime = (now + TimeDelta::minutes(30)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let t90: DateTime = (now + TimeDelta::minutes(45)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let meter_m1_evolution = TemporalEntity::builder()
        .id(meter_m1_id.clone())
        .entity_type(meter_type.clone())
        .property("power", Property::builder(5).observed_at(t30))
        .property("power", Property::builder(9).observed_at(t90))
        .build()?;
    broker.temporal().create_or_update(&meter_m1_evolution).send().await?.created()?;
    let meter_m2_evolution = TemporalEntity::builder()
        .id(meter_m2_id.clone())
        .entity_type(meter_type.clone())
        .property("power", Property::builder(9).observed_at(t30))
        .build()?;
    broker.temporal().create_or_update(&meter_m2_evolution).send().await?.created()?;
    let meter_m4_evolution = TemporalEntity::builder()
        .id(meter_m4_id.clone())
        .entity_type(meter_type.clone())
        .property("power", Property::builder(9).observed_at(t30))
        .property("power", Property::builder(1).observed_at(t60))
        .build()?;
    broker.temporal().create_or_update(&meter_m4_evolution).send().await?.created()?;
    let meter_m5_evolution = TemporalEntity::builder()
        .id(meter_m5_id.clone())
        .entity_type(meter_type.clone())
        .property("power", Property::builder(1).observed_at(t30))
        .property("power", Property::builder(9).observed_at(t30))
        .build()?;
    broker.temporal().create_or_update(&meter_m5_evolution).send().await?.created()?;
    let meter_m6_evolution = TemporalEntity::builder()
        .id(meter_m6_id.clone())
        .entity_type(meter_type.clone())
        .property("power", Property::builder(9).dataset_id(namespace.dataset_id("Dataset", "a")?).observed_at(t30))
        .property("power", Property::builder(1).observed_at(t50))
        .build()?;
    broker.temporal().create_or_update(&meter_m6_evolution).send().await?.created()?;
    let meter_m3 = Entity::builder()
        .id(meter_m3_id.clone())
        .entity_type(meter_type.clone())
        .property("power", Property::builder(9).observed_at(t30))
        .build()?;
    broker.entities().create(&meter_m3).send().await?.done()?;
    broker
        .entities()
        .delete_attribute(&meter_m3_id, &"power".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    for id in [&meter_m1_id, &meter_m2_id, &meter_m3_id, &meter_m4_id, &meter_m5_id, &meter_m6_id] {
        let instances = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(meter_type.clone())
            .property("status", Property::builder("on").observed_at(t75))
            .build()?
            .attributes;
        broker.temporal().add_attributes(id, &instances).send().await?;
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after(t60))
        .entity_type(&meter_type)
        .q(q::attr("power").gt(8))
        .attrs(vec!["status".parse()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(4)));
    for id in [&meter_m1_id, &meter_m2_id, &meter_m5_id, &meter_m6_id] {
        expect_that!(page.items(), contains(has_id(id)));
        let entity = page.items().iter().find(|entity| &entity.id == id).or_fail()?;
        expect_that!(entity.attributes.len(), eq(1));
        expect_that!(entity, has_evolution("status", all!(len(eq(1)), contains(any_attribute().observed_at(t75)))));
    }
    Ok(())
}
