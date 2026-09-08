//! Binary COPY path for bulk entity inserts.
//!
//! Drives the Postgres binary-format [`COPY`] protocol into a transaction-
//! scoped staging table (`stage_entities`), then materialises the staged
//! rows into the three current-state relations via one `INSERT ... SELECT`
//! statement that performs the geometry projection and JSONB-array decoding
//! server-side.
//!
//! Binary COPY avoids the jsonb-array parse + per-row deserialisation
//! overhead of the `UNNEST($1::text[], $2::jsonb[], …)` path — the server
//! receives pre-typed tuples and deposits them into the staging heap
//! without any additional parsing.
//!
//! [`COPY`]: https://www.postgresql.org/docs/current/sql-copy.html

use crate::{
    entity::{
        mapper::stored_written_entity,
        params::PreparedEntityColumns,
        types_present::{entity_write_failure, types_present},
        written_entity::WrittenEntity,
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_encoding::copy::{
    write_bytea_opt,
    write_field_count,
    write_header,
    write_jsonb,
    write_jsonb_text,
    write_text,
    write_timestamptz_opt,
    write_trailer,
};
use sqlx::{Executor, Postgres, Transaction};

/// Number of columns written per staged entity tuple.
const STAGE_ENTITIES_FIELD_COUNT: i16 = 11;

/// DDL for the per-transaction staging table. Dropped automatically on
/// transaction commit, so concurrent chunk writes on separate pool
/// connections each get their own fresh table.
const CREATE_STAGE_ENTITIES_SQL: &str = r"
CREATE TEMP TABLE stage_entities (
    id text,
    type_ids_json jsonb,
    attribute_name_ids_json jsonb,
    scope_paths_json jsonb,
    payload jsonb,
    loc bytea,
    obs bytea,
    ops bytea,
    loc_expires_at timestamptz,
    expires_at timestamptz,
    min_attribute_expires_at timestamptz
) ON COMMIT DROP
";

/// Binary COPY statement targeting the staging table.
const COPY_INTO_STAGE_SQL: &str = "COPY stage_entities (id, type_ids_json, attribute_name_ids_json, scope_paths_json, payload, loc, obs, ops, loc_expires_at, expires_at, min_attribute_expires_at) FROM STDIN WITH (FORMAT binary)";

/// Materialise staged rows into the three entity relations: the same
/// projection and the same three inserts the `UNNEST` path performs, read from
/// the staging table instead of parallel arrays.
const INSERT_FROM_STAGE_CREATE_SQL: &str = r"
WITH proj AS (
    SELECT
        s.id,
        ARRAY(SELECT jsonb_array_elements_text(s.type_ids_json)::integer) AS type_ids,
        ARRAY(SELECT jsonb_array_elements_text(s.attribute_name_ids_json)::integer) AS attribute_name_ids,
        ARRAY(SELECT jsonb_array_elements_text(s.scope_paths_json)::ltree) AS scope_paths,
        s.payload,
        CASE WHEN s.loc IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(s.loc)) ELSE NULL END AS location,
        CASE WHEN s.obs IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(s.obs)) ELSE NULL END AS observation_space,
        CASE WHEN s.ops IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(s.ops)) ELSE NULL END AS operation_space,
        s.loc_expires_at AS location_expires_at,
        s.expires_at,
        s.min_attribute_expires_at
    FROM stage_entities s
),
ins AS (
    INSERT INTO entities (context_space_key, id, expires_at, min_attribute_expires_at)
    SELECT $1, proj.id, proj.expires_at, proj.min_attribute_expires_at FROM proj
    ON CONFLICT (context_space_key, id) DO NOTHING
    RETURNING entity_key, id
),
docs AS (
    INSERT INTO entity_documents (context_space_key, entity_key, payload)
    SELECT $1, ins.entity_key, proj.payload FROM ins JOIN proj ON proj.id = ins.id
),
srch AS (
    INSERT INTO entity_search (context_space_key, entity_key, location_expires_at, type_ids, attribute_name_ids, scope_paths, location, observation_space, operation_space)
    SELECT $1, ins.entity_key, proj.location_expires_at, proj.type_ids, proj.attribute_name_ids, proj.scope_paths, proj.location, proj.observation_space, proj.operation_space
    FROM ins JOIN proj ON proj.id = ins.id
)
SELECT entity_key, id FROM ins
";

