//! A Snapshot as v1.0.0 stored it: its control record and preparation row in
//! `public`, and its branch schema built by the branch migrations v1.0.0
//! sealed.

use crate::fixtures::{
    statements::{insert, insert_returning, instant, json_document, name, smallint},
    test_error::UpgradeTestError,
};
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType, statement::Statement};
use andromeda_upgrade_tests::{
    release_schema::{V1_0_0_LAST_BRANCH_MIGRATION, V1_0_0_LAST_MIGRATION},
    upgrade_database::UpgradeDatabase,
};
use sea_query::{Expr, ExprTrait, Order, Query};
use serde_json::json;
use sqlx::PgPool;

/// When every seeded Snapshot was created.
const CREATED_AT: &str = "2025-01-27T21:00:00Z";

/// When every seeded Snapshot expires: long after any rehearsal ends.
const EXPIRES_AT: &str = "2125-01-27T21:00:00Z";

/// The stored status of a Snapshot whose preparation succeeded.
const SUCCESS: i16 = 2;

/// The stored status of a Snapshot still preparing.
const PREPARING: i16 = 1;

/// Writes the Snapshot `id` as v1.0.0 stored it, published when `branch` is
/// the published branch and still preparing otherwise, builds that branch
/// schema at the branch migrations v1.0.0 sealed, and returns its key.
pub(crate) async fn insert_v1_0_0_snapshot(database: &UpgradeDatabase, id: &str, branch: SnapshotBranch) -> Result<SnapshotKey, UpgradeTestError> {
    let raw_key = insert_returning::<i64>(
        database.pool(),
        "snapshots",
        "snapshot_key",
        vec![
            ("id", Expr::val(id)),
            ("created_at", instant(CREATED_AT)),
            ("expires_at", instant(EXPIRES_AT)),
            ("query_context", json_document(&json!({}))),
        ],
    )
    .await?;
    let status = match branch {
        SnapshotBranch::Published => SUCCESS,
        SnapshotBranch::Staging => PREPARING,
    };
    insert(
        database.pool(),
        "snapshot_preparation",
        vec![
            ("snapshot_key", Expr::val(raw_key)),
            ("status", smallint(status)),
            ("branch_schema_version", Expr::val(V1_0_0_LAST_BRANCH_MIGRATION.get())),
        ],
    )
    .await?;
    let key = SnapshotKey::try_from(raw_key)?;
    database.build_branch_schema(key, branch, V1_0_0_LAST_MIGRATION).await?;
    Ok(key)
}

/// The key of the Snapshot `id`.
pub(crate) async fn snapshot_key_of(pool: &PgPool, id: &str) -> Result<SnapshotKey, UpgradeTestError> {
    let select = Query::select()
        .column(name("snapshot_key"))
        .from(name("snapshots"))
        .and_where(Expr::col(name("id")).eq(id))
        .to_owned();
    Ok(SnapshotKey::try_from(select.to_query_scalar::<i64>().fetch_one(pool).await?)?)
}

/// The columns of `snapshot_preparation`, in their order.
pub(crate) async fn snapshot_preparation_columns(pool: &PgPool) -> Result<Vec<String>, UpgradeTestError> {
    let select = Query::select()
        .expr(cast(Expr::col(name("column_name")), PostgresType::Text))
        .from((name("information_schema"), name("columns")))
        .and_where(Expr::col(name("table_schema")).eq("public"))
        .and_where(Expr::col(name("table_name")).eq("snapshot_preparation"))
        .order_by(name("ordinal_position"), Order::Asc)
        .to_owned();
    Ok(select.to_query_scalar::<String>().fetch_all(pool).await?)
}
