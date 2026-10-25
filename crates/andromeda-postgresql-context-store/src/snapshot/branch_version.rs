//! Which Snapshots' branch schemas carry the binary's current branch version.
//!
//! A Snapshot whose `branch_schema_version` is behind the highest branch
//! migration this binary embeds has not been upgraded, or failed to upgrade,
//! and is quarantined: it is refused for scoping, clone and notification
//! delivery, and is still reaped by expiry, deletion and quota eviction.

use crate::{
    error::ContextStoreError,
    migration_set::branch::{BranchSchemaVersion, current_branch_version},
    store_fault::persistence_error,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_schema::snapshot::{SnapshotPreparation, Snapshots};
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_snapshot::{branch_condition::BranchCondition, error::SnapshotError};
use sea_query::{Expr, ExprTrait, JoinType, Query, SelectStatement, SimpleExpr};
use sqlx::PgPool;

/// `snapshot_preparation.branch_schema_version >= current`: the Snapshot's
/// branch schemas carry every branch migration this binary embeds.
pub(crate) fn branch_is_current(current: BranchSchemaVersion) -> SimpleExpr {
    Expr::col((SnapshotPreparation::Table, SnapshotPreparation::BranchSchemaVersion)).gte(current.get())
}

/// The binary's current branch version, as a Snapshot-policy failure.
pub(crate) fn current_version() -> Result<BranchSchemaVersion, SnapshotError> {
    current_branch_version().map_err(|error| persistence_error::<SnapshotError>(ContextStoreError::BranchVersion(error)))
}

/// The branch schema version one Snapshot URI carries.
fn branch_version_statement(id: &Identifier) -> SelectStatement {
    Query::select()
        .column((SnapshotPreparation::Table, SnapshotPreparation::BranchSchemaVersion))
        .from(Snapshots::Table)
        .join(
            JoinType::Join,
            SnapshotPreparation::Table,
            Expr::col((SnapshotPreparation::Table, SnapshotPreparation::SnapshotKey)).equals((Snapshots::Table, Snapshots::SnapshotKey)),
        )
        .and_where(Expr::col((Snapshots::Table, Snapshots::Id)).eq(id.as_str()))
        .to_owned()
}

/// Whether one Snapshot's branch schemas are current or quarantined; `None`
/// when no such Snapshot exists.
pub(crate) async fn read_branch_condition(pool: &PgPool, id: &Identifier) -> Result<Option<BranchCondition>, SnapshotError> {
    let current = current_version()?;
    let stored = branch_version_statement(id)
        .to_query_scalar::<i32>()
        .fetch_optional(pool)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    Ok(stored.map(|version| condition_of(version, current)))
}

/// The condition a stored branch version is in against the current one.
pub(crate) const fn condition_of(stored: i32, current: BranchSchemaVersion) -> BranchCondition {
    if stored >= current.get() {
        BranchCondition::Current
    } else {
        BranchCondition::Quarantined
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        migration_set::branch::BranchSchemaVersion,
        snapshot::branch_version::{branch_is_current, branch_version_statement, condition_of},
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_snapshot::branch_condition::BranchCondition;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    fn version(value: i64) -> BranchSchemaVersion {
        BranchSchemaVersion::try_from(value).expect("a positive version")
    }

    #[test]
    fn a_branch_behind_the_binary_is_quarantined_and_one_at_it_is_current() {
        assert_eq!(condition_of(20, version(21)), BranchCondition::Quarantined);
        assert_eq!(condition_of(21, version(21)), BranchCondition::Current);
    }

    #[test]
    fn the_current_predicate_compares_the_preparation_rows_branch_version() {
        let sql = Query::select().expr(branch_is_current(version(21))).to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT \"snapshot_preparation\".\"branch_schema_version\" >= 21");
    }

    #[test]
    fn the_version_read_joins_the_control_record_by_uri() {
        let id: Identifier = "urn:ngsi-ld:Snapshot:one".parse().expect("a URI");
        let sql = branch_version_statement(&id).to_string(PostgresQueryBuilder);
        assert!(sql.contains("JOIN \"snapshot_preparation\""), "{sql}");
        assert!(sql.contains("\"snapshots\".\"id\" = 'urn:ngsi-ld:Snapshot:one'"), "{sql}");
    }
}