/// Bulk-insert via binary COPY into a staging table, then materialise into
/// the entity relations inside the provided transaction. Returns the rows
/// actually inserted (rows blocked by `ON CONFLICT (id) DO NOTHING` are
/// omitted), each carrying the surrogate key the projection written in the
/// same transaction is keyed on.
///
/// The inserts name their target columns explicitly, so the physical column
/// order of the relations is free to change without this statement moving.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the batch names an entity type the
/// dictionary has not interned, when the staging table cannot be created, when
/// a staged value is too long for the binary COPY length prefix or does not
/// serialise, when the binary COPY stream is rejected, when the materialising
/// insert fails, or when a returned surrogate key or identifier is not a valid
/// written entity.
pub async fn create_batch_via_copy_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    columns: &PreparedEntityColumns,
) -> Result<Vec<WrittenEntity>, ContextStoreError> {
    if columns.ids.is_empty() {
        return Ok(Vec::new());
    }
    types_present(columns)?;

    tx.execute(CREATE_STAGE_ENTITIES_SQL).await?;

    let payload = encode_entities_copy_stream(columns)?;
    {
        let mut copy = tx.copy_in_raw(COPY_INTO_STAGE_SQL).await?;
        copy.send(payload.as_slice()).await?;
        copy.finish().await?;
    }

    let rows: Vec<(i64, String)> = sqlx::query_as(INSERT_FROM_STAGE_CREATE_SQL)
        .bind(target.key().as_i64())
        .fetch_all(&mut **tx)
        .await
        .map_err(entity_write_failure)?;
    rows.iter().map(|(key, id)| stored_written_entity(*key, id)).collect()
}

/// Encode a set of entity columns into the Postgres binary COPY format.
///
/// # Errors
///
/// Returns [`ContextStoreError::CopyField`] when a staged value is longer than
/// the format's 32-bit field prefix can announce, or when a staged JSON value
/// does not serialise.
fn encode_entities_copy_stream(columns: &PreparedEntityColumns) -> Result<Vec<u8>, ContextStoreError> {
    let mut buf = Vec::with_capacity(estimated_stream_bytes(columns));
    write_header(&mut buf);
    for index in 0..columns.ids.len() {
        write_field_count(&mut buf, STAGE_ENTITIES_FIELD_COUNT);
        write_text(&mut buf, columns.ids[index].as_str())?;
        write_jsonb(&mut buf, &columns.type_ids_json[index])?;
        write_jsonb(&mut buf, &columns.attribute_name_ids_json[index])?;
        write_jsonb(&mut buf, &columns.scope_paths_json[index])?;
        write_jsonb_text(&mut buf, &columns.payload_json[index])?;
        write_bytea_opt(&mut buf, columns.locations[index].as_deref())?;
        write_bytea_opt(&mut buf, columns.obs_spaces[index].as_deref())?;
        write_bytea_opt(&mut buf, columns.op_spaces[index].as_deref())?;
        write_timestamptz_opt(&mut buf, columns.location_expires_ats[index]);
        write_timestamptz_opt(&mut buf, columns.expires_ats[index]);
        write_timestamptz_opt(&mut buf, columns.min_attribute_expires_ats[index]);
    }
    write_trailer(&mut buf);
    Ok(buf)
}

