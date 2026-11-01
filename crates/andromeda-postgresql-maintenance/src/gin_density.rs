//! The density signal of a GIN that no canary B-tree stands in for.
//!
//! `pgstattuple` reads no GIN index. A density-measured GIN is judged by its
//! bytes per covered row instead, read from the catalogs only: its size, and
//! the rows of its table holding a non-null value in the column it covers
//! (`pg_class.reltuples` times one minus that column's `pg_stats.null_frac`),
//! kept as the GIN's analyzed rows so a vacuum's extrapolated `reltuples` never
//! enters a density. The density is compared with a baseline recorded on the
//! GIN's first observation and after each rebuild.
//!
//! Density is not normalised by value width: `pg_stats.avg_width` is the stored
//! width, compressed or a TOAST pointer, and says nothing about how many keys a
//! value yields. A shift towards values with more keys therefore reads as
//! degradation. The reclaim floor bounds that, and so does the verdict on the
//! rebuild: one reclaiming less than the floor is ineffective, and its density
//! becomes the new baseline.

use crate::{
    analyzed_rows::{RowCountReading, project_maintenance_instants},
    branch::Branch,
    candidate::index_target::{DensityColumn, IndexTarget},
    catalog_counter::{CatalogCounter, percent},
    error::MaintenanceError,
    inspection::IndexSize,
    policy::{DegradationSample, PolicyProfile},
};
use andromeda_postgresql_seaquery::{cast::cast_compound, pg_type::PostgresType, statement::Statement};
use sea_query::{Alias, Expr, ExprTrait, Iden, JoinType, Query, SelectStatement};
use sqlx::{PgConnection, Row};
use std::{num::NonZeroU64, str::FromStr};

/// The alias of the statistics row of the GIN's heap.
const TABLE_STATISTICS: &str = "t";

/// A density is held in thousandths of a byte per covered row, so a GIN of a
/// few bytes per row keeps three significant digits without a float.
const MILLIBYTES_PER_BYTE: u64 = 1_000;

/// A GIN's bytes per covered row, in thousandths of a byte, never zero.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Density(NonZeroU64);

impl Density {
    /// A density from its stored thousandths of a byte per row.
    pub(crate) const fn from_millibytes(millibytes_per_row: NonZeroU64) -> Density {
        Density(millibytes_per_row)
    }

    /// Thousandths of a byte per covered row.
    pub(crate) const fn millibytes_per_row(self) -> u64 {
        self.0.get()
    }
}

/// How many rows one density-measured GIN covers, read from the catalogs now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GinCoverageReading {
    /// The GIN.
    pub(crate) target: IndexTarget,
    /// Rows holding a non-null value in the column the GIN is partial on, with
    /// its table's latest analyze and vacuum.
    pub(crate) covered: RowCountReading,
}

/// How many rows one density-measured GIN covers, as its table's latest
/// analyze counted them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GinCoverage {
    /// The GIN.
    pub(crate) target: IndexTarget,
    /// Rows holding a non-null value in the column the GIN is partial on.
    pub(crate) covered_rows: u64,
}

/// One density-measured GIN's size and covered rows, read in the same pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DensityReading {
    /// The GIN.
    pub(crate) target: IndexTarget,
    /// The GIN's physical bytes.
    pub(crate) index_bytes: u64,
    /// Rows the GIN covers.
    pub(crate) covered_rows: u64,
}

impl DensityReading {
    /// The GIN's current density, `None` when it covers no row or rounds to
    /// zero.
    pub(crate) fn density(self) -> Option<Density> {
        let millibytes = self.index_bytes.saturating_mul(MILLIBYTES_PER_BYTE).checked_div(self.covered_rows)?;
        NonZeroU64::new(millibytes).map(Density)
    }

    /// The reading against `baseline` as a degradation sample: the share of
    /// the current density above the baseline, and the bytes above the size
    /// the covered rows take at the baseline. `None` without a density.
    pub(crate) fn sample(self, baseline: Density, profile: PolicyProfile) -> Option<DegradationSample> {
        let current = self.density()?.millibytes_per_row();
        let baseline_bytes = baseline.millibytes_per_row().saturating_mul(self.covered_rows) / MILLIBYTES_PER_BYTE;
        Some(DegradationSample {
            relation_bytes: self.index_bytes,
            reclaimable_bytes: self.index_bytes.saturating_sub(baseline_bytes),
            degraded_percent: percent(current.saturating_sub(baseline.millibytes_per_row()), current),
            threshold_percent: profile.index_free_percent,
        })
    }
}

/// Pair each density-measured GIN's size with its coverage; a GIN missing
/// from either has no reading.
pub(crate) fn density_readings<'pass>(sizes: &'pass [IndexSize], coverage: &'pass [GinCoverage]) -> impl Iterator<Item = DensityReading> + 'pass {
    coverage.iter().filter_map(|covered| {
        sizes.iter().find(|size| size.target == covered.target).map(|size| DensityReading {
            target: covered.target,
            index_bytes: size.bytes,
            covered_rows: covered.covered_rows,
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
                target: column.gin(),
                covered: RowCountReading::of(row.try_get(CoverageColumn::CoveredRows.to_string().as_str())?, CatalogCounter::CoveredRows, row)?,
            })
        })
        .collect()
}

