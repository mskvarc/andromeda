//! Establishing the temporal catalogue rows one write's history hangs off.
//!
//! The catalogue is two relations. `temporal_entities` names the entity whose
//! Temporal Evolution is being recorded and carries the narrow key everything
//! else references; `temporal_attribute_streams` names one attribute of that
//! entity under one `datasetId` and carries the identifier every history row is
//! stored against. Neither references `entities`: clause 5.6.6 leaves temporal
//! evolution standing when current state is deleted, so the temporal catalogue
//! has to be able to exist without it.

use crate::{
    error::TemporalStoreError,
    repo::TemporalRepository,
    temporal_stream::{TemporalCatalogUpsert, TemporalStreamKey},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_postgresql_codes::{stream_id::StreamId, temporal_entity_key::TemporalEntityKey};
use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

impl TemporalRepository {
    /// Establish the catalogue rows one write's history rows are stored against.
    ///
    /// Returns the stream each requested grain resolved to, whether the write
    /// created it or found it already present.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::AttributeLookup`] when the attribute-name
    /// dictionary cannot resolve a name, [`TemporalStoreError::NotFound`] when
    /// the write declares no entity types and no temporal entity exists to
    /// extend, [`TemporalStoreError::Query`] when the catalogue does not return
    /// a requested grain, and [`TemporalStoreError::Database`] when a statement
    /// fails.
    pub async fn upsert_temporal_catalog_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        upsert: &TemporalCatalogUpsert,
        recorded_at: DateTime<Utc>,
    ) -> Result<HashMap<TemporalStreamKey, StreamId>, TemporalStoreError> {
        let entity_key = upsert_temporal_entity(tx, upsert, recorded_at).await?;
        if upsert.streams.is_empty() {
            return Ok(HashMap::new());
        }
        let name_to_id = self
            .attribute_names
            .resolve_or_create(&distinct_attribute_names(&upsert.streams))
            .await
            .map_err(|error| TemporalStoreError::AttributeLookup(error.to_string()))?;
        let binds = stream_binds(&upsert.streams, &name_to_id)?;
        let rows = sqlx::query(STREAM_UPSERT_SQL)
            .bind(entity_key.get())
            .bind(&binds.ids)
            .bind(&binds.attribute_name_ids)
            .bind(&binds.dataset_ids)
            .fetch_all(&mut **tx)
            .await?;
        let resolved = rows.iter().map(decode_stream_row).collect::<Result<HashMap<_, _>, _>>()?;
        for key in &upsert.streams {
            if !resolved.contains_key(key) {
                return Err(TemporalStoreError::Query(format!(
                    "temporal catalogue upsert returned no stream for attribute '{}'",
                    key.attribute_name
                )));
            }
        }
        Ok(resolved)
    }
}

/// Create or refresh the temporal entity the write extends, and return its key.
///
/// A write that carries entity types states what the entity is and may create
/// it; a write that carries none — an attribute deletion, or a scope-only
/// observation — can only extend a temporal entity that already exists, because
/// clause 4.5.1 makes at least one type part of an entity's identity and
/// nothing here may invent one.
async fn upsert_temporal_entity(
    tx: &mut Transaction<'_, Postgres>,
    upsert: &TemporalCatalogUpsert,
    recorded_at: DateTime<Utc>,
) -> Result<TemporalEntityKey, TemporalStoreError> {
    if !upsert.types.is_empty() {
        let types = upsert.types.iter().map(EntityTypeName::as_str).collect::<Vec<_>>();
        sqlx::query(TEMPORAL_ENTITY_UPSERT_SQL)
            .bind(upsert.entity_id.as_str())
            .bind(recorded_at)
            .bind(&types)
            .execute(&mut **tx)
            .await?;
    }
    let located = sqlx::query(TEMPORAL_ENTITY_KEY_SQL)
        .bind(upsert.entity_id.as_str())
        .fetch_optional(&mut **tx)
        .await?;
    let Some(row) = located else {
        return Err(TemporalStoreError::NotFound(format!(
            "Temporal evolution of {} does not exist and the write declares no entity type to create it with",
            upsert.entity_id
        )));
    };
    let stored: i64 = row
        .try_get("entity_key")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_entities.entity_key: {error}")))?;
    TemporalEntityKey::try_from(stored).map_err(|error| TemporalStoreError::Decode(error.to_string()))
}

