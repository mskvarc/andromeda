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

use crate::{core::error::AndromedaError, repository::entity::params::PreparedEntityColumns};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Executor, Postgres, Transaction};

/// Postgres binary-COPY header signature (`PGCOPY\n\377\r\n\0`).
const COPY_HEADER_SIGNATURE: &[u8] = b"PGCOPY\n\xff\r\n\0";

/// Number of columns written per staged entity tuple.
const STAGE_ENTITIES_FIELD_COUNT: i16 = 8;

/// Offset (microseconds) between the Unix epoch and Postgres's
/// timestamptz epoch of 2000-01-01 00:00:00 UTC.
const POSTGRES_EPOCH_MICROS: i64 = 946_684_800_000_000;

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
        write_field_count(&mut buf);
        write_text(&mut buf, &columns.ids[index]);
        write_jsonb(&mut buf, &columns.types_json[index]);
        write_jsonb(&mut buf, &columns.scope_paths_json[index]);
        write_jsonb(&mut buf, &columns.payloads[index]);
        write_bytea_opt(&mut buf, columns.locations[index].as_ref().map(|wkb| wkb.to_vec()).as_deref());
        write_bytea_opt(&mut buf, columns.obs_spaces[index].as_ref().map(|wkb| wkb.to_vec()).as_deref());
        write_bytea_opt(&mut buf, columns.op_spaces[index].as_ref().map(|wkb| wkb.to_vec()).as_deref());
        write_timestamptz_opt(&mut buf, columns.expires_ats[index]);
    }
    write_trailer(&mut buf);
    buf
}

fn estimated_stream_bytes(columns: &PreparedEntityColumns) -> usize {
    // Rough guess: 1 KiB per row accounts for typical JSON payload size.
    19 + columns.ids.len() * 1024
}

fn write_header(buf: &mut Vec<u8>) {
    buf.extend_from_slice(COPY_HEADER_SIGNATURE);
    // 32-bit flags field — no OIDs, no special flags.
    buf.extend_from_slice(&0i32.to_be_bytes());
    // 32-bit header-extension length — zero.
    buf.extend_from_slice(&0i32.to_be_bytes());
}

fn write_field_count(buf: &mut Vec<u8>) {
    buf.extend_from_slice(&STAGE_ENTITIES_FIELD_COUNT.to_be_bytes());
}

fn write_trailer(buf: &mut Vec<u8>) {
    buf.extend_from_slice(&(-1i16).to_be_bytes());
}

fn write_text(buf: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    write_length(buf, bytes.len() as i32);
    buf.extend_from_slice(bytes);
}

fn write_jsonb(buf: &mut Vec<u8>, value: &Value) {
    let json = serde_json::to_vec(value).unwrap_or_else(|_| b"null".to_vec());
    let total_len = json.len() as i32 + 1; // +1 for the jsonb version byte
    write_length(buf, total_len);
    buf.push(0x01); // jsonb binary version
    buf.extend_from_slice(&json);
}

fn write_bytea_opt(buf: &mut Vec<u8>, value: Option<&[u8]>) {
    match value {
        Some(bytes) => {
            write_length(buf, bytes.len() as i32);
            buf.extend_from_slice(bytes);
        }
        None => write_null(buf),
    }
}

fn write_timestamptz_opt(buf: &mut Vec<u8>, value: Option<DateTime<Utc>>) {
    match value {
        Some(ts) => {
            let micros = ts.timestamp_micros().saturating_sub(POSTGRES_EPOCH_MICROS);
            write_length(buf, 8);
            buf.extend_from_slice(&micros.to_be_bytes());
        }
        None => write_null(buf),
    }
}

fn write_length(buf: &mut Vec<u8>, len: i32) {
    buf.extend_from_slice(&len.to_be_bytes());
}

fn write_null(buf: &mut Vec<u8>) {
    buf.extend_from_slice(&(-1i32).to_be_bytes());
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use crate::domain::geometry::Wkb;
    use bytes::Bytes;
    use chrono::TimeZone;

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
        columns.locations.push(Some(Wkb::from_postgis_bytes(Bytes::from_static(&[0x01, 0x02, 0x03]))));
        columns.locations.push(None);
        columns.obs_spaces.push(None);
        columns.obs_spaces.push(None);
        columns.op_spaces.push(None);
        columns.op_spaces.push(None);
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

    #[test]
    fn null_bytea_encodes_as_minus_one_length() {
        let mut buf = Vec::new();
        write_bytea_opt(&mut buf, None);
        pretty_assertions::assert_eq!(buf, vec![0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn jsonb_has_version_prefix() {
        let mut buf = Vec::new();
        write_jsonb(&mut buf, &serde_json::json!({"a": 1}));
        // first 4 bytes = length (big-endian), fifth = 0x01 version tag
        pretty_assertions::assert_eq!(buf[4], 0x01);
    }

    #[test]
    fn timestamptz_offset_against_postgres_epoch() {
        let mut buf = Vec::new();
        let epoch_2000 = Utc.timestamp_opt(946_684_800, 0).unwrap();
        write_timestamptz_opt(&mut buf, Some(epoch_2000));
        // 4-byte length prefix (8) + 8 zero bytes (midnight Jan 1 2000)
        pretty_assertions::assert_eq!(buf, vec![0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0]);
    }
}
