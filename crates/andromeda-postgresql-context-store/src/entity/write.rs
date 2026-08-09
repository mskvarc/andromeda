//! The statements that write the `entities` table.
//!
//! There is one statement per kind of change, and it serves any number of
//! entities: a single-entity write is a set of one, which is why no per-row
//! variant exists. Each statement returns the identifiers it affected, and the
//! caller turns that into a per-entity outcome by asking what the write assumed.
//!
//! No statement here reads. A read-modify-write takes its "before" snapshot
//! outside the transaction and states what it assumed in a precondition, which
//! the update statement enforces.

use crate::{
    entity::{mapper::stored_uri, params::PreparedEntityColumns, repo::EntityRepository},
    error::ContextStoreError,
};
use andromeda_context_commit::deleted_entity::DeletedEntity;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};

/// Insert entities that are not there, leaving any that are.
///
/// The conflict clause is how a duplicate create is detected: the identifier
/// simply does not come back, and the caller answers 409 off that (NGSI-LD
/// 6.4.3.1). Suppressing the conflict rather than raising a duplicate-key error
/// is what lets one statement serve a batch that is permitted to succeed
/// partially (clause 5.6.7) and the idempotent placeholder the Temporal API
/// needs, without three different statements.
const ENTITY_INSERT_SQL: &str = r#"
INSERT INTO entities (id, types, scope_paths, payload, location, observation_space, operation_space, expires_at)
SELECT
    d.id,
    ARRAY(SELECT jsonb_array_elements_text(d.types_json)),
    ARRAY(SELECT jsonb_array_elements_text(d.scope_paths_json)::ltree),
    d.payload::jsonb,
    CASE WHEN d.loc IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.loc)) ELSE NULL END,
    CASE WHEN d.obs IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.obs)) ELSE NULL END,
    CASE WHEN d.ops IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.ops)) ELSE NULL END,
    d.expires_at
FROM UNNEST($1::text[], $2::jsonb[], $3::jsonb[], $4::text[], $5::bytea[], $6::bytea[], $7::bytea[], $8::timestamptz[])
    AS d(id, types_json, scope_paths_json, payload, loc, obs, ops, expires_at)
ON CONFLICT (id) DO NOTHING
RETURNING id
"#;

/// Overwrite entities, optionally only where they still stand as the caller
/// read them.
///
/// The expectation column is nullable so one statement carries both an
/// unconditional overwrite and a compare-and-set in the same batch: a null
/// expectation matches whatever is there, a non-null one matches only an
/// untouched row. An identifier that does not come back therefore means either
/// "gone" or "moved on", and which of those it is depends on the precondition
/// the caller attached.
const ENTITY_UPDATE_SQL: &str = r#"
UPDATE entities SET
    payload = d.payload::jsonb,
    types = ARRAY(SELECT jsonb_array_elements_text(d.types_json)),
    scope_paths = ARRAY(SELECT jsonb_array_elements_text(d.scope_paths_json)::ltree),
    location = CASE WHEN d.loc IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.loc)) ELSE NULL END,
    observation_space = CASE WHEN d.obs IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.obs)) ELSE NULL END,
    operation_space = CASE WHEN d.ops IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.ops)) ELSE NULL END,
    modified_at = NOW(),
    expires_at = COALESCE(d.expires_at, entities.expires_at)
FROM UNNEST($1::text[], $2::jsonb[], $3::jsonb[], $4::text[], $5::bytea[], $6::bytea[], $7::bytea[], $8::timestamptz[], $9::timestamptz[])
    AS d(id, types_json, scope_paths_json, payload, loc, obs, ops, expires_at, expected_modified_at)
WHERE entities.id = d.id
  AND (d.expected_modified_at IS NULL OR entities.modified_at = d.expected_modified_at)
RETURNING entities.id
"#;

/// Remove entities, optionally only where they still stand as the caller read
/// them.
///
/// Carries the same nullable expectation column the update statement does, and
/// for the same reason: a delete computed from a payload the caller read is a
/// read-modify-write like any other, and one that overwrote a state its author
/// never saw would be a lost update whether it wrote a row or removed one. An
/// identifier that does not come back means "gone" or "moved on", and the
/// precondition says which.
const ENTITY_DELETE_SQL: &str = r#"
DELETE FROM entities
USING UNNEST($1::text[], $2::timestamptz[]) AS d(id, expected_modified_at)
WHERE entities.id = d.id
  AND (d.expected_modified_at IS NULL OR entities.modified_at = d.expected_modified_at)
RETURNING entities.id, entities.payload, entities.created_at, entities.modified_at, entities.expires_at
"#;

impl EntityRepository {
    /// Insert every entity in `entities` that does not already exist, returning
    /// the identifiers actually inserted.
    pub async fn create_batch_in_tx(tx: &mut Transaction<'_, Postgres>, entities: &PreparedEntityColumns) -> Result<Vec<Uri>, ContextStoreError> {
        if entities.ids.is_empty() {
            return Ok(Vec::new());
        }

        let ids = entities.ids.iter().map(Uri::as_str).collect::<Vec<_>>();
        let rows: Vec<(String,)> = sqlx::query_as(ENTITY_INSERT_SQL)
            .bind(&ids)
            .bind(&entities.types_json)
            .bind(&entities.scope_paths_json)
            .bind(&entities.payload_json)
            .bind(&entities.locations)
            .bind(&entities.obs_spaces)
            .bind(&entities.op_spaces)
            .bind(&entities.expires_ats)
            .fetch_all(&mut **tx)
            .await?;

        rows.into_iter().map(|(id,)| stored_uri(id)).collect()
    }