/// Column-major binds for one stream-catalogue upsert.
struct StreamBinds<'a> {
    /// Identifier each stream is created with, ignored where one already exists.
    ids: Vec<Uuid>,
    /// Dictionary identity of each stream's attribute name.
    attribute_name_ids: Vec<i32>,
    /// Multi-attribute discriminator of each stream.
    dataset_ids: Vec<Option<&'a str>>,
}

/// The distinct attribute names one catalogue upsert has to resolve.
fn distinct_attribute_names(streams: &[TemporalStreamKey]) -> Vec<AttributeName> {
    let mut seen = HashSet::with_capacity(streams.len());
    streams
        .iter()
        .filter(|key| seen.insert(key.attribute_name.as_str()))
        .map(|key| key.attribute_name.clone()) // clone: dictionary resolution owns each distinct name.
        .collect()
}

/// Spread the requested grains into column-major binds, one row per grain.
///
/// Repeated grains collapse to one row: one write may carry several
/// observations of one attribute, and they all extend the same stream.
fn stream_binds<'a>(streams: &'a [TemporalStreamKey], name_to_id: &HashMap<AttributeName, AttributeNameId>) -> Result<StreamBinds<'a>, TemporalStoreError> {
    let mut binds = StreamBinds {
        ids: Vec::with_capacity(streams.len()),
        attribute_name_ids: Vec::with_capacity(streams.len()),
        dataset_ids: Vec::with_capacity(streams.len()),
    };
    let mut seen: HashSet<(AttributeNameId, Option<&'a str>)> = HashSet::with_capacity(streams.len());
    for key in streams {
        let attribute_name_id = *name_to_id
            .get(&key.attribute_name)
            .ok_or_else(|| TemporalStoreError::AttributeLookup(format!("Attribute name '{}' not found in dictionary after resolve", key.attribute_name)))?;
        let dataset_id = key.dataset_id.as_ref().map(Uri::as_str);
        if !seen.insert((attribute_name_id, dataset_id)) {
            continue;
        }
        binds.ids.push(StreamId::generate().get());
        binds.attribute_name_ids.push(attribute_name_id.get());
        binds.dataset_ids.push(dataset_id);
    }
    Ok(binds)
}

/// Read back one resolved stream and the grain it answers for.
fn decode_stream_row(row: &sqlx::postgres::PgRow) -> Result<(TemporalStreamKey, StreamId), TemporalStoreError> {
    let raw_name: String = row
        .try_get("attribute_name")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_names.name: {error}")))?;
    let attribute_name = raw_name
        .parse::<AttributeName>()
        .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_names.name '{raw_name}': {error}")))?;
    let dataset_id = row
        .try_get::<Option<String>, _>("dataset_id")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_attribute_streams.dataset_id: {error}")))?
        .map(|raw| {
            raw.parse::<Uri>()
                .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_attribute_streams.dataset_id '{raw}': {error}")))
        })
        .transpose()?;
    let id: Uuid = row
        .try_get("id")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_attribute_streams.id: {error}")))?;
    Ok((TemporalStreamKey { attribute_name, dataset_id }, StreamId::from(id)))
}

/// The temporal entity a write with declared types creates or refreshes.
///
/// `created_at` is set once and never moved: the conflict clause advances only
/// `modified_at` and the declared types, so the instant the Temporal Evolution
/// began survives every later write (clause 4.8).
const TEMPORAL_ENTITY_UPSERT_SQL: &str = r#"
INSERT INTO temporal_entities (id, created_at, modified_at, types)
VALUES ($1, $2, $2, $3::text[])
ON CONFLICT (id) DO UPDATE
SET modified_at = EXCLUDED.modified_at, types = EXCLUDED.types
"#;

const TEMPORAL_ENTITY_KEY_SQL: &str = r#"
SELECT entity_key FROM temporal_entities WHERE id = $1
"#;

/// The streams one write needs, created where absent and read back where present.
///
/// A statement's own `INSERT` is invisible to the `SELECT` beside it in the same
/// command, so the rows this call created are reported from the `RETURNING`
/// clause and only the grains it did not create are read from the table.
const STREAM_UPSERT_SQL: &str = r#"
WITH input AS (
    SELECT d.id, d.attribute_name_id, d.dataset_id
    FROM UNNEST($2::uuid[], $3::integer[], $4::text[]) AS d(id, attribute_name_id, dataset_id)
),
inserted AS (
    INSERT INTO temporal_attribute_streams (id, temporal_entity_key, attribute_name_id, dataset_id)
    SELECT i.id, $1, i.attribute_name_id, i.dataset_id FROM input i
    ON CONFLICT (temporal_entity_key, attribute_name_id, dataset_id) DO NOTHING
    RETURNING id, attribute_name_id, dataset_id
)
SELECT ins.id, an.name AS attribute_name, ins.dataset_id
FROM inserted ins
JOIN attribute_names an ON an.id = ins.attribute_name_id
UNION ALL
SELECT s.id, an.name AS attribute_name, s.dataset_id
FROM input i
JOIN temporal_attribute_streams s
    ON s.temporal_entity_key = $1
   AND s.attribute_name_id = i.attribute_name_id
   AND s.dataset_id IS NOT DISTINCT FROM i.dataset_id
