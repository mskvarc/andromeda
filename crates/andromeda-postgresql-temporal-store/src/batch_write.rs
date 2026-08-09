//! Batch writes for temporal catalogue, attribute history, and scope history.

use crate::{
    bulk_write_mode::BulkWriteMode,
    copy::insert_history_batch_via_copy_in_tx,
    error::TemporalStoreError,
    params::{ScopeHistoryEntry, TeaBatchEntry},
    repo::TemporalRepository,
    rows::TeaBatchResultRow,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use andromeda_postgresql_query::{attribute_name_id::AttributeNameId, history_batch_entry::HistoryBatchEntry, ltree::scope_to_ltree};
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};
use std::collections::{HashMap, HashSet, hash_map::Entry};

impl TemporalRepository {
    /// Bulk-upsert temporal entity attribute catalogue entries.
    pub async fn upsert_tea_batch_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        entries: &[TeaBatchEntry],
    ) -> Result<Vec<TeaBatchResultRow>, TemporalStoreError> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }
        let name_to_id = self
            .attribute_names
            .resolve_or_create(&distinct_attribute_names(entries))
            .await
            .map_err(|error| TemporalStoreError::AttributeLookup(error.to_string()))?;
        let binds = tea_batch_binds(entries, &name_to_id)?;
        let rows = sqlx::query(TEA_UPSERT_SQL)
            .bind(&binds.entity_ids)
            .bind(&binds.attribute_name_ids)
            .bind(&binds.attribute_types)
            .bind(&binds.dataset_ids)
            .bind(&binds.value_types)
            .fetch_all(&mut **tx)
            .await?;
        rows_to_tea_results(rows)
    }

    /// Bulk-insert attribute-history rows in the caller's transaction.
    pub async fn insert_history_batch_in_tx(&self, tx: &mut Transaction<'_, Postgres>, entries: &[HistoryBatchEntry]) -> Result<(), TemporalStoreError> {
        if entries.is_empty() {
            return Ok(());
        }
        run_history_dedup_deletes(tx, entries).await?;
        match self.bulk_write_mode {
            BulkWriteMode::Unnest => insert_history_batch_unnest_in_tx(tx, entries).await?,
            BulkWriteMode::CopyStaging => insert_history_batch_via_copy_in_tx(tx, entries).await?,
        }
        Ok(())
    }

    /// Bulk-insert scope-history observations in the caller's transaction.
    pub async fn insert_scope_history_batch_in_tx(&self, tx: &mut Transaction<'_, Postgres>, entries: &[ScopeHistoryEntry]) -> Result<(), TemporalStoreError> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut times = Vec::with_capacity(entries.len());
        let mut entity_ids = Vec::with_capacity(entries.len());
        let mut scope_paths = Vec::with_capacity(entries.len());
        let mut instance_ids = Vec::with_capacity(entries.len());
        for entry in entries {
            times.push(entry.time);
            entity_ids.push(entry.entity_id.as_str());
            scope_paths.push(scope_to_ltree(&entry.scope_path));
            instance_ids.push(entry.instance_id.as_str());
        }
        sqlx::query(
            r#"
            DELETE FROM scope_history sh
            USING UNNEST($1::text[], $2::text[]) AS d(entity_id, instance_id)
            WHERE sh.entity_id = d.entity_id AND sh.instance_id = d.instance_id
            "#,
        )
        .bind(&entity_ids)
        .bind(&instance_ids)
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO scope_history (time, entity_id, scope_path, instance_id)
            SELECT d.time, d.entity_id, d.scope_path::ltree, d.instance_id
            FROM UNNEST($1::timestamptz[], $2::text[], $3::text[], $4::text[])
                AS d(time, entity_id, scope_path, instance_id)
            "#,
        )
        .bind(&times)
        .bind(&entity_ids)
        .bind(&scope_paths)
        .bind(&instance_ids)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }
}

#[derive(Default)]
struct TeaBatchBinds<'a> {
    entity_ids: Vec<&'a str>,
    attribute_name_ids: Vec<i16>,
    attribute_types: Vec<&'static str>,
    dataset_ids: Vec<Option<&'a str>>,
    value_types: Vec<&'static str>,
}

