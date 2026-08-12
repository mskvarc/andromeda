//! Reading and pruning the temporal catalogue.
//!
//! Every question the temporal mutation policy asks before it acts — does this
//! entity have a Temporal Evolution, does it record this attribute, which
//! datasets does that attribute have — is answered from `temporal_entities` and
//! `temporal_attribute_streams` alone. Current state is never consulted: clause
//! 5.6.6 leaves the Temporal Evolution standing when the entity is deleted, so
//! an answer taken from `entities` would report history that exists as absent.

use crate::{error::TemporalStoreError, repo::TemporalRepository, write_record_release::release_named};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use sqlx::Row;
use std::collections::HashMap;
use uuid::Uuid;

/// Which dataset instances of one attribute a deletion addresses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributeStreamScope {
    /// Every dataset instance of the attribute.
    All,
    /// The one named dataset instance, or the default instance when none is named.
    Single,
}

impl TemporalRepository {
    /// Whether the entity has a Temporal Evolution at all.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails.
    pub(crate) async fn temporal_entity_exists(&self, entity_id: &Uri) -> Result<bool, TemporalStoreError> {
        let exists = sqlx::query_scalar(TEMPORAL_ENTITY_EXISTS_SQL)
            .bind(entity_id.as_str())
            .fetch_one(&self.pool)
            .await?;
        Ok(exists)
    }

    /// The entity types the catalogue records for each named Temporal Evolution.
    ///
    /// The catalogue is the authority for them (clause 4.5.1 makes `type`
    /// mandatory in every representation, and clause 5.6.6 leaves the Temporal
    /// Evolution standing when current state is deleted), so an entity absent
    /// from the catalogue is absent from the result rather than typed empty.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails and
    /// [`TemporalStoreError::Decode`] when a stored identifier or type name is
    /// not the validated shape the catalogue writes.
    pub(crate) async fn temporal_entity_types(&self, entity_ids: &[Uri]) -> Result<HashMap<Uri, Vec<EntityTypeName>>, TemporalStoreError> {
        if entity_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let identifiers: Vec<&str> = entity_ids.iter().map(Uri::as_str).collect();
        let rows = sqlx::query(TEMPORAL_ENTITY_TYPES_SQL).bind(&identifiers).fetch_all(&self.pool).await?;
        let mut types_by_entity = HashMap::with_capacity(rows.len());
        for row in rows {
            let raw_id: String = row
                .try_get("id")
                .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_entities.id: {error}")))?;
            let entity_id = raw_id
                .parse::<Uri>()
                .map_err(|error| TemporalStoreError::Decode(format!("temporal_entities.id '{raw_id}': {error}")))?;
            let raw_types: Vec<String> = row
                .try_get("types")
                .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_entities.types: {error}")))?;
            let types = raw_types
                .into_iter()
                .map(|raw| {
                    raw.parse::<EntityTypeName>()
                        .map_err(|error| TemporalStoreError::Decode(format!("temporal_entities.types entry '{raw}': {error}")))
                })
                .collect::<Result<Vec<EntityTypeName>, TemporalStoreError>>()?;
            types_by_entity.insert(entity_id, types);
        }
        Ok(types_by_entity)
    }

    /// Whether the entity records any stream for the named attribute.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails.
    pub(crate) async fn attribute_stream_exists(&self, entity_id: &Uri, attribute_name: &AttributeName) -> Result<bool, TemporalStoreError> {
        let exists = sqlx::query_scalar(ATTRIBUTE_STREAM_EXISTS_SQL)
            .bind(entity_id.as_str())
            .bind(attribute_name.as_str())
            .fetch_one(&self.pool)
            .await?;
        Ok(exists)
    }

