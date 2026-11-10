//! A temporal capture of a last n above the instance maximum keeps the last n instances of every attribute.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    cleanup::Cleanup,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    paged_history::{series, stamp},
    snapshot_capture::{SnapshotQuery, await_query_result},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, evolution::AggregationMethod},
    query::temporal_query,
    snapshot::{ExecutionStatus, QueryDocument, SettledStatus, Snapshot, SnapshotId},
};

/// The `lastN` the captured query asks for, above the 19 instances per attribute the broker answers at once.
const LAST_N: u64 = 40;

/// Pins: On the broker answering at most 19 instances per attribute per page, a Snapshot with a temporal query after
/// 2024-01-01T00:00:00Z with `lastN` 40 over a type the broker holds the history of one Entity of, `speed` 1 to 45
/// observed at minutes 1 to 45 of 2024-01-01 and one `fuel` instance of 100 observed at minute 0, ends success, its
/// temporal query's execution details are success with no `problemDetails`, and Retrieve Temporal Evolution of the
/// Entity under the Snapshot with `aggrMethods` totalCount,min,max over the same window answers `speed` totalCount 40,
/// min 6 and max 45, and `fuel` totalCount 1
/// Clause: Table 5.2.21-1, 5.16.1.4, 5.7.4.4, 6.3.10, 4.5.19
#[gtest]
#[tokio::test]
async fn n_snp_cre_8182_a_temporal_capture_of_a_last_n_above_the_instance_maximum_keeps_the_last_n_instances_of_every_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let speeds: Vec<(i32, u32)> = (1..=45_u16).map(|minute| (i32::from(minute), u32::from(minute))).collect();
    let history = series(&namespace, "1", &speeds, &[(100, 0)])?;
    let series_id = namespace.urn("PagedSeries", "1")?;
    let series_type = namespace.entity_type("PagedSeries")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    broker.temporal().create_or_update(&history).send().await?.created()?;
    let _cleanup = Cleanup::of(&broker).entity(&series_id);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .temporal_query(
            QueryDocument::temporal_builder(temporal_query::after(stamp(0)?).last_n(LAST_N).build()?)
                .entities([series_type.as_str()])
                .build()?,
        )
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let detail = await_query_result(&broker, &snapshot_id, SettledStatus::Success, SnapshotQuery::Temporal(0)).await?;
    expect_that!(detail.result_status, eq(ExecutionStatus::Success));
    expect_that!(detail.problem_details, none());
    let aggregated = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .temporal()
        .retrieve_aggregated(&series_id)
        .temporal_q(temporal_query::after(stamp(0)?).aggregation_methods([AggregationMethod::TotalCount, AggregationMethod::Min, AggregationMethod::Max]))
        .send()
        .await?;
    let evolution = aggregated.body();
    expect_that!(evolution, has_aggregate("speed", AggregationMethod::TotalCount, nth(0, period().value(40))));
    expect_that!(evolution, has_aggregate("speed", AggregationMethod::Min, nth(0, period().value(6))));
    expect_that!(evolution, has_aggregate("speed", AggregationMethod::Max, nth(0, period().value(45))));
    expect_that!(evolution, has_aggregate("fuel", AggregationMethod::TotalCount, nth(0, period().value(1))));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
