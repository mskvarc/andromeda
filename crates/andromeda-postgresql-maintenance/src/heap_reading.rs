//! The dead share of each allowlisted heap, from cheap statistics.
//!
//! The dead tuples are `n_dead_tup`. The live rows they are weighed against
//! are the heap's analyzed rows, never the `n_live_tup` of the moment: a
//! vacuum overwrites that with its own extrapolation, the same figure it
//! writes into `reltuples`.

use crate::{
    action_measurement::TupleCounts,
    analyzed_rows::{AnalyzedRows, RowCountReading, project_maintenance_instants},
    autovacuum_trigger::{TABLE_STATISTICS, statistics_column},
    branch::Branch,
    candidate::heap_target::HeapTarget,
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
    policy::{DegradationSample, PolicyProfile},
};
use andromeda_postgresql_seaquery::{maintenance::total_relation_size, statement::Statement};
use sea_query::{Alias, ExprTrait, Iden, Query, SelectStatement};
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::str::FromStr;

/// One heap's statistics as read now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HeapReading {
    /// Compile-time heap target.
    pub(crate) target: HeapTarget,
    /// `n_dead_tup`.
    dead: u64,
    /// Total relation bytes.
    bytes: u64,
    /// `n_live_tup` with the heap's latest analyze and vacuum.
    pub(crate) live: RowCountReading,
}

impl HeapReading {
    /// The reading weighed against `live`, the heap's analyzed rows, and the
    /// profile's dead-space threshold.
    pub(crate) fn sample(self, live: AnalyzedRows, profile: PolicyProfile) -> HeapSample {
        let counts = TupleCounts {
            dead: self.dead,
            live: live.rows,
        };
        HeapSample {
            target: self.target,
            counts,
            degradation: DegradationSample {
                relation_bytes: self.bytes,
                reclaimable_bytes: self
                    .bytes
                    .saturating_mul(self.dead)
                    .checked_div(live.rows.saturating_add(self.dead))
                    .unwrap_or_default(),
                degraded_percent: counts.dead_percent().unwrap_or_default(),
                threshold_percent: profile.heap_dead_percent,
            },
        }
    }
}

/// One heap's dead share against its analyzed rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HeapSample {
    /// Compile-time heap target.
    pub(crate) target: HeapTarget,
    /// The dead tuples and the analyzed live rows the share is derived from.
    pub(crate) counts: TupleCounts,
    /// Threshold-ready degradation sample.
    pub(crate) degradation: DegradationSample,
}

/// Read every allowlisted heap `branch` holds.
pub(crate) async fn heap_readings(connection: &mut PgConnection, branch: Branch) -> Result<Vec<HeapReading>, MaintenanceError> {
    let rows = heap_readings_statement(branch).to_query().fetch_all(connection).await?;
    rows.iter().map(heap_reading_of).collect()
}

/// Read one heap in `branch`, as [`heap_readings`] reads every heap.
pub(crate) async fn heap_reading(connection: &mut PgConnection, branch: Branch, heap: HeapTarget) -> Result<HeapReading, MaintenanceError> {
    let statement = heap_readings_statement(branch)
        .and_where(statistics_column("relname").eq(heap.relation_name()))
        .to_owned();
    let row = statement.to_query().fetch_one(connection).await?;
    heap_reading_of(&row)
}

/// Read a heap's dead tuples in `branch`, compared across its vacuum.
///
/// The read runs as its own statement outside any transaction, so it never
/// sees the statistics snapshot an earlier read cached; a vacuum publishes its
/// remaining dead tuples as `n_dead_tup` when it finishes.
pub(crate) async fn heap_dead_tuples(connection: &mut PgConnection, branch: Branch, heap: HeapTarget) -> Result<u64, MaintenanceError> {
    let dead = heap_dead_tuples_statement(branch, heap).to_query_scalar::<i64>().fetch_one(connection).await?;
    non_negative(dead, CatalogCounter::DeadTuples)
}

/// The columns the heap statements project besides the maintenance instants.
#[derive(Clone, Copy, Iden)]
enum HeapColumn {
    /// The heap's name.
    Relname,
    /// `n_live_tup`.
    LiveTuples,
    /// `n_dead_tup`.
    DeadTuples,
    /// Total relation bytes.
    Bytes,
}

/// Decode one heap row.
fn heap_reading_of(row: &PgRow) -> Result<HeapReading, MaintenanceError> {
    let name = row.try_get::<String, _>(HeapColumn::Relname.to_string().as_str())?;
    Ok(HeapReading {
        target: HeapTarget::from_str(&name).map_err(MaintenanceError::UnknownTarget)?,
        dead: non_negative(row.try_get(HeapColumn::DeadTuples.to_string().as_str())?, CatalogCounter::DeadTuples)?,
        bytes: non_negative(row.try_get(HeapColumn::Bytes.to_string().as_str())?, CatalogCounter::HeapBytes)?,
        live: RowCountReading::of(row.try_get(HeapColumn::LiveTuples.to_string().as_str())?, CatalogCounter::LiveTuples, row)?,
    })
}

