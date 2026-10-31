//! A temporal values page keeps an emptied attribute with its type.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, received_status::ExpectedStatus, values_matcher::has_values};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities with `options=temporalValues` over an Entity with 30 instances and
/// one whose only instance lies beyond the page cut answers 206, the first speed with 19 values and the second
/// speed as a Property with no values
/// Clause: 4.5.9, 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 instances per attribute without
/// `lastN` and with `lastN` (clause 6.3.10 leaves both to the implementation)
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9030_a_temporal_values_page_keeps_an_emptied_attribute_with_its_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let paged_values_fleet_early_id = namespace.urn("PagedValuesFleet", "early")?;
    let paged_values_fleet_type = namespace.entity_type("PagedValuesFleet")?;
    let paged_values_fleet_late_id = namespace.urn("PagedValuesFleet", "late")?;
    let paged_values_fleet_early_evolution = TemporalEntity::builder()
        .id(paged_values_fleet_early_id.clone())
        .entity_type(paged_values_fleet_type.clone())
        .property("speed", Property::builder(0).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(1).observed_at("2024-01-01T00:01:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2024-01-01T00:02:00Z".parse()?))
        .property("speed", Property::builder(3).observed_at("2024-01-01T00:03:00Z".parse()?))
        .property("speed", Property::builder(4).observed_at("2024-01-01T00:04:00Z".parse()?))
        .property("speed", Property::builder(5).observed_at("2024-01-01T00:05:00Z".parse()?))
        .property("speed", Property::builder(6).observed_at("2024-01-01T00:06:00Z".parse()?))
        .property("speed", Property::builder(7).observed_at("2024-01-01T00:07:00Z".parse()?))
        .property("speed", Property::builder(8).observed_at("2024-01-01T00:08:00Z".parse()?))
        .property("speed", Property::builder(9).observed_at("2024-01-01T00:09:00Z".parse()?))
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:10:00Z".parse()?))
        .property("speed", Property::builder(11).observed_at("2024-01-01T00:11:00Z".parse()?))
        .property("speed", Property::builder(12).observed_at("2024-01-01T00:12:00Z".parse()?))
        .property("speed", Property::builder(13).observed_at("2024-01-01T00:13:00Z".parse()?))
        .property("speed", Property::builder(14).observed_at("2024-01-01T00:14:00Z".parse()?))
        .property("speed", Property::builder(15).observed_at("2024-01-01T00:15:00Z".parse()?))
        .property("speed", Property::builder(16).observed_at("2024-01-01T00:16:00Z".parse()?))
        .property("speed", Property::builder(17).observed_at("2024-01-01T00:17:00Z".parse()?))
        .property("speed", Property::builder(18).observed_at("2024-01-01T00:18:00Z".parse()?))
        .property("speed", Property::builder(19).observed_at("2024-01-01T00:19:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T00:20:00Z".parse()?))
        .property("speed", Property::builder(21).observed_at("2024-01-01T00:21:00Z".parse()?))
        .property("speed", Property::builder(22).observed_at("2024-01-01T00:22:00Z".parse()?))
        .property("speed", Property::builder(23).observed_at("2024-01-01T00:23:00Z".parse()?))
        .property("speed", Property::builder(24).observed_at("2024-01-01T00:24:00Z".parse()?))
        .property("speed", Property::builder(25).observed_at("2024-01-01T00:25:00Z".parse()?))
        .property("speed", Property::builder(26).observed_at("2024-01-01T00:26:00Z".parse()?))
        .property("speed", Property::builder(27).observed_at("2024-01-01T00:27:00Z".parse()?))
        .property("speed", Property::builder(28).observed_at("2024-01-01T00:28:00Z".parse()?))
        .property("speed", Property::builder(29).observed_at("2024-01-01T00:29:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&paged_values_fleet_early_evolution)
        .send()
        .await?
        .created()?;
    let paged_values_fleet_late_evolution = TemporalEntity::builder()
        .id(paged_values_fleet_late_id.clone())
        .entity_type(paged_values_fleet_type.clone())
        .property("speed", Property::builder(500).observed_at("2024-07-19T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&paged_values_fleet_late_evolution).send().await?.created()?;
    let page = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query_values(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .entity_type(&paged_values_fleet_type)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?
        .expecting(StatusCode::PARTIAL_CONTENT)?;
    let values = page.items().iter().find(|item| item.id == paged_values_fleet_early_id).or_fail()?;
    expect_that!(values, has_values("speed", len(eq(19))));
    let values_2 = page.items().iter().find(|item| item.id == paged_values_fleet_late_id).or_fail()?;
    expect_that!(values_2, has_values("speed", len(eq(0))));
    Ok(())
}
