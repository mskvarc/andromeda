//! Copying one Snapshot branch schema into another (clone preparation).
//!
//! A clone freezes the source Snapshot's published branch into the target
//! Snapshot's staging branch. Because each branch is its own schema with its own
//! identity sequences, a row copies verbatim -- the same `entity_key`,
//! `stream_key` and `subscription_key` land in the target -- so there is no
//! surrogate-key remap and no join: each branch relation is one
//! `INSERT ... SELECT` between the two schemas, in foreign-key-dependency order.
//! An identity column is copied through `OVERRIDING SYSTEM VALUE`; a stored
//! generated column is left out of the column list for the database to
//! recompute. The insertable column set is read from the catalogue so a schema
//! change moves the copy with it rather than leaving a stale column list behind.

use crate::{error::ContextStoreError, snapshot::branch_schema::BRANCH_TABLES};
use sqlx::{Executor, PgConnection, Row};

/// Copy every branch relation from `source_schema` into `target_schema`.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when reading a relation's columns or executing
/// one of the copies fails.
pub(crate) async fn clone_branch(conn: &mut PgConnection, source_schema: &str, target_schema: &str) -> Result<(), ContextStoreError> {
    for table in BRANCH_TABLES {
        let plan = column_plan(conn, table).await?;
        if plan.columns.is_empty() {
            continue;
        }
        let columns = plan.columns.join(", ");
        let overriding = if plan.has_always_identity { " OVERRIDING SYSTEM VALUE" } else { "" };
        let sql = format!("INSERT INTO {target_schema}.{table} ({columns}){overriding} SELECT {columns} FROM {source_schema}.{table}");
        conn.execute(sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))).await?;
    }
    Ok(())
}

/// The insertable columns of one branch relation and whether it carries a
/// database-generated identity column that a verbatim copy must override.
struct ColumnPlan {
    /// The non-generated columns, quoted, in ordinal order.
    columns: Vec<String>,
    /// Whether any column is `GENERATED ALWAYS AS IDENTITY`.
    has_always_identity: bool,
}

/// Read one relation's insertable columns from the catalogue, excluding stored
/// generated columns (which the database recomputes on insert).
async fn column_plan(conn: &mut PgConnection, table: &str) -> Result<ColumnPlan, ContextStoreError> {
    let rows = sqlx::query(
        "SELECT column_name, is_identity, identity_generation \
         FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = $1 AND is_generated = 'NEVER' \
         ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(&mut *conn)
    .await?;
    let mut columns = Vec::with_capacity(rows.len());
    let mut has_always_identity = false;
    for row in &rows {
        let name: String = row.try_get("column_name")?;
        let is_identity: String = row.try_get("is_identity")?;
        let identity_generation: Option<String> = row.try_get("identity_generation")?;
        if is_identity == "YES" && identity_generation.as_deref() == Some("ALWAYS") {
            has_always_identity = true;
        }
        columns.push(format!("\"{name}\""));
    }
    Ok(ColumnPlan { columns, has_always_identity })
}
