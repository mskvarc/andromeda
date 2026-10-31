//! A sparse attribute of a large type is counted and paged as its streams change.

use andromeda_test_suite::{
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    identity_matcher::{has_id, ids_in_order},
    namespace::Namespace,
    nth::nth,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, Input, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{Locality, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with attrs=fault and no type, count=true, over eight Meter Evolutions of
/// which two hold fault, answers those two in ascending identifier order with NGSILD-Results-Count 2; after one of them
/// gains the type Alarm it is still answered once with the same count, and after Delete Attribute from Temporal
/// Representation of Entity removes the other's fault it is no longer answered and the count is 1
/// Clause: 5.5.9, 5.6.11, 5.6.13, 5.7.4.4, 6.3.13
/// Recorded: no order is fixed for a query without orderBy; Andromeda pages in ascending identifier order
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9080_a_sparse_attribute_of_a_large_type_is_counted_and_paged_as_its_streams_change() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let meter_type = namespace.entity_type("Meter")?;
    let alarm_type = namespace.entity_type("Alarm")?;
    let power = namespace.attribute("power")?;
    let fault = namespace.attribute("fault")?;
    let power_name = power.to_string();
    let fault_name = fault.to_string();
    let meter_03_id = namespace.urn("Meter", "03")?;
    let meter_07_id = namespace.urn("Meter", "07")?;
    let observed_at = "2020-01-01T00:00:00Z";
    let meters = (1..=8)
        .map(|n| -> Result<Entity<Input>> {
            let id = namespace.urn("Meter", format!("{n:02}"))?;
            let builder = Entity::builder()
                .id(id.clone())
                .entity_type(meter_type.clone())
                .property(power.clone(), Property::builder(n).observed_at(observed_at.parse()?));
            Ok(if id == meter_03_id {
                builder
                    .property(fault.clone(), Property::builder("overheat").observed_at(observed_at.parse()?))
                    .build()?
            } else if id == meter_07_id {
                builder
                    .property(fault.clone(), Property::builder("tamper").observed_at(observed_at.parse()?))
                    .build()?
            } else {
                builder.build()?
            })
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&meters).send().await?.done()?;
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let page = client
        .temporal()
        .query(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .attrs(vec![fault.clone().into()])
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(2)));
    expect_that!(page.items(), ids_in_order(&[meter_03_id.clone(), meter_07_id.clone()]));
    expect_that!(page.items(), nth(0, lacks_evolution(&power_name)));
    let meter_03_evolution = TemporalEntity::builder()
        .id(meter_03_id.clone())
        .entity_type(vec![meter_type, alarm_type.clone()])
        .property(fault.clone(), Property::builder("cooled").observed_at("2020-01-02T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&meter_03_evolution).send().await?.updated()?;
    let page_2 = client
        .temporal()
        .query(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .attrs(vec![fault.clone().into()])
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(2)));
    expect_that!(page_2.items(), ids_in_order(&[meter_03_id.clone(), meter_07_id.clone()]));
    expect_that!(page_2.items(), nth(0, has_evolution(&fault_name, len(eq(2)))));
    let page_3 = client
        .temporal()
        .query(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .entity_type(&alarm_type)
        .attrs(vec![fault.clone().into()])
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&meter_03_id)));
    client.temporal().delete_attribute(&meter_07_id, &fault).send().await?;
    let page_4 = client
        .temporal()
        .query(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .attrs(vec![fault.into()])
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_4.total(), some(eq(1)));
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&meter_03_id)));
    Ok(())
}