/// Rough byte estimate for pre-sizing the COPY buffer.
const fn estimated_stream_bytes(columns: &PreparedEntityColumns) -> usize {
    // Rough guess: 1 KiB per row accounts for typical JSON payload size.
    19 + columns.ids.len() * 1024
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_postgresql_encoding::copy::COPY_HEADER_SIGNATURE;
    use chrono::{TimeZone, Utc};

    /// Build a two-row sample for encoding tests.
    fn sample_columns() -> PreparedEntityColumns {
        let mut columns = PreparedEntityColumns::with_capacity(2);
        columns.ids.push("urn:ngsi-ld:Building:1".parse().expect("entity URI"));
        columns.ids.push("urn:ngsi-ld:Building:2".parse().expect("entity URI"));
        columns.type_ids_json.push(serde_json::json!([7]));
        columns.type_ids_json.push(serde_json::json!([7]));
        columns.attribute_name_ids_json.push(serde_json::json!([3, 4]));
        columns.attribute_name_ids_json.push(serde_json::json!([]));
        columns.scope_paths_json.push(serde_json::json!(["root.a"]));
        columns.scope_paths_json.push(serde_json::json!([]));
        columns
            .push_payload(&serde_json::json!({"@id": "urn:ngsi-ld:Building:1"}))
            .expect("payload serialises");
        columns
            .push_payload(&serde_json::json!({"@id": "urn:ngsi-ld:Building:2"}))
            .expect("payload serialises");
        let point = Geometry::from_geojson_value(&serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0] })).expect("point parses");
        columns.push_geometries(Some(&point), None, None).expect("geometry encodes");
        columns.push_geometries(None, None, None).expect("geometry encodes");
        columns.location_expires_ats.push(None);
        columns.location_expires_ats.push(None);
        columns.expires_ats.push(Some(Utc.timestamp_opt(1_700_000_000, 0).unwrap()));
        columns.expires_ats.push(None);
        columns.min_attribute_expires_ats.push(None);
        columns.min_attribute_expires_ats.push(Some(Utc.timestamp_opt(1_700_000_000, 0).unwrap()));
        columns
    }

    #[test]
    fn stream_starts_with_signature_and_zero_flags() {
        let stream = encode_entities_copy_stream(&sample_columns()).expect("the sample columns encode");
        assert!(stream.starts_with(COPY_HEADER_SIGNATURE));
        let flags = &stream[11..15];
        pretty_assertions::assert_eq!(flags, &[0, 0, 0, 0]);
    }

    #[test]
    fn stream_terminates_with_trailer_sentinel() {
        let stream = encode_entities_copy_stream(&sample_columns()).expect("the sample columns encode");
        pretty_assertions::assert_eq!(&stream[stream.len() - 2..], &[0xff, 0xff]);
    }

    #[test]
    fn every_row_carries_the_eleven_staged_fields() {
        let stream = encode_entities_copy_stream(&sample_columns()).expect("the sample columns encode");
        // The first tuple's field count follows the 19-byte header.
        let field_count = u8::try_from(STAGE_ENTITIES_FIELD_COUNT).expect("the staged field count fits one byte");
        pretty_assertions::assert_eq!(&stream[19..21], &[0, field_count]);
    }

    #[test]
    fn empty_input_still_emits_header_and_trailer() {
        let empty = PreparedEntityColumns::with_capacity(0);
        let stream = encode_entities_copy_stream(&empty).expect("an empty column set encodes");
        // 11 (signature) + 4 (flags) + 4 (ext len) + 2 (trailer)
        pretty_assertions::assert_eq!(stream.len(), 21);
    }

    #[test]
    fn the_staged_columns_and_the_copy_column_list_agree() {
        for column in [
            "id",
            "type_ids_json",
            "attribute_name_ids_json",
            "scope_paths_json",
            "payload",
            "loc",
            "obs",
            "ops",
            "loc_expires_at",
            "expires_at",
            "min_attribute_expires_at",
        ] {
            assert!(CREATE_STAGE_ENTITIES_SQL.contains(column), "{column}");
            assert!(COPY_INTO_STAGE_SQL.contains(column), "{column}");
        }
        assert!(INSERT_FROM_STAGE_CREATE_SQL.contains("INSERT INTO entities (context_space_key, id, expires_at, min_attribute_expires_at)"));
        assert!(INSERT_FROM_STAGE_CREATE_SQL.contains("INSERT INTO entity_documents (context_space_key, entity_key, payload)"));
        assert!(
            INSERT_FROM_STAGE_CREATE_SQL
                .contains("INSERT INTO entity_search (context_space_key, entity_key, location_expires_at, type_ids, attribute_name_ids")
        );
    }
}
