//! Removing entities and attribute instances whose `expiresAt` has passed
//! (clause 4.22).

use crate::{
    entity::{mapper::stored_uri, repo::EntityRepository, snapshot::EntitySnapshot},
    error::ContextStoreError,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use sqlx::Row;

/// Every entity carrying at least one past-due attribute instance, with the
/// payload and the `modified_at` needed to rewrite it.
///
/// This is a read, and deliberately only a read. Deciding what the payload
/// looks like without its expired instances is specification semantics, and the
/// tombstones that removal implies have to commit in the same transaction as
/// the rewritten payload -- which is a transaction this statement is not in. The
/// caller reads, computes, and writes back under a precondition, so an entity
/// another writer touched in between is simply picked up on the next sweep.
const ENTITIES_WITH_EXPIRED_ATTRIBUTES_SQL: &str = r#"
SELECT id, payload, modified_at
FROM entities
WHERE EXISTS (
    SELECT 1
    FROM (
        SELECT key, value
        FROM jsonb_each(payload->0)
        WHERE key NOT LIKE '@%' AND jsonb_typeof(value) = 'array'
    ) AS kv(key, val),
    LATERAL jsonb_array_elements(kv.val) AS inst(elem)
    WHERE (inst.elem #>>'{https://uri.etsi.org/ngsi-ld/expiresAt,0,@value}')::timestamptz <= NOW()
)
"#;

impl EntityRepository {
    /// Delete entities whose `expiresAt` has passed, returning their identifiers.
    pub async fn expire_due_entities(&self) -> Result<Vec<Uri>, ContextStoreError> {
        let rows: Vec<(String,)> = sqlx::query_as("DELETE FROM entities WHERE expires_at IS NOT NULL AND expires_at <= NOW() RETURNING id")
            .fetch_all(&self.pool)
            .await?;

        rows.into_iter().map(|(id,)| stored_uri(id)).collect()
    }

    /// Read every entity that holds a past-due attribute instance.
    pub async fn entities_with_expired_attributes(&self) -> Result<Vec<(Uri, EntitySnapshot)>, ContextStoreError> {
        let rows = sqlx::query(ENTITIES_WITH_EXPIRED_ATTRIBUTES_SQL).fetch_all(&self.pool).await?;

        rows.into_iter()
            .map(|row| {
                Ok((
                    stored_uri(row.get("id"))?,
                    EntitySnapshot {
                        payload: row.get("payload"),
                        modified_at: row.get("modified_at"),
                    },
                ))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::ENTITIES_WITH_EXPIRED_ATTRIBUTES_SQL;

    #[test]
    fn the_expiry_candidate_query_reads_the_version_token_with_the_payload() {
        assert!(ENTITIES_WITH_EXPIRED_ATTRIBUTES_SQL.contains("SELECT id, payload, modified_at"));
    }

    #[test]
    fn the_expiry_candidate_query_writes_nothing() {
        for statement in ["UPDATE", "DELETE", "INSERT"] {
            assert!(
                !ENTITIES_WITH_EXPIRED_ATTRIBUTES_SQL.contains(statement),
                "the candidate query must not mutate: found {statement}"
            );
        }
    }
}
