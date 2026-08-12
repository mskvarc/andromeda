//! Bulk inserts into the rebuilt `scope_history` hypertable.

use crate::{
    error::TemporalStoreError,
    repo::TemporalRepository,
    scope_history_entry::ScopeHistoryEntry,
    stored_instance_columns::{generated_instance_uuid, verbatim_instance_id},
};
use andromeda_postgresql_codes::scope_value_kind::ScopeValueKind;
use andromeda_postgresql_query::ltree::scope_to_ltree;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

impl TemporalRepository {
    /// Bulk-insert scope-history observations in the caller's transaction.
    ///
    /// The narrow temporal-entity key each row is stored under is resolved by
    /// the statement itself, so a caller holding only the entity identifier does
    /// not read the catalogue first. A scope observation for an entity with no
    /// temporal catalogue entry is therefore dropped by the join rather than
    /// inserted against an invented key.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Validation`] when a row's position within
    /// its write exceeds the stored column's width, and
    /// [`TemporalStoreError::Database`] when a statement fails.
    pub async fn insert_scope_history_batch_in_tx(&self, tx: &mut Transaction<'_, Postgres>, entries: &[ScopeHistoryEntry]) -> Result<(), TemporalStoreError> {
        if entries.is_empty() {
            return Ok(());
        }
        let binds = scope_history_binds(entries)?;
        sqlx::query(SCOPE_HISTORY_DEDUP_SQL)
            .bind(&binds.entity_ids)
            .bind(&binds.generated_instance_uuids)
            .bind(&binds.verbatim_instance_ids)
            .execute(&mut **tx)
            .await?;
        sqlx::query(SCOPE_HISTORY_INSERT_SQL)
            .bind(&binds.recorded_ats)
            .bind(&binds.entity_ids)
            .bind(&binds.write_ids)
            .bind(&binds.write_ordinals)
            .bind(&binds.value_kinds)
            .bind(&binds.observed_ats)
            .bind(&binds.created_ats)
            .bind(&binds.modified_ats)
            .bind(&binds.deleted_ats)
            .bind(&binds.generated_instance_uuids)
            .bind(&binds.verbatim_instance_ids)
            .bind(&binds.scope_paths)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }
}

/// Column-major binds for one scope-history batch.
struct ScopeHistoryBinds<'a> {
    /// Write clock of each row.
    recorded_ats: Vec<DateTime<Utc>>,
    /// Entity identifier the temporal-entity key is resolved from.
    entity_ids: Vec<&'a str>,
    /// Atomic write each row belongs to.
    write_ids: Vec<Uuid>,
    /// Position of each row within its atomic write.
    write_ordinals: Vec<i32>,
    /// Stored value shape of each row.
    value_kinds: Vec<i16>,
    /// Client-declared observation instants.
    observed_ats: Vec<Option<DateTime<Utc>>>,
    /// Instants the rows were first recorded at.
    created_ats: Vec<Option<DateTime<Utc>>>,
    /// Instants the rows were last altered at.
    modified_ats: Vec<Option<DateTime<Utc>>>,
    /// Deletion instants, present only on tombstones.
    deleted_ats: Vec<Option<DateTime<Utc>>>,
    /// Broker-minted instance identifiers.
    generated_instance_uuids: Vec<Option<Uuid>>,
    /// Instance identifiers stored exactly as received.
    verbatim_instance_ids: Vec<Option<&'a str>>,
    /// Scope paths in `ltree` label notation, absent on tombstones.
    scope_paths: Vec<Option<String>>,
}

