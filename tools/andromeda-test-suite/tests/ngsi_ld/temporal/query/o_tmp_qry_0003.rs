//! Q count and pages hold exactly the entities q admits.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::{Done, Upserted},
};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{AttributeName, DateTime, Entity, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: with a q read against the instances in effect over a between window, count=true reports the four entities
/// q admits and limit and offset page through exactly those four in identifier order
/// Clause: 5.7.4.4, 5.5.9, 6.3.10, 6.3.13, 4.11, 4.9
/// Recorded: clause 5.7.4.4 checks q against the Attribute instances "during the interval defined by the temporal
/// query" and clause 4.11 makes them the values "valid within the specified timeframe", without saying whether the
/// instance observed before the window opens and not yet replaced is one of them; Andromeda reads it as one (as
/// Annex C clause C.5.5 evaluates a q on an attribute it does not render), evaluated by q but never rendered; the
/// identifier order of the pages is Andromeda's choice (clause 5.5.9 sets none)
#[gtest]
#[tokio::test]
async fn o_tmp_qry_0003_q_count_and_pages_hold_exactly_the_entities_q_admits() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let meter_m1_id = namespace.urn("Meter", "M1")?;
    let meter_type = namespace.entity_type("Meter")?;
    let meter_m2_id = namespace.urn("Meter", "M2")?;
    let meter_m4_id = namespace.urn("Meter", "M4")?;
    let meter_m5_id = namespace.urn("Meter", "M5")?;
    let meter_m6_id = namespace.urn("Meter", "M6")?;
    let meter_m3_id = namespace.urn("Meter", "M3")?;
    let now = Utc::now();
    let t30: DateTime = (now + TimeDelta::minutes(-15)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let t50: DateTime = (now + TimeDelta::minutes(5)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let t60: DateTime = (now + TimeDelta::minutes(15)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let t75: DateTime = (now + TimeDelta::minutes(30)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let t90: DateTime = (now + TimeDelta::minutes(45)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let t120: DateTime = (now + TimeDelta::minutes(75)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
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
    let instances = TemporalEntity::builder()
        .id(meter_m1_id.clone())
        .entity_type(meter_type.clone())
        .property("status", Property::builder("on").observed_at(t75))
        .build()?
        .attributes;
    broker.temporal().add_attributes(&meter_m1_id, &instances).send().await?;
    let instances_2 = TemporalEntity::builder()
        .id(meter_m2_id.clone())
        .entity_type(meter_type.clone())
        .property("status", Property::builder("on").observed_at(t75))
        .build()?
        .attributes;
    broker.temporal().add_attributes(&meter_m2_id, &instances_2).send().await?;
    let instances_3 = TemporalEntity::builder()
        .id(meter_m3_id.clone())
        .entity_type(meter_type.clone())
        .property("status", Property::builder("on").observed_at(t75))
        .build()?
        .attributes;
    broker.temporal().add_attributes(&meter_m3_id, &instances_3).send().await?;
    let instances_4 = TemporalEntity::builder()
        .id(meter_m4_id.clone())
        .entity_type(meter_type.clone())
        .property("status", Property::builder("on").observed_at(t75))
        .build()?
        .attributes;
    broker.temporal().add_attributes(&meter_m4_id, &instances_4).send().await?;
    let instances_5 = TemporalEntity::builder()
        .id(meter_m5_id.clone())
        .entity_type(meter_type.clone())
        .property("status", Property::builder("on").observed_at(t75))
        .build()?
        .attributes;
    broker.temporal().add_attributes(&meter_m5_id, &instances_5).send().await?;
    let instances_6 = TemporalEntity::builder()
        .id(meter_m6_id.clone())
        .entity_type(meter_type.clone())
        .property("status", Property::builder("on").observed_at(t75))
        .build()?
        .attributes;
    broker.temporal().add_attributes(&meter_m6_id, &instances_6).send().await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between(t60, t120).last_n(1))
        .entity_type(&meter_type)
        .q(q::attr("power").gt(8))
        .attrs(vec!["status".parse()?])
        .count(ResultsCount::Include)
        .limit(2)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(4)));
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&meter_m1_id)));
    expect_that!(page.items(), nth(1, has_id(&meter_m2_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between(t60, t120).last_n(1))
        .entity_type(&meter_type)
        .q(q::attr("power").gt(8))
        .attrs(vec!["status".parse()?])
        .count(ResultsCount::Include)
        .limit(2)
        .offset(2)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(4)));
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&meter_m5_id)));
    expect_that!(page_2.items(), nth(1, has_id(&meter_m6_id)));
    Ok(())
}
