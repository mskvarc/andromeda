//! Binary COPY path for bulk entity inserts.
//!
//! Drives the Postgres binary-format [`COPY`] protocol into a transaction-
//! scoped staging table (`stage_entities`), then materialises the staged
//! rows into `entities` via an `INSERT ... SELECT` that performs the
//! geometry projection and text[]/ltree[] decoding server-side.
//!
//! Binary COPY avoids the jsonb-array parse + per-row deserialisation
//! overhead of the `UNNEST($1::text[], $2::jsonb[], …)` path — the server
//! receives pre-typed tuples and deposits them into the staging heap
//! without any additional parsing.
//!
//! [`COPY`]: https://www.postgresql.org/docs/current/sql-copy.html

use crate::{
    core::error::AndromedaError,
    repository::{
        copy::{write_bytea_opt, write_field_count, write_header, write_jsonb, write_text, write_timestamptz_opt, write_trailer},
        entity::params::PreparedEntityColumns,
    },
};
use sqlx::{Executor, Postgres, Transaction};

/// Number of columns written per staged entity tuple.
const STAGE_ENTITIES_FIELD_COUNT: i16 = 8;

/// DDL for the per-transaction staging table. Dropped automatically on
/// transaction commit, so concurrent chunk writes on separate pool
/// connections each get their own fresh table.
const CREATE_STAGE_ENTITIES_SQL: &str = r#"
CREATE TEMP TABLE stage_entities (
    id text,
    types_json jsonb,
    scope_paths_json jsonb,
    payload jsonb,
    loc bytea,
    obs bytea,
    ops bytea,
    expires_at timestamptz
) ON COMMIT DROP
"#;

const COPY_INTO_STAGE_SQL: &str = "COPY stage_entities (id, types_json, scope_paths_json, payload, loc, obs, ops, expires_at) FROM STDIN WITH (FORMAT binary)";

const INSERT_FROM_STAGE_CREATE_SQL: &str = r#"
INSERT INTO entities (id, types, scope_paths, payload, location, observation_space, operation_space, expires_at)
SELECT
    s.id,
    ARRAY(SELECT jsonb_array_elements_text(s.types_json)),
    ARRAY(SELECT jsonb_array_elements_text(s.scope_paths_json)::ltree),
    s.payload,
    CASE WHEN s.loc IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(s.loc)) ELSE NULL END,
    CASE WHEN s.obs IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(s.obs)) ELSE NULL END,
    CASE WHEN s.ops IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(s.ops)) ELSE NULL END,
    s.expires_at
FROM stage_entities s
ON CONFLICT (id) DO NOTHING
RETURNING id
"#;

/// Bulk-insert via binary COPY into a staging table, then materialise into
/// `entities` inside the provided transaction. Returns the list of IDs
/// actually inserted (rows blocked by `ON CONFLICT (id) DO NOTHING` are
/// omitted).
pub async fn create_batch_via_copy_in_tx(tx: &mut Transaction<'_, Postgres>, columns: &PreparedEntityColumns) -> Result<Vec<String>, AndromedaError> {
    if columns.ids.is_empty() {
        return Ok(Vec::new());
    }

    tx.execute(CREATE_STAGE_ENTITIES_SQL).await?;

    let payload = encode_entities_copy_stream(columns);
    {
        let mut copy = tx.copy_in_raw(COPY_INTO_STAGE_SQL).await?;
        copy.send(payload.as_slice()).await?;
        copy.finish().await?;
    }

    let rows: Vec<(String,)> = sqlx::query_as(INSERT_FROM_STAGE_CREATE_SQL).fetch_all(&mut **tx).await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

fn encode_entities_copy_stream(columns: &PreparedEntityColumns) -> Vec<u8> {
    let mut buf = Vec::with_capacity(estimated_stream_bytes(columns));
    write_header(&mut buf);
    for index in 0..columns.ids.len() {
        write_field_count(&mut buf, STAGE_ENTITIES_FIELD_COUNT);
        write_text(&mut buf, &columns.ids[index]);
        write_jsonb(&mut buf, &columns.types_json[index]);
        write_jsonb(&mut buf, &columns.scope_paths_json[index]);
        write_jsonb(&mut buf, &columns.payloads[index]);
        write_bytea_opt(&mut buf, columns.locations[index].as_deref());
        write_bytea_opt(&mut buf, columns.obs_spaces[index].as_deref());
        write_bytea_opt(&mut buf, columns.op_spaces[index].as_deref());
        write_timestamptz_opt(&mut buf, columns.expires_ats[index]);
    }
    write_trailer(&mut buf);
    buf
}

const fn estimated_stream_bytes(columns: &PreparedEntityColumns) -> usize {
    // Rough guess: 1 KiB per row accounts for typical JSON payload size.
    19 + columns.ids.len() * 1024
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::copy::COPY_HEADER_SIGNATURE;
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use chrono::{TimeZone, Utc};

    fn sample_columns() -> PreparedEntityColumns {
        let mut columns = PreparedEntityColumns::with_capacity(2);
        columns.ids.push("urn:ngsi-ld:Building:1".to_string());
        columns.ids.push("urn:ngsi-ld:Building:2".to_string());
        columns.types_json.push(serde_json::json!(["https://example.com/Building"]));
        columns.types_json.push(serde_json::json!(["https://example.com/Building"]));
        columns.scope_paths_json.push(serde_json::json!(["root.a"]));
        columns.scope_paths_json.push(serde_json::json!([]));
        columns
            .push_payload(serde_json::json!({"@id": "urn:ngsi-ld:Building:1"}))
            .expect("payload serialises");
        columns
            .push_payload(serde_json::json!({"@id": "urn:ngsi-ld:Building:2"}))
            .expect("payload serialises");
        let point = Geometry::from_geojson_value(&serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0] })).expect("point parses");
        columns.push_geometries(Some(&point), None, None).expect("geometry encodes");
        columns.push_geometries(None, None, None).expect("geometry encodes");
        columns.expires_ats.push(Some(Utc.timestamp_opt(1_700_000_000, 0).unwrap()));
        columns.expires_ats.push(None);
        columns
    }

    #[test]
    fn stream_starts_with_signature_and_zero_flags() {
        let stream = encode_entities_copy_stream(&sample_columns());
        assert!(stream.starts_with(COPY_HEADER_SIGNATURE));
        let flags = &stream[11..15];
        pretty_assertions::assert_eq!(flags, &[0, 0, 0, 0]);
    }

    #[test]
    fn stream_terminates_with_trailer_sentinel() {
        let stream = encode_entities_copy_stream(&sample_columns());
        pretty_assertions::assert_eq!(&stream[stream.len() - 2..], &[0xff, 0xff]);
    }

    #[test]
    fn empty_input_still_emits_header_and_trailer() {
        let empty = PreparedEntityColumns::with_capacity(0);
        let stream = encode_entities_copy_stream(&empty);
        // 11 (signature) + 4 (flags) + 4 (ext len) + 2 (trailer)
        pretty_assertions::assert_eq!(stream.len(), 21);
    }
}
