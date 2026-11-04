//! The density signal of a GIN that no canary B-tree stands in for.
//!
//! `pgstattuple` reads no GIN index. A density-measured GIN is judged by its
//! size against the size its covered rows take at its keys per row and its
//! bytes per key, read from the catalogs and the platform state only: its
//! size, and the rows of its table holding a non-null value in the column it
//! covers (`pg_class.reltuples` times one minus that column's
//! `pg_stats.null_frac`), kept as the GIN's analyzed rows so a vacuum's
//! extrapolated `reltuples` never enters the comparison. The keys per row and
//! the bytes per key, and why a GIN is judged by them, are described with
//! their sampling and their defaults.

use crate::{
    analyzed_rows::{RowCountReading, project_maintenance_instants},
    branch::Branch,
    candidate::index_target::DensityColumn,
    catalog_counter::{CatalogCounter, non_negative, percent},
    error::MaintenanceError,
    gin_keys::{KeyBytes, KeysPerRow},
    inspection::IndexSize,
    policy::{DegradationSample, PolicyProfile},
};
use andromeda_postgresql_seaquery::{cast::cast_compound, pg_type::PostgresType, statement::Statement};
use sea_query::{Alias, Expr, ExprTrait, Iden, JoinType, Query, SelectStatement};
use sqlx::{PgConnection, Row};
use std::{num::NonZeroU64, str::FromStr};

/// The alias of the statistics row of the GIN's heap.
const TABLE_STATISTICS: &str = "t";

/// Keys per row and bytes per key are both thousandths, so their product over
/// the covered rows is in millionths of a byte.
const MILLIONTHS: u128 = 1_000_000;

/// How many rows one density-measured GIN covers, read from the catalogs now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GinCoverageReading {
    /// The column the GIN covers.
    pub(crate) column: DensityColumn,
    /// Rows holding a non-null value in the column the GIN is partial on, with
    /// its table's latest analyze and vacuum.
    pub(crate) covered: RowCountReading,
    /// Pages of the GIN's table, from `pg_class.relpages`, which bound a
    /// sample of its keys.
    pub(crate) heap_pages: u64,
}

/// How many rows one density-measured GIN covers, as its table's latest
/// analyze counted them, and the keys it holds for each.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GinCoverage {
    /// The column the GIN covers.
    pub(crate) column: DensityColumn,
    /// Rows holding a non-null value in the column the GIN is partial on.
    pub(crate) covered_rows: u64,
    /// Keys the GIN holds per covered row.
    pub(crate) keys_per_row: KeysPerRow,
}

/// One density-measured GIN's size and coverage, read in the same pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DensityReading {
    /// The column the GIN covers.
    pub(crate) column: DensityColumn,
    /// The GIN's physical bytes.
    pub(crate) index_bytes: u64,
    /// Rows the GIN covers.
    pub(crate) covered_rows: u64,
    /// Keys the GIN holds per covered row.
    pub(crate) keys_per_row: KeysPerRow,
}

impl DensityReading {
    /// The bytes the covered rows take at `key_bytes` per key.
    fn expected_bytes(self, key_bytes: KeyBytes) -> u64 {
        let millionths = u128::from(self.covered_rows) * u128::from(self.keys_per_row.thousandths()) * u128::from(key_bytes.thousandths());
        u64::try_from(millionths / MILLIONTHS).unwrap_or(u64::MAX)
    }

    /// The reading as a degradation sample against the size its covered rows
    /// take at `key_bytes` per key: the share of the GIN's size above that
    /// size, and the bytes above it. `None` when the GIN covers no row.
    pub(crate) fn sample(self, key_bytes: KeyBytes, profile: PolicyProfile) -> Option<DegradationSample> {
        if self.covered_rows == 0 {
            return None;
        }
        let reclaimable_bytes = self.index_bytes.saturating_sub(self.expected_bytes(key_bytes));
        Some(DegradationSample {
            relation_bytes: self.index_bytes,
            reclaimable_bytes,
            degraded_percent: percent(reclaimable_bytes, self.index_bytes),
            threshold_percent: profile.index_free_percent,
        })
    }

    /// The bytes one key takes in the GIN as it stands, `None` when it covers
    /// no row or its keys round to none.
    pub(crate) fn key_bytes(self) -> Option<KeyBytes> {
        let keys = u128::from(self.covered_rows) * u128::from(self.keys_per_row.thousandths());
        let thousandths = (u128::from(self.index_bytes) * MILLIONTHS).checked_div(keys)?;
        NonZeroU64::new(u64::try_from(thousandths).unwrap_or(u64::MAX)).map(KeyBytes::from_thousandths)
    }
}