    /// The `datasetId` of every stream the entity records for one attribute.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails and
    /// [`TemporalStoreError::Decode`] when a stored discriminator is not a URI.
    pub(crate) async fn attribute_dataset_ids(&self, entity_id: &Uri, attribute_name: &AttributeName) -> Result<Vec<Option<Uri>>, TemporalStoreError> {
        let rows = sqlx::query(ATTRIBUTE_DATASET_IDS_SQL)
            .bind(entity_id.as_str())
            .bind(attribute_name.as_str())
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|row| {
                row.try_get::<Option<String>, _>("dataset_id")
                    .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_attribute_streams.dataset_id: {error}")))?
                    .map(|raw| {
                        raw.parse::<Uri>()
                            .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_attribute_streams.dataset_id '{raw}': {error}")))
                    })
                    .transpose()
            })
            .collect()
    }

    /// Every expanded attribute IRI the entity records a stream for.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails and
    /// [`TemporalStoreError::Decode`] when a stored name is not an attribute IRI.
    pub(crate) async fn temporal_attribute_names(&self, entity_id: &Uri) -> Result<Vec<AttributeName>, TemporalStoreError> {
        let rows = sqlx::query(TEMPORAL_ATTRIBUTE_NAMES_SQL).bind(entity_id.as_str()).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                let raw: String = row.try_get("attribute_name")?;
                raw.parse::<AttributeName>()
                    .map_err(|error| TemporalStoreError::Decode(format!("Invalid attribute_names.name '{raw}': {error}")))
            })
            .collect()
    }

    /// Remove the addressed attribute streams and every observation they hold.
    ///
    /// Clause 5.6.17 deletes the Temporal Evolution of one attribute, so the
    /// history goes with the stream rather than being tombstoned; nothing in the
    /// database sequences that, so the order is stated here inside one
    /// transaction, and the idempotency records the removed rows were the last
    /// protected effect of are released with them.
    ///
    /// Returns how many streams the deletion removed.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when a statement fails and
    /// [`TemporalStoreError::Decode`] when a removed row's write identity does
    /// not carry the expected type.
    pub(crate) async fn delete_attribute_streams(
        &self,
        entity_id: &Uri,
        attribute_name: &AttributeName,
        dataset_id: Option<&Uri>,
        scope: AttributeStreamScope,
    ) -> Result<u64, TemporalStoreError> {
        let statement = match scope {
            AttributeStreamScope::All => SELECT_ATTRIBUTE_STREAMS_SQL,
            AttributeStreamScope::Single => match dataset_id {
                Some(_) => SELECT_ATTRIBUTE_STREAMS_FOR_DATASET_SQL,
                None => SELECT_ATTRIBUTE_STREAMS_FOR_DEFAULT_SQL,
            },
        };
        let mut query = sqlx::query_scalar::<_, Uuid>(statement).bind(entity_id.as_str()).bind(attribute_name.as_str());
        if let (AttributeStreamScope::Single, Some(dataset_id)) = (scope, dataset_id) {
            query = query.bind(dataset_id.as_str());
        }
        let mut tx = self.pool.begin().await?;
        let stream_ids: Vec<Uuid> = query.fetch_all(&mut *tx).await?;
        if stream_ids.is_empty() {
            tx.rollback().await?;
            return Ok(0);
        }
        let removed = sqlx::query(DELETE_STREAM_HISTORY_SQL).bind(&stream_ids).fetch_all(&mut *tx).await?;
        let touched_writes = removed
            .iter()
            .map(|row| {
                row.try_get::<Uuid, _>("write_id")
                    .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_history.write_id: {error}")))
            })
            .collect::<Result<Vec<Uuid>, _>>()?;
        let streams = sqlx::query(DELETE_STREAMS_SQL).bind(&stream_ids).execute(&mut *tx).await?.rows_affected();
        release_named(&mut tx, &touched_writes).await?;
        tx.commit().await?;
        Ok(streams)
    }
}

const TEMPORAL_ENTITY_EXISTS_SQL: &str = "SELECT EXISTS(SELECT 1 FROM temporal_entities WHERE id = $1)";

const TEMPORAL_ENTITY_TYPES_SQL: &str = "SELECT id, types FROM temporal_entities WHERE id = ANY($1)";

const ATTRIBUTE_STREAM_EXISTS_SQL: &str = r#"
SELECT EXISTS(
    SELECT 1
    FROM temporal_attribute_streams s
    JOIN temporal_entities te ON te.entity_key = s.temporal_entity_key
    JOIN attribute_names an ON an.id = s.attribute_name_id
    WHERE te.id = $1 AND an.name = $2
)
"#;

const ATTRIBUTE_DATASET_IDS_SQL: &str = r#"
SELECT s.dataset_id
FROM temporal_attribute_streams s
JOIN temporal_entities te ON te.entity_key = s.temporal_entity_key
JOIN attribute_names an ON an.id = s.attribute_name_id
WHERE te.id = $1 AND an.name = $2
"#;

