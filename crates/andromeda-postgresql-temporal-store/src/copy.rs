//! Binary COPY path for bulk attribute-history inserts.
//!
//! Mirrors the staging-table pattern from the entity COPY path: COPY a
//! transaction-scoped `stage_attribute_history` table from STDIN in binary
//! format, then materialise into `attribute_history` via `INSERT ... SELECT`
//! so the geometry projection (`ST_GeomFromEWKB`) runs server-side and the
//! per-row payload parsing of the prior `UNNEST(..)` path is gone.

use crate::error::TemporalStoreError;
use andromeda_postgresql_encoding::{
    copy::{
        write_bool_opt,
        write_bytea_opt,
        write_f64_opt,
        write_field_count,
        write_header,
        write_jsonb_opt,
        write_text,
        write_text_opt,
        write_timestamptz,
        write_timestamptz_opt,
        write_trailer,
        write_uuid,
    },
    ewkb_binding::geometry_bytes,
};
use andromeda_postgresql_query::history_batch_entry::HistoryBatchEntry;
use sqlx::{Executor, Postgres, Transaction};

/// Number of columns written per staged history tuple.
const STAGE_ATTRIBUTE_HISTORY_FIELD_COUNT: i16 = 13;

const CREATE_STAGE_ATTRIBUTE_HISTORY_SQL: &str = r#"
CREATE TEMP TABLE stage_attribute_history (
    time            timestamptz NOT NULL,
    tea_id          uuid NOT NULL,
    value_number    double precision,
    value_string    text,
    value_bool      boolean,
    value_geo       bytea,
    value_object    jsonb,
    value_datetime  timestamptz,
    value_uri       text,
    instance_id     text NOT NULL,
    observed_at     timestamptz,
    metadata        jsonb,
    recorded_at     timestamptz NOT NULL
) ON COMMIT DROP
"#;

const COPY_INTO_STAGE_SQL: &str = "COPY stage_attribute_history (time, tea_id, value_number, value_string, value_bool, \
     value_geo, value_object, value_datetime, value_uri, instance_id, observed_at, metadata, recorded_at) \
     FROM STDIN WITH (FORMAT binary)";

const INSERT_FROM_STAGE_SQL: &str = r#"
INSERT INTO attribute_history (
    time, tea_id, value_number, value_string, value_bool,
    value_geo, value_object, value_datetime, value_uri,
    instance_id, observed_at, metadata, recorded_at
)
SELECT
    s.time, s.tea_id, s.value_number, s.value_string, s.value_bool,
    CASE WHEN s.value_geo IS NOT NULL
         THEN ST_MakeValid(ST_GeomFromEWKB(s.value_geo))
         ELSE NULL END,
    s.value_object, s.value_datetime, s.value_uri,
    s.instance_id, s.observed_at, s.metadata, s.recorded_at
FROM stage_attribute_history s
"#;

/// Bulk-insert via binary COPY into a per-transaction staging table,
/// then materialise into `attribute_history`. The caller is responsible
/// for running any dedup `DELETE`s (instance-id pass + observed-at pass)
/// against `attribute_history` *before* this call so the staged INSERT
/// does not collide with already-present rows.
pub(crate) async fn insert_history_batch_via_copy_in_tx(tx: &mut Transaction<'_, Postgres>, entries: &[HistoryBatchEntry]) -> Result<(), TemporalStoreError> {
    if entries.is_empty() {
        return Ok(());
    }

    tx.execute(CREATE_STAGE_ATTRIBUTE_HISTORY_SQL).await?;

    let payload = encode_history_copy_stream(entries)?;
    {
        let mut copy = tx.copy_in_raw(COPY_INTO_STAGE_SQL).await?;
        copy.send(payload.as_slice()).await?;
        copy.finish().await?;
    }

    sqlx::query(INSERT_FROM_STAGE_SQL).execute(&mut **tx).await?;
    Ok(())
}