/// Pair each density-measured GIN's size with its coverage; a GIN missing
/// from either has no reading.
pub(crate) fn density_readings<'pass>(sizes: &'pass [IndexSize], coverage: &'pass [GinCoverage]) -> impl Iterator<Item = DensityReading> + 'pass {
    coverage.iter().filter_map(|covered| {
        sizes.iter().find(|size| size.target == covered.column.gin()).map(|size| DensityReading {
            column: covered.column,
            index_bytes: size.bytes,
            covered_rows: covered.covered_rows,
            keys_per_row: covered.keys_per_row,
        })
    })
}

/// Read how many rows each density-measured GIN of `branch` covers now.
///
/// A table never analyzed (`reltuples` of -1), or a column `pg_stats` does
/// not describe, gives its GIN no reading.
pub(crate) async fn gin_coverage_readings(connection: &mut PgConnection, branch: Branch) -> Result<Vec<GinCoverageReading>, MaintenanceError> {
    let rows = gin_coverage_statement(branch).to_query().fetch_all(connection).await?;
    rows.iter()
        .map(|row| {
            let column = DensityColumn::from_str(&row.try_get::<String, _>("attname")?).map_err(MaintenanceError::UnknownTarget)?;
            Ok(GinCoverageReading {
                column,
                covered: RowCountReading::of(row.try_get(CoverageColumn::CoveredRows.to_string().as_str())?, CatalogCounter::CoveredRows, row)?,
                heap_pages: non_negative(row.try_get(CoverageColumn::HeapPages.to_string().as_str())?, CatalogCounter::HeapPages)?,
            })
        })
        .collect()
}

/// The columns the coverage statement projects besides `attname`.
#[derive(Clone, Copy, Iden)]
enum CoverageColumn {
    /// `reltuples * (1 - null_frac)`, rounded to a whole row.
    CoveredRows,
    /// `relpages` of the heap, widened to `bigint`.
    HeapPages,
}

/// A column of one aliased catalog.
fn catalog_column(alias: &'static str, column: &'static str) -> Expr {
    Expr::col((Alias::new(alias), Alias::new(column)))
}

