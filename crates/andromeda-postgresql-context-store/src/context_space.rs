//! Resolution of role-neutral context-space records.

use andromeda_context_space::{
    context_space::{ContextSpace, ContextSpaceKind},
    context_space_key::ContextSpaceKey,
    context_target::ContextTarget,
    snapshot_target::SnapshotTarget,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
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
pub async fn resolve_live_target(pool: &PgPool) -> Result<ContextTarget, ContextSpacePersistenceError> {
    let row = sqlx::query("SELECT context_space_key FROM context_spaces WHERE kind = 'live'::context_space_kind")
        .fetch_one(pool)
        .await?;
    let raw = row.try_get::<i64, _>("context_space_key")?;
    let key = ContextSpaceKey::try_from(raw).map_err(|error| ContextSpacePersistenceError::InvalidKey(error.to_string()))?;
    ContextTarget::live(ContextSpace::new(key, ContextSpaceKind::Live)).map_err(|error| ContextSpacePersistenceError::InvalidTarget(error.to_string()))
}

/// Resolve any persisted branch by its durable key, including the owning Snapshot URI.
pub async fn resolve_context_target(pool: &PgPool, key: ContextSpaceKey) -> Result<ContextTarget, ContextSpacePersistenceError> {
    let row = sqlx::query(
        "SELECT cs.kind::text AS kind, s.id AS snapshot_id FROM context_spaces cs LEFT JOIN snapshots s USING (context_space_key) WHERE cs.context_space_key = $1",
    )
    .bind(key.as_i64())
    .fetch_one(pool)
    .await?;
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
