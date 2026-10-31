//! A temporal capture reads every instance page of its query.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    eventually::retried,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, EntityId, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{
        temporal_query::{self, TemporalQuery},
        type_selection,
    },
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot whose temporal query selects a Temporal Evolution of 250 instances of one attribute, many times
/// the stack's instance page of 19, captures every instance once: under it the query pages with a 206, and thirteen
/// windows of 19 instances and the last 3 each answer 200 with exactly their instances
/// Clause: 5.16.1.4, 5.7.4.4, 6.3.10
#[gtest]
#[tokio::test]
async fn n_snp_cre_8127_a_temporal_capture_reads_every_instance_page_of_its_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = format!("urn:ngsi-ld:Vehicle:{namespace}").parse::<EntityId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}").parse::<SnapshotId>()?;
    let observed = |n: u32| -> Result<DateTime> { Ok(format!("2025-01-01T{:02}:{:02}:00Z", n / 60, n % 60).parse()?) };
    let evolution = (0..250_u32)
        .try_fold(
            TemporalEntity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()),
            |builder, n| -> Result<_> { Ok(builder.property("speed", Property::builder(n).observed_at(observed(n)?))) },
        )?
        .build()?;
    broker.temporal().create_or_update(&evolution).send().await?.created()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .temporal_query(
            QueryDocument::temporal_builder(TemporalQuery::after("2024-01-01T00:00:00Z".parse()?))
                .entities([vehicle_type.as_str()])
                .build()?,
        )
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(retrieved.status(), eq("success".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let selection = type_selection::entity_type(&vehicle_type).build()?;
    // The first page of the open window is partial: the answer pages with a 206.
    let first = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .temporal()
        .query(temporal_query::after("2024-01-01T00:00:00Z".parse()?))
        .entity_type(selection.clone())
        .send()
        .await?;
    expect_that!(first.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(first.outcome.items(), len(eq(1)));
    let first_evolution = first.outcome.items().first().or_fail()?;
    expect_that!(first_evolution, has_evolution("speed", all!(len(eq(19)), nth(0, any_attribute().value(0)))));
    for window in 0..13_u32 {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .on_snapshot(&snapshot_id)
            .temporal()
            .query(temporal_query::between(observed(window * 19)?, observed(window * 19 + 19)?))
            .entity_type(selection.clone())
            .send()
            .await?;
        let evolution = page.items().first().or_fail()?;
        expect_that!(
            evolution,
            has_evolution(
                "speed",
                all!(
                    len(eq(19)),
                    nth(0, any_attribute().value(window * 19)),
                    nth(18, any_attribute().value(window * 19 + 18))
                )
            )
        );
    }
    let last = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .temporal()
        .query(temporal_query::after("2025-01-01T04:07:00Z".parse()?))
        .entity_type(selection)
        .send()
        .await?;
    let last_evolution = last.items().first().or_fail()?;
    expect_that!(
        last_evolution,
        has_evolution(
            "speed",
            all!(len(eq(3)), nth(0, any_attribute().value(247)), nth(2, any_attribute().value(249)))
        )
    );
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