/// The non-null rows of each density column in `branch`'s schema, read on the
/// heap of the GIN partial on it, with that heap's latest analyze and vacuum.
fn gin_coverage_statement(branch: Branch) -> SelectStatement {
    let pairs = DensityColumn::ALL.map(|column| Expr::tuple([Expr::val(column.gin().heap().relation_name()), Expr::val(<&'static str>::from(column))]));
    let covered = catalog_column("c", "reltuples").mul(Expr::val(1).sub(catalog_column("s", "null_frac")));
    let mut statement = Query::select();
    statement
        .column((Alias::new("s"), Alias::new("attname")))
        .expr_as(cast_compound(covered, PostgresType::Bigint), CoverageColumn::CoveredRows)
        .expr_as(cast_compound(catalog_column("c", "relpages"), PostgresType::Bigint), CoverageColumn::HeapPages)
        .from_as(Alias::new("pg_class"), Alias::new("c"))
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_namespace"),
            Alias::new("n"),
            catalog_column("n", "oid").equals((Alias::new("c"), Alias::new("relnamespace"))),
        )
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_stats"),
            Alias::new("s"),
            catalog_column("s", "schemaname")
                .equals((Alias::new("n"), Alias::new("nspname")))
                .and(catalog_column("s", "tablename").equals((Alias::new("c"), Alias::new("relname"))))
                .and(catalog_column("s", "inherited").eq(false)),
        )
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_stat_user_tables"),
            Alias::new(TABLE_STATISTICS),
            catalog_column(TABLE_STATISTICS, "relid").equals((Alias::new("c"), Alias::new("oid"))),
        )
        .and_where(catalog_column("n", "nspname").eq(branch.schema_name()))
        .and_where(catalog_column("c", "reltuples").gte(0))
        .and_where(Expr::tuple([catalog_column("c", "relname"), catalog_column("s", "attname")]).is_in(pairs));
    project_maintenance_instants(&mut statement, TABLE_STATISTICS);
    statement
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        action_measurement::ActionComparison,
        action_policy::{ActionVerdict, action_verdict},
        branch::Branch,
        candidate::{MaintenanceTarget, index_target::DensityColumn},
        gin_density::{DensityReading, gin_coverage_statement},
        gin_keys::{KeyBytes, KeysPerRow, default_key_bytes},
        policy::PolicyProfile,
    };
    use andromeda_configuration::schema::database::maintenance::MaintenanceProfile;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;
    use std::num::NonZeroU64;

    const MIB: u64 = 1_048_576;

    fn conservative() -> PolicyProfile {
        PolicyProfile::from_profile(MaintenanceProfile::Conservative)
    }

    fn keys(thousandths: u64) -> KeysPerRow {
        KeysPerRow::from_thousandths(NonZeroU64::new(thousandths).expect("a test key count is positive"))
    }

    fn key_bytes(thousandths: u64) -> KeyBytes {
        KeyBytes::from_thousandths(NonZeroU64::new(thousandths).expect("a test key size is positive"))
    }

    fn identifier_default() -> KeyBytes {
        default_key_bytes(DensityColumn::Id).expect("the identifier trigram GIN has a default")
    }

    /// An identifier trigram GIN of `index_bytes` covering `covered_rows` rows
    /// of `keys_per_row` thousandths of a trigram each.
    const fn identifiers(index_bytes: u64, covered_rows: u64, keys_per_row: u64) -> DensityReading {
        DensityReading {
            column: DensityColumn::Id,
            index_bytes,
            covered_rows,
            keys_per_row: KeysPerRow::from_thousandths(NonZeroU64::MIN.saturating_add(keys_per_row - 1)),
        }
    }

    /// 2,047,710 identifiers of 124.198 trigrams take 381,482,229 bytes at the
    /// default 1.5 bytes per trigram; the GIN holds 1,139,130,368, 66 % of
    /// them above that. Rebuilt from every row the GIN measured 455,491,584
    /// bytes, 1.791 per trigram, against which it is 60 % degraded.
    #[test]
    fn a_large_identifier_gin_reads_its_degradation_at_the_default_close_to_what_a_rebuild_measures() {
        let reading = identifiers(1_139_130_368, 2_047_710, 124_198);
        let sample = reading.sample(identifier_default(), conservative()).expect("the GIN covers rows");
        assert_eq!(sample.degraded_percent, 66);
        assert_eq!(sample.reclaimable_bytes, 757_648_139);
        assert_eq!(sample.relation_bytes, 1_139_130_368);
        assert_eq!(sample.threshold_percent, conservative().index_free_percent);
        let rebuilt = identifiers(455_491_584, 2_047_710, 124_198);
        let measured = rebuilt.key_bytes().expect("the rebuilt GIN covers rows");
        assert_eq!(measured, key_bytes(1_791));
        assert_eq!(reading.sample(measured, conservative()).map(|sample| sample.degraded_percent), Some(60));
    }

    /// 759,319 identifiers of 45.086 trigrams, 108,781,568 bytes: 52 % at the
    /// default and 49 % at the 1.612 bytes per trigram a rebuild measured. The
    /// default crosses the conservative 50 % threshold the measurement stays
    /// under, and the rebuild it queues measures the GIN.
    #[test]
    fn a_default_below_what_a_rebuild_measures_overstates_degradation_by_a_few_points() {
        let reading = identifiers(108_781_568, 759_319, 45_086);
        let at = |bytes| reading.sample(bytes, conservative()).map(|sample| sample.degraded_percent);
        assert_eq!(at(identifier_default()), Some(52));
        assert_eq!(at(key_bytes(1_612)), Some(49));
    }

    /// 938,585 identifiers of 36.478 trigrams take 51,356,555 bytes at the
    /// default; 91,471,872 bytes are 43 % above that, as they are at the
    /// 1.514 a rebuild measured.
    #[test]
    fn a_gin_under_the_threshold_at_its_rebuilt_size_reads_under_it_at_the_default() {
        let reading = identifiers(91_471_872, 938_585, 36_478);
        assert_eq!(
            reading.sample(identifier_default(), conservative()).map(|sample| sample.degraded_percent),
            Some(43)
        );
        assert_eq!(reading.sample(key_bytes(1_514), conservative()).map(|sample| sample.degraded_percent), Some(43));
    }

    /// The bytes per key are measured to a thousandth of a byte, rounded down,
    /// so the rebuilt GIN stands 1,802 bytes above the 455,489,782 its rows
    /// take at the 1.791 measured: no whole percent of it.
    #[test]
    fn a_gin_at_the_bytes_per_key_its_rebuild_measured_is_not_degraded() {
        let rebuilt = identifiers(455_491_584, 2_047_710, 124_198);
        let measured = rebuilt.key_bytes().expect("the rebuilt GIN covers rows");
        let sample = rebuilt.sample(measured, conservative()).expect("the rebuilt GIN covers rows");
        assert_eq!((sample.degraded_percent, sample.reclaimable_bytes), (0, 1_802));
    }

    #[test]
    fn values_with_more_keys_raise_the_size_a_gin_is_judged_against() {
        let before = identifiers(200 * MIB, 1_000_000, 100_000);
        let measured = before.key_bytes().expect("the GIN covers rows");
        let longer = identifiers(400 * MIB, 1_000_000, 200_000);
        assert_eq!(longer.sample(measured, conservative()).map(|sample| sample.degraded_percent), Some(0));
        let bloated = identifiers(400 * MIB, 1_000_000, 100_000);
        assert_eq!(bloated.sample(measured, conservative()).map(|sample| sample.degraded_percent), Some(50));
    }

    #[test]
    fn a_gin_smaller_than_its_covered_rows_take_is_not_degraded() {
        let reading = identifiers(10 * MIB, 1_000_000, 10_000);
        let sample = reading.sample(identifier_default(), conservative()).expect("the GIN covers rows");
        assert_eq!((sample.degraded_percent, sample.reclaimable_bytes), (0, 0));
    }

    /// A `null_frac` of 1.0 leaves no covered row.
    #[test]
    fn zero_covered_rows_yields_no_sample_and_no_bytes_per_key() {
        let reading = identifiers(64 * MIB, 0, 36_000);
        assert_eq!(reading.sample(identifier_default(), conservative()), None);
        assert_eq!(reading.key_bytes(), None);
    }

    /// The rebuild reclaims 1 MiB against the conservative 64 MiB floor, and
    /// the bytes per key it measures judge the GIN as it stands after it.
    #[test]
    fn a_low_reclaim_rebuild_is_ineffective_and_still_measures_the_gin() {
        let comparison = ActionComparison::Index {
            before_bytes: 101 * MIB,
            after_bytes: 100 * MIB,
        };
        assert_eq!(
            action_verdict(conservative(), MaintenanceTarget::Index(DensityColumn::ValueLongText.gin()), comparison),
            ActionVerdict::Ineffective
        );
        let rebuilt = DensityReading {
            column: DensityColumn::ValueLongText,
            index_bytes: 100 * MIB,
            covered_rows: 50_000,
            keys_per_row: keys(400_000),
        };
        let measured = rebuilt.key_bytes().expect("the rebuilt GIN covers rows");
        assert_eq!(measured, key_bytes(5_242));
        assert_eq!(rebuilt.sample(measured, conservative()).map(|sample| sample.degraded_percent), Some(0));
    }

    #[test]
    fn density_statement_reads_null_frac_reltuples_relpages_and_the_latest_analyze_and_vacuum() {
        let sql = gin_coverage_statement(Branch::Live).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""n"."nspname" = 'public'"#), "{sql}");
        assert!(sql.contains(r#"("c"."reltuples" * (1 - "s"."null_frac"))::bigint AS "covered_rows""#), "{sql}");
        assert!(sql.contains(r#"("c"."relpages")::bigint AS "heap_pages""#), "{sql}");
        assert!(sql.contains(r#""s"."inherited" = FALSE"#), "{sql}");
        assert!(sql.contains(r#""c"."reltuples" >= 0"#), "a table never analyzed covers nothing: {sql}");
        assert!(sql.contains(r#"INNER JOIN "pg_stat_user_tables" AS "t" ON "t"."relid" = "c"."oid""#), "{sql}");
        assert!(
            sql.contains(r#"GREATEST("t"."last_analyze", "t"."last_autoanalyze") AS "last_analyzed""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"GREATEST("t"."last_vacuum", "t"."last_autovacuum") AS "last_vacuumed""#),
            "{sql}"
        );
        assert!(
            sql.contains(
                r#"("c"."relname", "s"."attname") IN (('current_attributes', 'sub_eq_keys'), ('current_attributes', 'value_object'), ('current_attributes', 'value_long_text'), ('entities', 'id'))"#
            ),
            "{sql}"
        );
    }
}