    /// Overwrite every entity in `entities` whose expectation still holds,
    /// returning the identifiers actually written.
    ///
    /// `expected_modified_at` is positionally aligned with `entities`; a `None`
    /// entry overwrites unconditionally.
    pub async fn save_batch_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        entities: &PreparedEntityColumns,
        expected_modified_at: &[Option<DateTime<Utc>>],
    ) -> Result<Vec<Uri>, ContextStoreError> {
        if entities.ids.is_empty() {
            return Ok(Vec::new());
        }
        if expected_modified_at.len() != entities.ids.len() {
            return Err(ContextStoreError::Internal(format!(
                "entity update given {} expectations for {} rows",
                expected_modified_at.len(),
                entities.ids.len()
            )));
        }

        let ids = entities.ids.iter().map(Uri::as_str).collect::<Vec<_>>();
        let rows: Vec<(String,)> = sqlx::query_as(ENTITY_UPDATE_SQL)
            .bind(&ids)
            .bind(&entities.types_json)
            .bind(&entities.scope_paths_json)
            .bind(&entities.payload_json)
            .bind(&entities.locations)
            .bind(&entities.obs_spaces)
            .bind(&entities.op_spaces)
            .bind(&entities.expires_ats)
            .bind(expected_modified_at)
            .fetch_all(&mut **tx)
            .await?;

        rows.into_iter().map(|(id,)| stored_uri(id)).collect()
    }

    /// Delete every entity in `ids` whose expectation still holds, returning
    /// what each one held.
    ///
    /// The payload and system attributes come back because the clause 5.6.10
    /// notification carries both and cannot read them once the row is gone.
    ///
    /// `expected_modified_at` is positionally aligned with `ids`; a `None` entry
    /// deletes unconditionally.
    pub async fn delete_batch_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        ids: &[Uri],
        expected_modified_at: &[Option<DateTime<Utc>>],
    ) -> Result<Vec<DeletedEntity>, ContextStoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        if expected_modified_at.len() != ids.len() {
            return Err(ContextStoreError::Internal(format!(
                "entity delete given {} expectations for {} rows",
                expected_modified_at.len(),
                ids.len()
            )));
        }

        let ids = ids.iter().map(Uri::as_str).collect::<Vec<_>>();
        let rows = sqlx::query(ENTITY_DELETE_SQL).bind(ids).bind(expected_modified_at).fetch_all(&mut **tx).await?;

        rows.into_iter()
            .map(|row| {
                Ok(DeletedEntity {
                    id: stored_uri(row.get("id"))?,
                    payload: row.get("payload"),
                    created_at: row.get("created_at"),
                    modified_at: row.get("modified_at"),
                    expires_at: row.get("expires_at"),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{ENTITY_DELETE_SQL, ENTITY_INSERT_SQL, ENTITY_UPDATE_SQL};
    use std::collections::BTreeSet;

    /// Every `$n` placeholder in `sql`, as numbers.
    fn placeholders(sql: &str) -> BTreeSet<u32> {
        sql.match_indices('$')
            .filter_map(|(at, _)| {
                let digits: String = sql[at + 1..].chars().take_while(char::is_ascii_digit).collect();
                digits.parse().ok()
            })
            .collect()
    }

    #[test]
    fn the_fused_update_honours_a_null_expectation() {
        assert!(ENTITY_UPDATE_SQL.contains("d.expected_modified_at IS NULL OR entities.modified_at = d.expected_modified_at"));
    }

    #[test]
    fn the_fused_update_binds_contiguous_placeholders() {
        pretty_assertions::assert_eq!(placeholders(ENTITY_UPDATE_SQL), (1..=9).collect::<BTreeSet<u32>>());
    }

    #[test]
    fn the_fused_insert_binds_contiguous_placeholders() {
        pretty_assertions::assert_eq!(placeholders(ENTITY_INSERT_SQL), (1..=8).collect::<BTreeSet<u32>>());
    }

    #[test]
    fn the_fused_delete_honours_a_null_expectation() {
        assert!(ENTITY_DELETE_SQL.contains("d.expected_modified_at IS NULL OR entities.modified_at = d.expected_modified_at"));
    }

    #[test]
    fn the_fused_delete_binds_contiguous_placeholders() {
        pretty_assertions::assert_eq!(placeholders(ENTITY_DELETE_SQL), (1..=2).collect::<BTreeSet<u32>>());
    }

    #[test]
    fn the_fused_delete_returns_what_the_notification_needs() {
        for column in [
            "entities.id",
            "entities.payload",
            "entities.created_at",
            "entities.modified_at",
            "entities.expires_at",
        ] {
            assert!(ENTITY_DELETE_SQL.contains(column), "the delete must return {column}");
        }
    }

    #[test]
    fn the_fused_insert_keeps_its_conflict_clause() {
        assert!(ENTITY_INSERT_SQL.contains("ON CONFLICT (id) DO NOTHING"));
        assert!(ENTITY_INSERT_SQL.contains("RETURNING id"));
    }
}