pub(crate) fn encode_history_copy_stream(entries: &[HistoryBatchEntry]) -> Result<Vec<u8>, TemporalStoreError> {
    let mut buf = Vec::with_capacity(estimated_stream_bytes(entries.len()));
    write_header(&mut buf);
    for entry in entries {
        write_field_count(&mut buf, STAGE_ATTRIBUTE_HISTORY_FIELD_COUNT);
        write_timestamptz(&mut buf, entry.time);
        write_uuid(&mut buf, entry.tea_id);
        write_f64_opt(&mut buf, entry.value_number);
        write_text_opt(&mut buf, entry.value_string.as_deref());
        write_bool_opt(&mut buf, entry.value_bool);
        write_bytea_opt(&mut buf, geometry_bytes(entry.value_geo.as_ref())?.as_deref());
        write_jsonb_opt(&mut buf, entry.value_object.as_ref());
        write_timestamptz_opt(&mut buf, entry.value_datetime);
        write_text_opt(&mut buf, entry.value_uri.as_deref());
        write_text(&mut buf, entry.instance_id.as_str());
        write_timestamptz_opt(&mut buf, entry.observed_at);
        write_jsonb_opt(&mut buf, entry.metadata.as_ref());
        write_timestamptz(&mut buf, entry.recorded_at);
    }
    write_trailer(&mut buf);
    Ok(buf)
}