const TEMPORAL_ATTRIBUTE_NAMES_SQL: &str = r#"
SELECT DISTINCT an.name AS attribute_name
FROM temporal_attribute_streams s
JOIN temporal_entities te ON te.entity_key = s.temporal_entity_key
JOIN attribute_names an ON an.id = s.attribute_name_id
WHERE te.id = $1
"#;

const SELECT_ATTRIBUTE_STREAMS_SQL: &str = r#"
SELECT s.id
FROM temporal_attribute_streams s
JOIN temporal_entities te ON te.entity_key = s.temporal_entity_key
JOIN attribute_names an ON an.id = s.attribute_name_id
WHERE te.id = $1 AND an.name = $2
FOR UPDATE OF s
"#;

const SELECT_ATTRIBUTE_STREAMS_FOR_DATASET_SQL: &str = r#"
SELECT s.id
FROM temporal_attribute_streams s
JOIN temporal_entities te ON te.entity_key = s.temporal_entity_key
JOIN attribute_names an ON an.id = s.attribute_name_id
WHERE te.id = $1 AND an.name = $2 AND s.dataset_id = $3
FOR UPDATE OF s
"#;

const SELECT_ATTRIBUTE_STREAMS_FOR_DEFAULT_SQL: &str = r#"
SELECT s.id
FROM temporal_attribute_streams s
JOIN temporal_entities te ON te.entity_key = s.temporal_entity_key
JOIN attribute_names an ON an.id = s.attribute_name_id
WHERE te.id = $1 AND an.name = $2 AND s.dataset_id IS NULL
FOR UPDATE OF s
"#;

const DELETE_STREAM_HISTORY_SQL: &str = "DELETE FROM attribute_history WHERE stream_id = ANY($1) RETURNING write_id";

const DELETE_STREAMS_SQL: &str = "DELETE FROM temporal_attribute_streams WHERE id = ANY($1)";

#[cfg(test)]
mod tests {
    use crate::temporal_catalog_read::{
        ATTRIBUTE_DATASET_IDS_SQL,
        ATTRIBUTE_STREAM_EXISTS_SQL,
        DELETE_STREAM_HISTORY_SQL,
        DELETE_STREAMS_SQL,
        SELECT_ATTRIBUTE_STREAMS_FOR_DATASET_SQL,
        SELECT_ATTRIBUTE_STREAMS_FOR_DEFAULT_SQL,
        SELECT_ATTRIBUTE_STREAMS_SQL,
        TEMPORAL_ATTRIBUTE_NAMES_SQL,
        TEMPORAL_ENTITY_EXISTS_SQL,
    };

    const CATALOGUE_READS: [&str; 5] = [
        TEMPORAL_ENTITY_EXISTS_SQL,
        ATTRIBUTE_STREAM_EXISTS_SQL,
        ATTRIBUTE_DATASET_IDS_SQL,
        TEMPORAL_ATTRIBUTE_NAMES_SQL,
        SELECT_ATTRIBUTE_STREAMS_SQL,
    ];

    #[test]
    fn no_catalogue_question_is_answered_from_current_state() {
        for statement in CATALOGUE_READS {
            assert!(!statement.contains("FROM entities"));
            assert!(!statement.contains("JOIN entities"));
            assert!(!statement.contains("temporal_entity_attributes"));
        }
    }

    #[test]
    fn the_default_instance_is_addressed_by_absence_rather_than_by_a_value() {
        assert!(SELECT_ATTRIBUTE_STREAMS_FOR_DEFAULT_SQL.contains("s.dataset_id IS NULL"));
        assert!(SELECT_ATTRIBUTE_STREAMS_FOR_DATASET_SQL.contains("s.dataset_id = $3"));
    }

    #[test]
    fn attribute_deletion_removes_the_observations_before_the_stream_holding_them() {
        assert!(DELETE_STREAM_HISTORY_SQL.starts_with("DELETE FROM attribute_history"));
        assert!(DELETE_STREAM_HISTORY_SQL.contains("RETURNING write_id"));
        assert!(DELETE_STREAMS_SQL.starts_with("DELETE FROM temporal_attribute_streams"));
    }

    #[test]
    fn attribute_deletion_records_no_tombstone() {
        for statement in [DELETE_STREAM_HISTORY_SQL, DELETE_STREAMS_SQL] {
            assert!(!statement.contains("INSERT"));
            assert!(!statement.contains("deleted_at"));
        }
    }
}
