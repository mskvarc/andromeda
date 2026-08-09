//! Shared encoders for the Postgres binary `COPY ... FROM STDIN` protocol.
//!
//! The format is documented at
//! <https://www.postgresql.org/docs/current/sql-copy.html#id-1.9.3.55.9.4.6>.
//! Each tuple is preceded by a 16-bit field count; each field is either
//! `-1` (NULL, signalled as `i32 = -1`) or `len` followed by `len` bytes
//! of pre-typed payload. Encoders here own the per-type payload layout
//! so callers only need to drive tuple structure.

use chrono::{DateTime, Utc};
use serde_json::Value;

/// Postgres binary-COPY header signature (`PGCOPY\n\377\r\n\0`).
pub const COPY_HEADER_SIGNATURE: &[u8] = b"PGCOPY\n\xff\r\n\0";

/// Offset (microseconds) between the Unix epoch and Postgres's
/// timestamptz epoch of 2000-01-01 00:00:00 UTC.
pub const POSTGRES_EPOCH_MICROS: i64 = 946_684_800_000_000;

/// Append the binary-COPY stream header: signature, zero flags, zero
/// extension length.
pub fn write_header(buf: &mut Vec<u8>) {
    buf.extend_from_slice(COPY_HEADER_SIGNATURE);
    buf.extend_from_slice(&0i32.to_be_bytes());
    buf.extend_from_slice(&0i32.to_be_bytes());
}

/// Append the trailer sentinel (`-1` as i16) that marks end-of-stream.
pub fn write_trailer(buf: &mut Vec<u8>) {
    buf.extend_from_slice(&(-1i16).to_be_bytes());
}

/// Append the per-tuple field count (i16 big-endian).
pub fn write_field_count(buf: &mut Vec<u8>, count: i16) {
    buf.extend_from_slice(&count.to_be_bytes());
}

/// Append a non-null `text` field as a length-prefixed UTF-8 payload.
pub fn write_text(buf: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    write_length(buf, bytes.len() as i32);
    buf.extend_from_slice(bytes);
}

/// Append an optional `text` field, encoding `None` as a NULL.
pub fn write_text_opt(buf: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(text) => write_text(buf, text),
        None => write_null(buf),
    }
}

/// Append a `jsonb` field — length prefix, version byte 0x01, then the
/// serialised JSON bytes.
pub fn write_jsonb(buf: &mut Vec<u8>, value: &Value) {
    let json = serde_json::to_vec(value).unwrap_or_else(|_| b"null".to_vec());
    let total_len = json.len() as i32 + 1;
    write_length(buf, total_len);
    buf.push(0x01);
    buf.extend_from_slice(&json);
}

/// Append an optional `jsonb` field, encoding `None` as a NULL.
pub fn write_jsonb_opt(buf: &mut Vec<u8>, value: Option<&Value>) {
    match value {
        Some(json) => write_jsonb(buf, json),
        None => write_null(buf),
    }
}

/// Append an optional `bytea` field, encoding `None` as a NULL.
pub fn write_bytea_opt(buf: &mut Vec<u8>, value: Option<&[u8]>) {
    match value {
        Some(bytes) => {
            write_length(buf, bytes.len() as i32);
            buf.extend_from_slice(bytes);
        }
        None => write_null(buf),
    }
}

/// Append a non-null `timestamptz` field as 8 bytes of microseconds
/// since the Postgres epoch (2000-01-01 UTC).
pub fn write_timestamptz(buf: &mut Vec<u8>, value: DateTime<Utc>) {
    let micros = value.timestamp_micros().saturating_sub(POSTGRES_EPOCH_MICROS);
    write_length(buf, 8);
    buf.extend_from_slice(&micros.to_be_bytes());
}

/// Append an optional `timestamptz` field, encoding `None` as a NULL.
pub fn write_timestamptz_opt(buf: &mut Vec<u8>, value: Option<DateTime<Utc>>) {
    match value {
        Some(ts) => write_timestamptz(buf, ts),
        None => write_null(buf),
    }
}

/// Append a non-null `uuid` field as 16 raw bytes.
pub fn write_uuid(buf: &mut Vec<u8>, value: uuid::Uuid) {
    write_length(buf, 16);
    buf.extend_from_slice(value.as_bytes());
}

/// Append an optional `uuid` field, encoding `None` as a NULL.
pub fn write_uuid_opt(buf: &mut Vec<u8>, value: Option<uuid::Uuid>) {
    match value {
        Some(value) => write_uuid(buf, value),
        None => write_null(buf),
    }
}

/// Append an optional 32-bit integer field in network byte order.
pub fn write_i32_opt(buf: &mut Vec<u8>, value: Option<i32>) {
    match value {
        Some(value) => {
            write_length(buf, 4);
            buf.extend_from_slice(&value.to_be_bytes());
        }
        None => write_null(buf),
    }
}

/// Append an optional `double precision` (f64) field as 8 bytes IEEE 754
/// big-endian, encoding `None` as a NULL.
pub fn write_f64_opt(buf: &mut Vec<u8>, value: Option<f64>) {
    match value {
        Some(n) => {
            write_length(buf, 8);
            buf.extend_from_slice(&n.to_be_bytes());
        }
        None => write_null(buf),
    }
}