/// Spread one scope-history batch into its column-major binds.
fn scope_history_binds<'a>(entries: &'a [ScopeHistoryEntry]) -> Result<ScopeHistoryBinds<'a>, TemporalStoreError> {
    let mut binds = ScopeHistoryBinds {
        recorded_ats: Vec::with_capacity(entries.len()),
        entity_ids: Vec::with_capacity(entries.len()),
        write_ids: Vec::with_capacity(entries.len()),
        write_ordinals: Vec::with_capacity(entries.len()),
        value_kinds: Vec::with_capacity(entries.len()),
        observed_ats: Vec::with_capacity(entries.len()),
        created_ats: Vec::with_capacity(entries.len()),
        modified_ats: Vec::with_capacity(entries.len()),
        deleted_ats: Vec::with_capacity(entries.len()),
        generated_instance_uuids: Vec::with_capacity(entries.len()),
        verbatim_instance_ids: Vec::with_capacity(entries.len()),
        scope_paths: Vec::with_capacity(entries.len()),
    };
    for entry in entries {
        binds.recorded_ats.push(entry.recorded_at);
        binds.entity_ids.push(entry.entity_id.as_str());
        binds.write_ids.push(entry.identity.write_id.as_uuid());
        binds.write_ordinals.push(
            entry
                .identity
                .ordinal
                .as_i32()
                .map_err(|error| TemporalStoreError::Validation(format!("scope write ordinal {} is out of range: {error}", entry.identity.ordinal)))?,
        );
        binds.value_kinds.push(ScopeValueKind::code(entry.value.kind()));
        binds.observed_ats.push(entry.axes.observed_at);
        binds.created_ats.push(entry.axes.created_at);
        binds.modified_ats.push(entry.axes.modified_at);
        binds.deleted_ats.push(entry.value.deleted_at());
        binds.generated_instance_uuids.push(generated_instance_uuid(&entry.instance));
        binds.verbatim_instance_ids.push(verbatim_instance_id(&entry.instance));
        binds.scope_paths.push(entry.value.path().map(|path| scope_to_ltree(path)));
    }
    Ok(binds)
}

const SCOPE_HISTORY_DEDUP_SQL: &str = r#"
DELETE FROM scope_history sh
USING UNNEST($1::text[], $2::uuid[], $3::text[]) AS d(entity_id, generated_instance_uuid, verbatim_instance_id)
JOIN temporal_entities te ON te.id = d.entity_id
WHERE sh.temporal_entity_key = te.entity_key
  AND sh.generated_instance_uuid IS NOT DISTINCT FROM d.generated_instance_uuid
  AND sh.verbatim_instance_id IS NOT DISTINCT FROM d.verbatim_instance_id
"#;

const SCOPE_HISTORY_INSERT_SQL: &str = r#"
INSERT INTO scope_history (
    recorded_at, temporal_entity_key, write_id, write_ordinal, value_kind,
    observed_at, created_at, modified_at, deleted_at,
    generated_instance_uuid, verbatim_instance_id, scope_path
)
SELECT d.recorded_at, te.entity_key, d.write_id, d.write_ordinal, d.value_kind,
       d.observed_at, d.created_at, d.modified_at, d.deleted_at,
       d.generated_instance_uuid, d.verbatim_instance_id, d.scope_path::ltree
FROM UNNEST(
    $1::timestamptz[], $2::text[], $3::uuid[], $4::integer[], $5::smallint[],
    $6::timestamptz[], $7::timestamptz[], $8::timestamptz[], $9::timestamptz[],
    $10::uuid[], $11::text[], $12::text[]
) AS d(recorded_at, entity_id, write_id, write_ordinal, value_kind,
       observed_at, created_at, modified_at, deleted_at,
       generated_instance_uuid, verbatim_instance_id, scope_path)
JOIN temporal_entities te ON te.id = d.entity_id
ON CONFLICT ON CONSTRAINT uq_scope_history_write DO NOTHING
"#;

#[cfg(test)]
mod tests {
    use crate::scope_history_write::{SCOPE_HISTORY_DEDUP_SQL, SCOPE_HISTORY_INSERT_SQL};

    #[test]
    fn a_scope_row_hangs_off_the_temporal_catalogue_and_never_off_current_state() {
        for statement in [SCOPE_HISTORY_DEDUP_SQL, SCOPE_HISTORY_INSERT_SQL] {
            assert!(statement.contains("temporal_entities te ON te.id = d.entity_id"));
            assert!(!statement.contains("FROM entities"));
        }
    }

    #[test]
    fn the_insert_carries_the_partition_column_and_the_whole_write_identity() {
        assert!(SCOPE_HISTORY_INSERT_SQL.contains("recorded_at, temporal_entity_key, write_id, write_ordinal"));
    }

    /// A commit replayed under its original write identity re-inserts every
    /// row it already wrote; the row-level conflict clause is what makes that
    /// second insert a no-op instead of a duplicate or an error.
    #[test]
    fn a_row_replayed_under_its_original_write_identity_is_not_duplicated() {
        assert!(SCOPE_HISTORY_INSERT_SQL.contains("ON CONFLICT ON CONSTRAINT uq_scope_history_write DO NOTHING"));
    }
}