fn distinct_attribute_names(entries: &[TeaBatchEntry]) -> Vec<AttributeName> {
    let mut seen = HashSet::with_capacity(entries.len());
    entries
        .iter()
        .filter(|entry| seen.insert(entry.attribute_name.as_str()))
        .map(|entry| entry.attribute_name.clone()) // clone: dictionary resolution owns each distinct name.
        .collect()
}

fn tea_batch_binds<'a>(entries: &'a [TeaBatchEntry], name_to_id: &HashMap<AttributeName, AttributeNameId>) -> Result<TeaBatchBinds<'a>, TemporalStoreError> {
    let mut binds = TeaBatchBinds {
        entity_ids: Vec::with_capacity(entries.len()),
        attribute_name_ids: Vec::with_capacity(entries.len()),
        attribute_types: Vec::with_capacity(entries.len()),
        dataset_ids: Vec::with_capacity(entries.len()),
        value_types: Vec::with_capacity(entries.len()),
    };
    let mut row_of: HashMap<(&'a Uri, AttributeNameId, Option<&'a Uri>), usize> = HashMap::with_capacity(entries.len());
    for entry in entries {
        let attribute_name_id = name_to_id
            .get(&entry.attribute_name)
            .ok_or_else(|| TemporalStoreError::AttributeLookup(format!("Attribute name '{}' not found in dictionary after resolve", entry.attribute_name)))?;
        match row_of.entry((&entry.entity_id, *attribute_name_id, entry.dataset_id.as_ref())) {
            Entry::Occupied(occupied) => {
                let row = *occupied.get();
                binds.attribute_types[row] = entry.attribute_type.short_name();
                binds.value_types[row] = entry.value_type.as_str();
            }
            Entry::Vacant(vacant) => {
                vacant.insert(binds.entity_ids.len());
                binds.entity_ids.push(entry.entity_id.as_str());
                binds.attribute_name_ids.push(attribute_name_id.get());
                binds.attribute_types.push(entry.attribute_type.short_name());
                binds.dataset_ids.push(entry.dataset_id.as_ref().map(Uri::as_str));
                binds.value_types.push(entry.value_type.as_str());
            }
        }
    }
    Ok(binds)
}

const TEA_UPSERT_SQL: &str = r#"
WITH input AS (
    SELECT d.entity_id, d.attribute_name_id, d.attribute_type, d.dataset_id, d.value_type
    FROM UNNEST($1::text[], $2::smallint[], $3::text[], $4::text[], $5::text[])
        AS d(entity_id, attribute_name_id, attribute_type, dataset_id, value_type)
),
upserted AS (
    INSERT INTO temporal_entity_attributes (entity_id, attribute_name_id, attribute_type, dataset_id, value_type)
    SELECT entity_id, attribute_name_id, attribute_type, dataset_id, value_type FROM input
    ON CONFLICT (entity_id, attribute_name_id, dataset_id) DO UPDATE
    SET attribute_type = EXCLUDED.attribute_type, value_type = EXCLUDED.value_type
    WHERE temporal_entity_attributes.attribute_type IS DISTINCT FROM EXCLUDED.attribute_type
       OR temporal_entity_attributes.value_type IS DISTINCT FROM EXCLUDED.value_type
    RETURNING id, entity_id, attribute_name_id, dataset_id
)
SELECT u.id, u.entity_id, an.name AS attribute_name, u.dataset_id
FROM upserted u
JOIN attribute_names an ON an.id = u.attribute_name_id
UNION ALL
SELECT tea.id, tea.entity_id, an.name AS attribute_name, tea.dataset_id
FROM input i
JOIN temporal_entity_attributes tea
    ON tea.entity_id = i.entity_id
   AND tea.attribute_name_id = i.attribute_name_id
   AND tea.dataset_id IS NOT DISTINCT FROM i.dataset_id
JOIN attribute_names an ON an.id = tea.attribute_name_id
WHERE NOT EXISTS (
    SELECT 1 FROM upserted u
    WHERE u.entity_id = i.entity_id
      AND u.attribute_name_id = i.attribute_name_id
      AND u.dataset_id IS NOT DISTINCT FROM i.dataset_id
)
"#;

