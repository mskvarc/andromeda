//! Resolution of role-neutral context-space records.

use andromeda_context_space::{
    context_space::{ContextSpace, ContextSpaceKind},
    context_space_key::ContextSpaceKey,
    context_target::ContextTarget,
    snapshot_target::SnapshotTarget,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_schema::{context_space::ContextSpaces, snapshot::Snapshots};
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
use sea_query::{Alias, DeleteStatement, Expr, ExprTrait, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Row};
use thiserror::Error;

/// Failure to resolve the mandatory live branch of a tenant database.
#[derive(Debug, Error)]
pub enum ContextSpacePersistenceError {
    /// PostgreSQL could not read the context-space record.
    #[error("context-space persistence failed: {0}")]
    Persistence(#[from] sqlx::Error),
    /// The database returned a non-positive surrogate key.
    #[error("invalid live context-space key: {0}")]
    InvalidKey(String),
    /// A live record could not construct a live target.
    #[error("invalid live context-space record: {0}")]
    InvalidTarget(String),
}

/// Resolve the exactly-one live context space installed by tenant migrations.
///
/// # Errors
///
/// Returns [`ContextSpacePersistenceError::Persistence`] when the lookup fails
/// or no live row exists, [`ContextSpacePersistenceError::InvalidKey`] when the
/// stored surrogate key is not positive, and
/// [`ContextSpacePersistenceError::InvalidTarget`] when the record cannot form a
/// live target.
pub async fn resolve_live_target(pool: &PgPool) -> Result<ContextTarget, ContextSpacePersistenceError> {
    let statement = Query::select()
        .column(ContextSpaces::ContextSpaceKey)
        .from(ContextSpaces::Table)
        .and_where(Expr::col(ContextSpaces::Kind).eq(cast(Expr::val("live"), PostgresType::ContextSpaceKind)))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(pool).await?;
    let raw = row.try_get::<i64, _>("context_space_key")?;
    let key = ContextSpaceKey::try_from(raw).map_err(|error| ContextSpacePersistenceError::InvalidKey(error.to_string()))?;
    ContextTarget::live(ContextSpace::new(key, ContextSpaceKind::Live)).map_err(|error| ContextSpacePersistenceError::InvalidTarget(error.to_string()))
}

/// Resolve any persisted branch by its durable key, including the owning Snapshot URI.
///
/// # Errors
///
/// Returns [`ContextSpacePersistenceError::Persistence`] when the lookup fails
/// or the key names no row, and
/// [`ContextSpacePersistenceError::InvalidTarget`] when the stored kind is
/// unknown, when a snapshot branch carries no owning Snapshot URI, or when that
/// URI does not parse.
pub async fn resolve_context_target(pool: &PgPool, key: ContextSpaceKey) -> Result<ContextTarget, ContextSpacePersistenceError> {
    let kind = Alias::new("kind");
    let snapshot_id = Alias::new("snapshot_id");
    let statement = Query::select()
        .expr_as(cast(Expr::col((ContextSpaces::Table, ContextSpaces::Kind)), PostgresType::Text), kind)
        .expr_as(Expr::col((Snapshots::Table, Snapshots::Id)), snapshot_id)
        .from(ContextSpaces::Table)
        .left_join(
            Snapshots::Table,
            Expr::col((Snapshots::Table, Snapshots::ContextSpaceKey)).equals((ContextSpaces::Table, ContextSpaces::ContextSpaceKey)),
        )
        .and_where(Expr::col((ContextSpaces::Table, ContextSpaces::ContextSpaceKey)).eq(key.as_i64()))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(pool).await?;
    let kind = row.try_get::<String, _>("kind")?;
    let space = match kind.as_str() {
        "live" => ContextSpace::new(key, ContextSpaceKind::Live),
        "snapshot" => ContextSpace::new(key, ContextSpaceKind::Snapshot),
        _ => return Err(ContextSpacePersistenceError::InvalidTarget(format!("unknown context-space kind `{kind}`"))),
    };
    match space.kind() {
        ContextSpaceKind::Live => ContextTarget::live(space).map_err(|error| ContextSpacePersistenceError::InvalidTarget(error.to_string())),
        ContextSpaceKind::Snapshot => {
            let raw = row
                .try_get::<Option<String>, _>("snapshot_id")?
                .ok_or_else(|| ContextSpacePersistenceError::InvalidTarget("Snapshot context has no owning Snapshot URI".to_owned()))?;
            let snapshot_id = raw
                .parse::<Uri>()
                .map_err(|error| ContextSpacePersistenceError::InvalidTarget(error.to_string()))?;
            SnapshotTarget::new(space, snapshot_id)
                .map(ContextTarget::Snapshot)
                .map_err(|error| ContextSpacePersistenceError::InvalidTarget(error.to_string()))
        }
    }
}

/// Build deletion of one branch parent so database cascades reclaim all owned state.
pub(crate) fn delete_context_space_statement(key: i64) -> DeleteStatement {
    Query::delete()
        .from_table(ContextSpaces::Table)
        .and_where(Expr::col(ContextSpaces::ContextSpaceKey).eq(key))
        .to_owned()
}