/// The statistics view every heap statement reads, aliased for
/// [`statistics_column`], filtered to `branch`'s schema.
fn statistics_of(branch: Branch) -> SelectStatement {
    Query::select()
        .from_as(Alias::new("pg_stat_user_tables"), Alias::new(TABLE_STATISTICS))
        .and_where(statistics_column("schemaname").eq(branch.schema_name()))
        .to_owned()
}

/// Every allowlisted heap `branch` holds with its tuples, bytes and latest
/// analyze and vacuum.
fn heap_readings_statement(branch: Branch) -> SelectStatement {
    let mut statement = statistics_of(branch);
    statement
        .expr_as(statistics_column("relname"), HeapColumn::Relname)
        .expr_as(statistics_column("n_live_tup"), HeapColumn::LiveTuples)
        .expr_as(statistics_column("n_dead_tup"), HeapColumn::DeadTuples)
        .expr_as(total_relation_size(statistics_column("relid")), HeapColumn::Bytes)
        .and_where(statistics_column("relname").is_in(branch.heaps().map(HeapTarget::relation_name)));
    project_maintenance_instants(&mut statement, TABLE_STATISTICS);
    statement
}

/// The dead tuples of one allowlisted heap of `branch`.
fn heap_dead_tuples_statement(branch: Branch, heap: HeapTarget) -> SelectStatement {
    statistics_of(branch)
        .expr(statistics_column("n_dead_tup"))
        .and_where(statistics_column("relname").eq(heap.relation_name()))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::{
        analyzed_rows::{AnalyzedRows, RowCountReading},
        branch::Branch,
        candidate::heap_target::HeapTarget,
        heap_reading::{HeapReading, heap_dead_tuples_statement, heap_readings_statement},
        policy::PolicyProfile,
    };
    use andromeda_configuration::schema::database::maintenance::MaintenanceProfile;
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn responsive() -> PolicyProfile {
        PolicyProfile::from_profile(MaintenanceProfile::Responsive)
    }

    /// 300,000 dead against 3,000,000 analyzed live rows is 9%, under the
    /// responsive 10%; against the 3,700,000 a vacuum extrapolated it would
    /// read 7%, and 400,000 dead would read 9% instead of 11%.
    #[test]
    fn the_dead_share_is_weighed_against_the_analyzed_rows_not_the_live_count_of_the_moment() {
        let reading = |dead| HeapReading {
            target: HeapTarget::CurrentAttributes,
            dead,
            bytes: 1_000_000,
            live: RowCountReading {
                rows: 3_700_000,
                last_analyzed: None,
                last_vacuumed: None,
            },
        };
        let analyzed = AnalyzedRows {
            rows: 3_000_000,
            analyzed_at: DateTime::<Utc>::UNIX_EPOCH,
        };
        assert_eq!(reading(300_000).sample(analyzed, responsive()).degradation.degraded_percent, 9);
        let sample = reading(400_000).sample(analyzed, responsive());
        assert_eq!(sample.degradation.degraded_percent, 11);
        assert_eq!(sample.degradation.reclaimable_bytes, 117_647);
        assert_eq!((sample.counts.dead, sample.counts.live), (400_000, 3_000_000));
    }

    #[test]
    fn the_heap_reading_reads_every_hot_and_queue_heap_with_its_latest_analyze_and_vacuum() {
        let sql = heap_readings_statement(Branch::Live).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM "pg_stat_user_tables" AS "s""#), "{sql}");
        assert!(sql.contains(r#""s"."n_live_tup" AS "live_tuples""#), "{sql}");
        assert!(sql.contains(r#""s"."n_dead_tup" AS "dead_tuples""#), "{sql}");
        assert!(
            sql.contains(r#"GREATEST("s"."last_analyze", "s"."last_autoanalyze") AS "last_analyzed""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"GREATEST("s"."last_vacuum", "s"."last_autovacuum") AS "last_vacuumed""#),
            "{sql}"
        );
        for target in HeapTarget::ALL {
            assert!(sql.contains(&format!("'{}'", target.relation_name())), "{target}: {sql}");
        }
    }

    #[test]
    fn the_action_dead_tuple_read_names_one_heap_and_no_live_count() {
        let sql = heap_dead_tuples_statement(Branch::Live, HeapTarget::Entities).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"SELECT "s"."n_dead_tup" FROM"#), "{sql}");
        assert!(!sql.contains("n_live_tup"), "{sql}");
        assert!(sql.contains(r#""s"."relname" = 'entities'"#), "{sql}");
    }

    #[test]
    fn a_snapshot_branch_reading_reads_its_schema_and_only_the_branch_heaps() {
        let branch = Branch::Snapshot(SnapshotKey::try_from(7).expect("positive snapshot key"));
        let heaps = heap_readings_statement(branch).to_string(PostgresQueryBuilder);
        assert!(heaps.contains(r#""s"."schemaname" = 'snap_7'"#), "{heaps}");
        for heap in HeapTarget::ALL {
            let named = heaps.contains(&format!("'{}'", heap.relation_name()));
            assert_eq!(named, branch.heaps().any(|held| held == heap), "{heap}: {heaps}");
        }
        let dead = heap_dead_tuples_statement(branch, HeapTarget::Entities).to_string(PostgresQueryBuilder);
        assert!(dead.contains(r#""s"."schemaname" = 'snap_7'"#), "{dead}");
    }
}