fn rows_to_tea_results(rows: Vec<sqlx::postgres::PgRow>) -> Result<Vec<TeaBatchResultRow>, TemporalStoreError> {
    rows.into_iter()
        .map(|row| {
            let entity_id_raw: String = row.try_get("entity_id")?;
            let entity_id = entity_id_raw
                .parse()
                .map_err(|error| TemporalStoreError::Decode(format!("Invalid TEA entity_id '{entity_id_raw}': {error}")))?;
            let attribute_name_raw: String = row.try_get("attribute_name")?;
            let attribute_name = attribute_name_raw
                .parse()
                .map_err(|error| TemporalStoreError::Decode(format!("Invalid TEA attribute_name '{attribute_name_raw}': {error}")))?;
            let dataset_id = row
                .try_get::<Option<String>, _>("dataset_id")?
                .map(|raw| {
                    raw.parse()
                        .map_err(|error| TemporalStoreError::Decode(format!("Invalid TEA dataset_id '{raw}': {error}")))
                })
                .transpose()?;
            Ok(TeaBatchResultRow {
                id: row.try_get("id")?,
                entity_id,
                attribute_name,
                dataset_id,
            })
        })
        .collect()
}

async fn run_history_dedup_deletes(tx: &mut Transaction<'_, Postgres>, entries: &[HistoryBatchEntry]) -> Result<(), TemporalStoreError> {
    let mut any_client_instance = false;
    let mut tea_ids = Vec::with_capacity(entries.len());
    let mut instance_ids = Vec::with_capacity(entries.len());
    let mut observed_pairs = Vec::with_capacity(entries.len());
    for entry in entries {
        tea_ids.push(entry.tea_id);
        instance_ids.push(entry.instance_id.as_str());
        if let Some(observed_at) = entry.observed_at {
            observed_pairs.push((entry.tea_id, observed_at));
        }
        any_client_instance |= entry.instance_id_client_supplied;
    }
    if any_client_instance {
        sqlx::query(
            "DELETE FROM attribute_history ah USING UNNEST($1::uuid[], $2::text[]) AS d(tea_id, instance_id) WHERE ah.tea_id = d.tea_id AND ah.instance_id = d.instance_id",
        )
        .bind(&tea_ids)
        .bind(&instance_ids)
        .execute(&mut **tx)
        .await?;
    }
    if !observed_pairs.is_empty() {
        let (observed_tea_ids, observed_at): (Vec<uuid::Uuid>, Vec<DateTime<Utc>>) = observed_pairs.into_iter().unzip();
        sqlx::query(
            "DELETE FROM attribute_history ah USING UNNEST($1::uuid[], $2::timestamptz[]) AS d(tea_id, observed_at) WHERE ah.tea_id = d.tea_id AND ah.observed_at = d.observed_at",
        )
        .bind(&observed_tea_ids)
        .bind(&observed_at)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn insert_history_batch_unnest_in_tx(tx: &mut Transaction<'_, Postgres>, entries: &[HistoryBatchEntry]) -> Result<(), TemporalStoreError> {
    let mut times = Vec::with_capacity(entries.len());
    let mut tea_ids = Vec::with_capacity(entries.len());
    let mut value_numbers = Vec::with_capacity(entries.len());
    let mut value_strings = Vec::with_capacity(entries.len());
    let mut value_bools = Vec::with_capacity(entries.len());
    let mut value_geos = Vec::with_capacity(entries.len());
    let mut value_objects = Vec::with_capacity(entries.len());
    let mut value_datetimes = Vec::with_capacity(entries.len());
    let mut value_uris = Vec::with_capacity(entries.len());
    let mut instance_ids = Vec::with_capacity(entries.len());
    let mut observed_ats = Vec::with_capacity(entries.len());
    let mut metadatas = Vec::with_capacity(entries.len());
    let mut recorded_ats = Vec::with_capacity(entries.len());
    let mut intent_ids = Vec::with_capacity(entries.len());
    let mut intent_ordinals = Vec::with_capacity(entries.len());
    for entry in entries {
        times.push(entry.time);
        tea_ids.push(entry.tea_id);
        value_numbers.push(entry.value_number);
        value_strings.push(entry.value_string.as_deref());
        value_bools.push(entry.value_bool);
        value_geos.push(geometry_bytes(entry.value_geo.as_ref())?);
        value_objects.push(entry.value_object.as_ref());
        value_datetimes.push(entry.value_datetime);
        value_uris.push(entry.value_uri.as_ref().map(Uri::as_str));
        instance_ids.push(entry.instance_id.as_str());
        observed_ats.push(entry.observed_at);
        metadatas.push(entry.metadata.as_ref());
        recorded_ats.push(entry.recorded_at);
        intent_ids.push(entry.intent_id);
        intent_ordinals.push(entry.intent_ordinal);
    }
    sqlx::query(
        r#"
        INSERT INTO attribute_history
            (time, tea_id, value_number, value_string, value_bool,
             value_geo, value_object, value_datetime, value_uri,
             instance_id, observed_at, metadata, recorded_at, intent_id, intent_ordinal)
        SELECT
            d.time, d.tea_id, d.value_number, d.value_string, d.value_bool,
            CASE WHEN d.value_geo IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.value_geo)) ELSE NULL END,
            d.value_object, d.value_datetime, d.value_uri,
            d.instance_id, d.observed_at, d.metadata, d.recorded_at, d.intent_id, d.intent_ordinal
        FROM UNNEST(
            $1::timestamptz[], $2::uuid[], $3::double precision[], $4::text[], $5::boolean[],
            $6::bytea[], $7::jsonb[], $8::timestamptz[], $9::text[],
            $10::text[], $11::timestamptz[], $12::jsonb[], $13::timestamptz[], $14::uuid[], $15::integer[]
        ) AS d(time, tea_id, value_number, value_string, value_bool,
               value_geo, value_object, value_datetime, value_uri,
               instance_id, observed_at, metadata, recorded_at, intent_id, intent_ordinal)
        "#,
    )
    .bind(&times)
    .bind(&tea_ids)
    .bind(&value_numbers)
    .bind(&value_strings)
    .bind(&value_bools)
    .bind(&value_geos)
    .bind(&value_objects)
    .bind(&value_datetimes)
    .bind(&value_uris)
    .bind(&instance_ids)
    .bind(&observed_ats)
    .bind(&metadatas)
    .bind(&recorded_ats)
    .bind(&intent_ids)
    .bind(&intent_ordinals)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        batch_write::{TEA_UPSERT_SQL, distinct_attribute_names, tea_batch_binds},
        params::TeaBatchEntry,
        value_type::ValueType,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, uri::Uri};
    use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
    use std::collections::HashMap;

    const ENTITY: &str = "urn:ngsi-ld:Vehicle:A100";
    const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";
    const BRAND: &str = "https://uri.etsi.org/ngsi-ld/default-context/brand";

    fn tea_entry(attribute_name: &str, dataset_id: Option<&str>, value_type: ValueType) -> TeaBatchEntry {
        TeaBatchEntry {
            entity_id: ENTITY.parse::<Uri>().expect("valid entity URI"),
            attribute_name: attribute_name.parse::<AttributeName>().expect("valid attribute name"),
            attribute_type: AttributeType::Property,
            dataset_id: dataset_id.map(|raw| raw.parse::<Uri>().expect("valid dataset URI")),
            value_type,
        }
    }

    fn name_ids() -> HashMap<AttributeName, AttributeNameId> {
        HashMap::from([
            (SPEED.parse().expect("valid speed name"), AttributeNameId::from(1_i16)),
            (BRAND.parse().expect("valid brand name"), AttributeNameId::from(2_i16)),
        ])
    }

    #[test]
    fn conflict_target_matches_the_catalogue_deduplication_key() {
        assert!(TEA_UPSERT_SQL.contains("ON CONFLICT (entity_id, attribute_name_id, dataset_id) DO UPDATE"));
    }

    #[test]
    fn repeated_instances_bind_one_catalogue_row() {
        let entries = [
            tea_entry(SPEED, None, ValueType::Number),
            tea_entry(SPEED, None, ValueType::Number),
            tea_entry(SPEED, None, ValueType::Number),
        ];
        let binds = tea_batch_binds(&entries, &name_ids()).expect("known names");
        pretty_assertions::assert_eq!(binds.entity_ids, vec![ENTITY]);
        pretty_assertions::assert_eq!(binds.attribute_name_ids, vec![1_i16]);
    }

    #[test]
    fn distinct_names_preserve_first_seen_order() {
        let entries = [tea_entry(BRAND, None, ValueType::String), tea_entry(SPEED, None, ValueType::Number)];
        let names = distinct_attribute_names(&entries);
        pretty_assertions::assert_eq!(names.iter().map(AttributeName::as_str).collect::<Vec<_>>(), vec![BRAND, SPEED]);
    }
}