/// The column the coverage statement projects besides `attname`.
#[derive(Clone, Copy, Iden)]
enum CoverageColumn {
    /// `reltuples * (1 - null_frac)`, rounded to a whole row.
    CoveredRows,
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
        candidate::{MaintenanceTarget, index_target::IndexTarget},
        gin_density::{Density, DensityReading, gin_coverage_statement},
        policy::PolicyProfile,
    };
    use andromeda_configuration::schema::database::maintenance::MaintenanceProfile;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;
    use std::num::NonZeroU64;

    const MIB: u64 = 1_048_576;

    fn responsive() -> PolicyProfile {
        PolicyProfile::from_profile(MaintenanceProfile::Responsive)
    }

    fn density(millibytes_per_row: u64) -> Density {
        Density::from_millibytes(NonZeroU64::new(millibytes_per_row).expect("a test density is positive"))
    }

    /// A reading of 1,000 covered rows whose density is `millibytes_per_row`.
    const fn reading(millibytes_per_row: u64) -> DensityReading {
        DensityReading {
            target: IndexTarget::IdxCaValueObject,
            index_bytes: millibytes_per_row,
            covered_rows: 1_000,
        }
    }

    /// `100 * (1 - baseline / current)`: 2.0 against 4.0 bytes per row is 50.
    #[test]
    fn density_degradation_follows_the_baseline() {
        let baseline = density(2_000);
        let degraded = |current| reading(current).sample(baseline, responsive()).map(|sample| sample.degraded_percent);
        assert_eq!(degraded(4_000), Some(50));
        assert_eq!(degraded(2_000), Some(0));
        assert_eq!(degraded(1_500), Some(0), "a GIN denser than its baseline is not degraded");
    }

    /// 400 MiB is 419,430,400 bytes; 1,000,000 rows at 200 bytes each take
    /// 200,000,000.
    #[test]
    fn density_reclaim_is_bytes_above_the_baseline_layout() {
        let reading = DensityReading {
            target: IndexTarget::IdxCaValueObject,
            index_bytes: 400 * MIB,
            covered_rows: 1_000_000,
        };
        let sample = reading.sample(density(200_000), responsive()).expect("the GIN covers rows");
        assert_eq!(sample.reclaimable_bytes, 219_430_400);
        assert_eq!(sample.relation_bytes, 419_430_400);
        assert_eq!(sample.threshold_percent, responsive().index_free_percent);
    }

    /// The rebuild reclaims 1 MiB against the responsive 16 MiB floor.
    #[test]
    fn low_reclaim_rebuild_resets_the_baseline() {
        let target = IndexTarget::IdxCaValueLongTextTrgm;
        let comparison = ActionComparison::Index {
            before_bytes: 101 * MIB,
            after_bytes: 100 * MIB,
        };
        assert_eq!(
            action_verdict(responsive(), MaintenanceTarget::Index(target), comparison),
            ActionVerdict::Ineffective
        );
        let rebuilt = DensityReading {
            target,
            index_bytes: 100 * MIB,
            covered_rows: 50_000,
        };
        let baseline = rebuilt.density().expect("the rebuilt GIN covers rows");
        assert_eq!(baseline, density(2_097_152));
        let sample = rebuilt.sample(baseline, responsive()).expect("the rebuilt GIN covers rows");
        assert_eq!((sample.degraded_percent, sample.reclaimable_bytes), (0, 0));
        assert_eq!(
            action_verdict(responsive(), MaintenanceTarget::Index(IndexTarget::IdxCaValueNumber), comparison),
            ActionVerdict::Effective,
            "a probed index is rebuilt on measured free space, so its rebuild is never judged by its reclaim"
        );
    }

    /// 49,496,064 bytes over 850,000 stored rows is 58.230 bytes per row;
    /// `100 * (58,230 - 31,900) / 58,230` is 45.2.
    #[test]
    fn a_density_over_the_stored_covered_rows_reads_its_degradation_against_the_baseline() {
        let reading = DensityReading {
            target: IndexTarget::IdxCaSubEqKeys,
            index_bytes: 49_496_064,
            covered_rows: 850_000,
        };
        assert_eq!(reading.density(), Some(density(58_230)));
        let sample = reading.sample(density(31_900), responsive()).expect("the GIN covers rows");
        assert_eq!(sample.degraded_percent, 45);
        assert_eq!(sample.reclaimable_bytes, 22_381_064);
    }

    /// A `null_frac` of 1.0 leaves no covered row.
    #[test]
    fn zero_covered_rows_yields_no_sample() {
        let reading = DensityReading {
            target: IndexTarget::IdxCaSubEqKeys,
            index_bytes: 64 * MIB,
            covered_rows: 0,
        };
        assert_eq!(reading.density(), None);
        assert_eq!(reading.sample(density(2_000), responsive()), None);
    }

    #[test]
    fn density_statement_reads_null_frac_reltuples_and_the_latest_analyze_and_vacuum() {
        let sql = gin_coverage_statement(Branch::Live).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""n"."nspname" = 'public'"#), "{sql}");
        assert!(sql.contains(r#"("c"."reltuples" * (1 - "s"."null_frac"))::bigint AS "covered_rows""#), "{sql}");
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