const fn estimated_stream_bytes(rows: usize) -> usize {
    // Header (19) + trailer (2) + ~256 B per row (mixed nulls + small
    // payload + 12 length prefixes).
    21 + rows * 256
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_postgresql_encoding::copy::COPY_HEADER_SIGNATURE;
    use chrono::{TimeZone, Utc};

    fn make_entry(tea_id: uuid::Uuid, instance_id: &str) -> HistoryBatchEntry {
        HistoryBatchEntry {
            time: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
            tea_id,
            value_number: None,
            value_string: None,
            value_bool: None,
            value_geo: None,
            value_object: None,
            value_datetime: None,
            value_uri: None,
            instance_id: instance_id.to_string(),
            observed_at: None,
            metadata: None,
            recorded_at: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
            instance_id_client_supplied: false,
        }
    }

    #[test]
    fn binary_copy_buffer_starts_with_pgcopy_signature() {
        let entries = [make_entry(uuid::Uuid::nil(), "urn:ngsi-ld:attribute:instance:0")];
        let buf = encode_history_copy_stream(&entries).expect("copy stream encodes");
        assert!(buf.starts_with(COPY_HEADER_SIGNATURE));
    }

    #[test]
    fn binary_copy_buffer_ends_with_neg_one_terminator() {
        let entries = [make_entry(uuid::Uuid::nil(), "urn:ngsi-ld:attribute:instance:0")];
        let buf = encode_history_copy_stream(&entries).expect("copy stream encodes");
        assert_eq!(&buf[buf.len() - 2..], &[0xff, 0xff]);
    }

    #[test]
    fn binary_copy_emits_field_count_13_per_tuple() {
        let entries = [
            make_entry(uuid::Uuid::nil(), "i0"),
            make_entry(uuid::Uuid::nil(), "i1"),
            make_entry(uuid::Uuid::nil(), "i2"),
        ];
        let buf = encode_history_copy_stream(&entries).expect("copy stream encodes");
        // Header is 19 bytes; first tuple field count starts at byte 19.
        assert_eq!(&buf[19..21], &[0x00, 0x0d]);
    }

    #[test]
    fn binary_copy_encodes_recorded_at_as_trailing_timestamptz() {
        let entries = [make_entry(uuid::Uuid::nil(), "i")];
        let buf = encode_history_copy_stream(&entries).expect("copy stream encodes");
        // recorded_at is the last field: the stream ends with the 2-byte
        // trailer preceded by the 4-byte length prefix and 8-byte payload.
        let offset = buf.len() - 2 - 12;
        assert_eq!(&buf[offset..offset + 4], &[0, 0, 0, 8]);
        let micros = i64::from_be_bytes([
            buf[offset + 4],
            buf[offset + 5],
            buf[offset + 6],
            buf[offset + 7],
            buf[offset + 8],
            buf[offset + 9],
            buf[offset + 10],
            buf[offset + 11],
        ]);
        let expected = 1_700_000_000_i64.saturating_mul(1_000_000) - 946_684_800_000_000_i64;
        assert_eq!(micros, expected);
    }

    #[test]
    fn binary_copy_encodes_uuid_as_16_raw_bytes() {
        let uid = uuid::Uuid::from_bytes([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
        let entries = [make_entry(uid, "i")];
        let buf = encode_history_copy_stream(&entries).expect("copy stream encodes");
        // Header (19) + field count (2) + time field (4 + 8) = offset 33 for tea_id length prefix.
        let tea_offset = 19 + 2 + 4 + 8;
        assert_eq!(&buf[tea_offset..tea_offset + 4], &[0, 0, 0, 16]);
        assert_eq!(&buf[tea_offset + 4..tea_offset + 20], uid.as_bytes());
    }

    #[test]
    fn binary_copy_encodes_optional_f64_value_number() {
        let mut entry = make_entry(uuid::Uuid::nil(), "i");
        entry.value_number = Some(2.75);
        let buf = encode_history_copy_stream(&[entry]).expect("copy stream encodes");
        // After header (19) + field_count (2) + time (4+8) + tea_id (4+16) = 53.
        let offset = 19 + 2 + 12 + 20;
        assert_eq!(&buf[offset..offset + 4], &[0, 0, 0, 8]);
        assert_eq!(&buf[offset + 4..offset + 12], &2.75f64.to_be_bytes());
    }

    #[test]
    fn binary_copy_encodes_none_as_neg_one_length() {
        let entry = make_entry(uuid::Uuid::nil(), "i");
        let buf = encode_history_copy_stream(&[entry]).expect("copy stream encodes");
        let offset = 19 + 2 + 12 + 20;
        assert_eq!(&buf[offset..offset + 4], &[0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn binary_copy_encodes_optional_bool_as_single_byte() {
        let mut entry_t = make_entry(uuid::Uuid::nil(), "i");
        entry_t.value_bool = Some(true);
        let buf_t = encode_history_copy_stream(&[entry_t]).expect("copy stream encodes");
        // Offsets: 19 (header) + 2 (fc) + 12 (time) + 20 (uuid) + 4 (null number) + 4 (null string) = 61.
        let offset = 19 + 2 + 12 + 20 + 4 + 4;
        assert_eq!(&buf_t[offset..offset + 4], &[0, 0, 0, 1]);
        assert_eq!(buf_t[offset + 4], 0x01);

        let mut entry_f = make_entry(uuid::Uuid::nil(), "i");
        entry_f.value_bool = Some(false);
        let buf_f = encode_history_copy_stream(&[entry_f]).expect("copy stream encodes");
        assert_eq!(&buf_f[offset..offset + 4], &[0, 0, 0, 1]);
        assert_eq!(buf_f[offset + 4], 0x00);
    }

    #[test]
    fn binary_copy_encodes_jsonb_with_version_byte() {
        let mut entry = make_entry(uuid::Uuid::nil(), "i");
        entry.value_object = Some(serde_json::json!({"a": 1}));
        let buf = encode_history_copy_stream(&[entry]).expect("copy stream encodes");
        // Offsets: 19 + 2 + 12 + 20 + 4 + 4 + 4 + 4 (geo null) = 69 -- that's value_object length prefix.
        let offset = 19 + 2 + 12 + 20 + 4 + 4 + 4 + 4;
        // 4-byte length, then version byte 0x01, then UTF-8 JSON.
        let payload_len = i32::from_be_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]]);
        assert_eq!(payload_len, br#"{"a":1}"#.len() as i32 + 1);
        assert_eq!(buf[offset + 4], 0x01);
        assert_eq!(&buf[offset + 5..offset + 5 + 7], br#"{"a":1}"#);
    }

    #[test]
    fn binary_copy_encodes_timestamptz_microseconds_since_2000() {
        let entries = [make_entry(uuid::Uuid::nil(), "i")];
        let buf = encode_history_copy_stream(&entries).expect("copy stream encodes");
        // Time field directly after field-count: header (19) + fc (2) = 21.
        let len = i32::from_be_bytes([buf[21], buf[22], buf[23], buf[24]]);
        assert_eq!(len, 8);
        let micros = i64::from_be_bytes([buf[25], buf[26], buf[27], buf[28], buf[29], buf[30], buf[31], buf[32]]);
        let expected = 1_700_000_000_i64.saturating_mul(1_000_000) - 946_684_800_000_000_i64;
        assert_eq!(micros, expected);
    }

    #[test]
    fn binary_copy_text_field_encodes_utf8_with_length_prefix() {
        let id = "urn:ngsi-ld:attribute:instance:client";
        let entries = [make_entry(uuid::Uuid::nil(), id)];
        let buf = encode_history_copy_stream(&entries).expect("copy stream encodes");
        // Locate instance_id. Layout up to instance_id length prefix:
        // header(19) + field_count(2) + time(4+8) + tea_id(4+16)
        //   + 7 NULL fields (value_number, value_string, value_bool,
        //     value_geo, value_object, value_datetime, value_uri) of 4 B each.
        let offset = 19 + 2 + 12 + 20 + 4 * 7;
        let len = i32::from_be_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]]);
        assert_eq!(len as usize, id.len());
        assert_eq!(&buf[offset + 4..offset + 4 + id.len()], id.as_bytes());
    }
}
