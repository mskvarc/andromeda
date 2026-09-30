//! Auditing `attribute_history.unit_code_id` against the unit-code catalog.
//!
//! `attribute_history` is a hypertable, and a foreign key on a hypertable costs
//! a lookup per inserted row on the broker's hottest write path, so the column
//! carries none. Every write goes through this crate, which resolves the code
//! against the compiled-in catalog before binding it, so the referent is
//! established in Rust rather than by the database. This audit is the check that
//! the arrangement held: it names any stored identity with no catalog row, which
//! is what a hand-written row or a catalog regression would leave behind.

use crate::{error::TemporalStoreError, repo::TemporalRepository};
use andromeda_postgresql_schema::{temporal::AttributeHistory, unit::UnitCodes};
use andromeda_postgresql_seaquery::aggregate::count_all;
use sea_query::{Expr, ExprTrait, Iden, JoinType, Order, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, Row};

/// The `attribute_history` alias the audit reads stored identities under.
#[derive(Iden, Clone, Copy)]
#[iden = "ah"]
struct Ah;

/// The `unit_codes` alias the audit left-joins to find missing registrations.
#[derive(Iden, Clone, Copy)]
#[iden = "uc"]
struct Uc;

/// The count column the audit reports per unregistered identity.
#[derive(Iden, Clone, Copy)]
#[iden = "row_count"]
struct RowCount;

/// One stored unit-code identity that names no catalog registration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnregisteredUnitCodeRows {
    /// The identity exactly as stored, which by definition is not one the
    /// catalog assigns and therefore is not a [`UnitCodeId`].
    ///
    /// [`UnitCodeId`]: andromeda_unit_catalog::unit_code_id::UnitCodeId
    pub stored_identity: i16,
    /// How many history rows carry it.
    pub rows: i64,
}

impl TemporalRepository {
    /// Report history rows whose `unit_code_id` has no `unit_codes` row.
    ///
    /// An empty result is the expected outcome. Any row returned means the
    /// hypertable holds a unit nothing can decode, so the affected attributes
    /// render without their unit until the catalog registers the identity again.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the audit statement fails
    /// and when a returned column does not carry the expected type.
    pub async fn audit_unregistered_unit_codes(&self) -> Result<Vec<UnregisteredUnitCodeRows>, TemporalStoreError> {
        let (sql, values) = audit_statement().build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(UnregisteredUnitCodeRows {
                    stored_identity: row.try_get("unit_code_id")?,
                    rows: row.try_get("row_count")?,
                })
            })
            .collect()
    }
}

/// Audit statement replacing the foreign key the hypertable does not carry.
///
/// A left join to the catalog with `uc.id IS NULL` keeps exactly the stored
/// identities that have no registration, counted per identity.
fn audit_statement() -> SelectStatement {
    Query::select()
        .column((Ah, AttributeHistory::UnitCodeId))
        .expr_as(count_all(), RowCount)
        .from_as(AttributeHistory::Table, Ah)
        .join_as(
            JoinType::LeftJoin,
            UnitCodes::Table,
            Uc,
            Expr::col((Uc, UnitCodes::Id)).equals((Ah, AttributeHistory::UnitCodeId)),
        )
        .and_where(Expr::col((Ah, AttributeHistory::UnitCodeId)).is_not_null())
        .and_where(Expr::col((Uc, UnitCodes::Id)).is_null())
        .group_by_col((Ah, AttributeHistory::UnitCodeId))
        .order_by((Ah, AttributeHistory::UnitCodeId), Order::Asc)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::unit_code_audit::audit_statement;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_audit_reports_only_identities_the_catalog_is_missing() {
        let sql = audit_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"LEFT JOIN "unit_codes" AS "uc" ON "uc"."id" = "ah"."unit_code_id""#), "{sql}");
        assert!(sql.contains(r#"WHERE "ah"."unit_code_id" IS NOT NULL AND "uc"."id" IS NULL"#), "{sql}");
        assert!(sql.contains(r#"COUNT(*) AS "row_count""#), "{sql}");
        assert!(sql.contains(r#"GROUP BY "ah"."unit_code_id""#), "{sql}");
    }
}