JOIN attribute_names an ON an.id = s.attribute_name_id
WHERE NOT EXISTS (
    SELECT 1 FROM inserted ins
    WHERE ins.attribute_name_id = i.attribute_name_id
      AND ins.dataset_id IS NOT DISTINCT FROM i.dataset_id
)
"#;

#[cfg(test)]
mod tests {
    use crate::{
        temporal_catalog_write::{STREAM_UPSERT_SQL, TEMPORAL_ENTITY_UPSERT_SQL, distinct_attribute_names, stream_binds},
        temporal_stream::TemporalStreamKey,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
    use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
    use std::collections::HashMap;

    const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";
    const BRAND: &str = "https://uri.etsi.org/ngsi-ld/default-context/brand";

    fn key(attribute_name: &str, dataset_id: Option<&str>) -> TemporalStreamKey {
        TemporalStreamKey {
            attribute_name: attribute_name.parse::<AttributeName>().expect("valid attribute name"),
            dataset_id: dataset_id.map(|raw| raw.parse::<Uri>().expect("valid dataset URI")),
        }
    }

    fn name_ids() -> HashMap<AttributeName, AttributeNameId> {
        HashMap::from([
            (SPEED.parse().expect("valid speed name"), AttributeNameId::from(1_i32)),
            (BRAND.parse().expect("valid brand name"), AttributeNameId::from(2_i32)),
        ])
    }

    #[test]
    fn repeated_observations_of_one_grain_bind_a_single_stream_row() {
        let streams = [key(SPEED, None), key(SPEED, None), key(SPEED, None)];
        let binds = stream_binds(&streams, &name_ids()).expect("known names");
        pretty_assertions::assert_eq!(binds.attribute_name_ids, vec![1_i32]);
        pretty_assertions::assert_eq!(binds.ids.len(), 1);
    }

    #[test]
    fn one_attribute_under_two_datasets_binds_two_streams() {
        let streams = [key(SPEED, None), key(SPEED, Some("urn:ngsi-ld:Dataset:a"))];
        let binds = stream_binds(&streams, &name_ids()).expect("known names");
        pretty_assertions::assert_eq!(binds.attribute_name_ids, vec![1_i32, 1_i32]);
        pretty_assertions::assert_eq!(binds.dataset_ids, vec![None, Some("urn:ngsi-ld:Dataset:a")]);
        assert_ne!(binds.ids[0], binds.ids[1]);
    }

    #[test]
    fn distinct_names_preserve_first_seen_order() {
        let streams = [key(BRAND, None), key(SPEED, None)];
        let names = distinct_attribute_names(&streams);
        pretty_assertions::assert_eq!(names.iter().map(AttributeName::as_str).collect::<Vec<_>>(), vec![BRAND, SPEED]);
    }

    #[test]
    fn the_stream_conflict_target_is_the_grain_the_catalogue_is_keyed_by() {
        assert!(STREAM_UPSERT_SQL.contains("ON CONFLICT (temporal_entity_key, attribute_name_id, dataset_id) DO NOTHING"));
    }

    #[test]
    fn a_repeated_write_never_moves_the_instant_the_evolution_began() {
        assert!(TEMPORAL_ENTITY_UPSERT_SQL.contains("SET modified_at = EXCLUDED.modified_at, types = EXCLUDED.types"));
        assert!(!TEMPORAL_ENTITY_UPSERT_SQL.contains("SET created_at"));
    }

    #[test]
    fn the_catalogue_names_no_legacy_relation_and_no_current_state_table() {
        for statement in [TEMPORAL_ENTITY_UPSERT_SQL, STREAM_UPSERT_SQL] {
            assert!(!statement.contains("temporal_entity_attributes"));
            assert!(!statement.contains(" entities"));
        }
    }
}