/// Append an optional `boolean` field as a single byte (0x00/0x01),
/// encoding `None` as a NULL.
pub fn write_bool_opt(buf: &mut Vec<u8>, value: Option<bool>) {
    match value {
        Some(b) => {
            write_length(buf, 1);
            buf.push(u8::from(b));
        }
        None => write_null(buf),
    }
}

/// Append a 4-byte big-endian length prefix.
pub fn write_length(buf: &mut Vec<u8>, len: i32) {
    buf.extend_from_slice(&len.to_be_bytes());
}

/// Append a NULL marker (length = -1 as i32 big-endian).
pub fn write_null(buf: &mut Vec<u8>) {
    buf.extend_from_slice(&(-1i32).to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn header_layout_matches_pgcopy_spec() {
        let mut buf = Vec::new();
        write_header(&mut buf);
        assert_eq!(&buf[..11], COPY_HEADER_SIGNATURE);
        assert_eq!(&buf[11..15], &[0, 0, 0, 0]);
        assert_eq!(&buf[15..19], &[0, 0, 0, 0]);
    }

    #[test]
    fn trailer_is_neg_one_i16() {
        let mut buf = Vec::new();
        write_trailer(&mut buf);
        assert_eq!(buf, vec![0xff, 0xff]);
    }

    #[test]
    fn field_count_encoded_as_i16_be() {
        let mut buf = Vec::new();
        write_field_count(&mut buf, 12);
        assert_eq!(buf, vec![0x00, 0x0c]);
    }

    #[test]
    fn null_marker_is_neg_one_i32() {
        let mut buf = Vec::new();
        write_null(&mut buf);
        assert_eq!(buf, vec![0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn text_field_carries_length_prefix() {
        let mut buf = Vec::new();
        write_text(&mut buf, "abc");
        assert_eq!(buf, vec![0, 0, 0, 3, b'a', b'b', b'c']);
    }

    #[test]
    fn text_opt_none_is_null() {
        let mut buf = Vec::new();
        write_text_opt(&mut buf, None);
        assert_eq!(buf, vec![0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn jsonb_prepends_version_byte_after_length() {
        let mut buf = Vec::new();
        write_jsonb(&mut buf, &serde_json::json!({"a": 1}));
        assert_eq!(buf[4], 0x01);
        let payload = &buf[5..];
        assert_eq!(payload, br#"{"a":1}"#);
    }

    #[test]
    fn jsonb_opt_none_is_null() {
        let mut buf = Vec::new();
        write_jsonb_opt(&mut buf, None);
        assert_eq!(buf, vec![0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn timestamptz_offset_is_relative_to_postgres_epoch() {
        let mut buf = Vec::new();
        let epoch_2000 = Utc.timestamp_opt(946_684_800, 0).unwrap();
        write_timestamptz(&mut buf, epoch_2000);
        assert_eq!(buf, vec![0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn timestamptz_opt_none_is_null() {
        let mut buf = Vec::new();
        write_timestamptz_opt(&mut buf, None);
        assert_eq!(buf, vec![0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn uuid_serialises_as_16_raw_bytes() {
        let mut buf = Vec::new();
        let uid = uuid::Uuid::from_bytes([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
        write_uuid(&mut buf, uid);
        assert_eq!(&buf[..4], &[0, 0, 0, 16]);
        assert_eq!(&buf[4..], uid.as_bytes());
    }

    #[test]
    fn f64_opt_some_writes_eight_be_bytes() {
        let mut buf = Vec::new();
        write_f64_opt(&mut buf, Some(2.75));
        assert_eq!(&buf[..4], &[0, 0, 0, 8]);
        assert_eq!(&buf[4..], &2.75f64.to_be_bytes());
    }

    #[test]
    fn f64_opt_none_is_null() {
        let mut buf = Vec::new();
        write_f64_opt(&mut buf, None);
        assert_eq!(buf, vec![0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn bool_opt_some_writes_single_byte() {
        let mut buf_true = Vec::new();
        write_bool_opt(&mut buf_true, Some(true));
        assert_eq!(buf_true, vec![0, 0, 0, 1, 0x01]);

        let mut buf_false = Vec::new();
        write_bool_opt(&mut buf_false, Some(false));
        assert_eq!(buf_false, vec![0, 0, 0, 1, 0x00]);
    }

    #[test]
    fn bool_opt_none_is_null() {
        let mut buf = Vec::new();
        write_bool_opt(&mut buf, None);
        assert_eq!(buf, vec![0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn bytea_opt_some_carries_length_prefix() {
        let mut buf = Vec::new();
        write_bytea_opt(&mut buf, Some(&[0xaa, 0xbb][..]));
        assert_eq!(buf, vec![0, 0, 0, 2, 0xaa, 0xbb]);
    }

    #[test]
    fn bytea_opt_none_is_null() {
        let mut buf = Vec::new();
        write_bytea_opt(&mut buf, None);
        assert_eq!(buf, vec![0xff, 0xff, 0xff, 0xff]);
    }
}
