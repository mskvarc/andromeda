//! Shared encoders for the Postgres binary `COPY ... FROM STDIN` protocol.
//!
//! The format is documented at
//! <https://www.postgresql.org/docs/current/sql-copy.html#id-1.9.3.55.9.4.6>.
//! Each tuple is preceded by a 16-bit field count; each field is either
//! `-1` (NULL, signalled as `i32 = -1`) or `len` followed by `len` bytes
//! of pre-typed payload. Encoders here own the per-type payload layout
//! so callers only need to drive tuple structure.

use chrono::{DateTime, Utc};
use thiserror::Error;

/// Postgres binary-COPY header signature (`PGCOPY\n\377\r\n\0`).
pub const COPY_HEADER_SIGNATURE: &[u8] = b"PGCOPY\n\xff\r\n\0";

/// Offset (microseconds) between the Unix epoch and Postgres's
/// timestamptz epoch of 2000-01-01 00:00:00 UTC.
pub const POSTGRES_EPOCH_MICROS: i64 = 946_684_800_000_000;

/// Version byte that opens every `jsonb` binary payload. `PostgreSQL` has
/// defined only version 1 of that layout.
const JSONB_VERSION: u8 = 0x01;

/// Errors raised while encoding a single binary-COPY field.
#[derive(Debug, Error)]
pub enum CopyFieldError {
    /// The payload is longer than the signed 32-bit prefix that announces
    /// every field, so the field cannot be described by the format at all.
    #[error("binary COPY field is {length} bytes, more than the 32-bit length prefix can describe")]
    FieldTooLong {
        /// Length, in bytes, of the payload that was refused.
        length: usize,
    },
}

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
///
/// # Errors
///
/// Returns [`CopyFieldError::FieldTooLong`] when the string's UTF-8 length
/// exceeds [`i32::MAX`], the widest payload a field prefix can announce.
pub fn write_text(buf: &mut Vec<u8>, value: &str) -> Result<(), CopyFieldError> {
    let bytes = value.as_bytes();
    write_payload_length(buf, bytes.len())?;
    buf.extend_from_slice(bytes);
    Ok(())
}

/// Append an optional `text` field, encoding `None` as a NULL.
///
/// # Errors
///
/// Returns [`CopyFieldError::FieldTooLong`] when a present string is longer
/// than a field prefix can announce.
pub fn write_text_opt(buf: &mut Vec<u8>, value: Option<&str>) -> Result<(), CopyFieldError> {
    let Some(text) = value else {
        write_null(buf);
        return Ok(());
    };
    write_text(buf, text)
}

/// Append a `jsonb` field from JSON already serialised to text — length
/// prefix, version byte 0x01, then the text's bytes.
///
/// The text is written as given, so it must be valid JSON; a caller that
/// serialised a typed value once for another bind path hands the same bytes
/// here instead of serialising it again.
///
/// # Errors
///
/// Returns [`CopyFieldError::FieldTooLong`] when the text is longer than a
/// field prefix can announce.
pub fn write_jsonb_text(buf: &mut Vec<u8>, json: &str) -> Result<(), CopyFieldError> {
    write_jsonb_payload(buf, json.as_bytes())
}

/// Append the body both `jsonb` forms share: a length prefix covering the
/// version byte together with `json`, that version byte, then the JSON bytes.
///
/// # Errors
///
/// Returns [`CopyFieldError::FieldTooLong`] when the version byte and the JSON
/// bytes together are longer than a field prefix can announce.
fn write_jsonb_payload(buf: &mut Vec<u8>, json: &[u8]) -> Result<(), CopyFieldError> {
    let json_len = json.len();
    let total_len = json_len.checked_add(1).ok_or(CopyFieldError::FieldTooLong { length: json_len })?;
    write_payload_length(buf, total_len)?;
    buf.push(JSONB_VERSION);
    buf.extend_from_slice(json);
    Ok(())
}

/// Append an optional `bytea` field, encoding `None` as a NULL.
///
/// # Errors
///
/// Returns [`CopyFieldError::FieldTooLong`] when present bytes are longer than
/// a field prefix can announce.
pub fn write_bytea_opt(buf: &mut Vec<u8>, value: Option<&[u8]>) -> Result<(), CopyFieldError> {
    let Some(bytes) = value else {
        write_null(buf);
        return Ok(());
    };
    write_payload_length(buf, bytes.len())?;
    buf.extend_from_slice(bytes);
    Ok(())
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

/// Append a non-null 64-bit integer field in network byte order.
pub fn write_i64(buf: &mut Vec<u8>, value: i64) {
    write_length(buf, 8);
    buf.extend_from_slice(&value.to_be_bytes());
}

/// Append an optional 16-bit integer field in network byte order.
pub fn write_i16_opt(buf: &mut Vec<u8>, value: Option<i16>) {
    match value {
        Some(value) => {
            write_length(buf, 2);
            buf.extend_from_slice(&value.to_be_bytes());
        }
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
///
/// Fixed-width encoders pass their own constant width here; a payload whose
/// length is only known at runtime is announced through the checked path,
/// which refuses a length the prefix cannot carry.
pub fn write_length(buf: &mut Vec<u8>, len: i32) {
    buf.extend_from_slice(&len.to_be_bytes());
}

/// Append the length prefix for a payload sized at runtime.
///
/// # Errors
///
/// Returns [`CopyFieldError::FieldTooLong`] when the length does not fit the
/// signed 32-bit prefix the binary-COPY format defines.
fn write_payload_length(buf: &mut Vec<u8>, len: usize) -> Result<(), CopyFieldError> {
    let prefix = i32::try_from(len).map_err(|_| CopyFieldError::FieldTooLong { length: len })?;
    write_length(buf, prefix);
    Ok(())
}

/// Append a NULL marker (length = -1 as i32 big-endian).
pub fn write_null(buf: &mut Vec<u8>) {
    buf.extend_from_slice(&(-1i32).to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use pretty_assertions::assert_eq;

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
        write_text(&mut buf, "abc").expect("a short text field encodes");
        assert_eq!(buf, vec![0, 0, 0, 3, b'a', b'b', b'c']);
    }

    #[test]
    fn jsonb_text_carries_the_length_the_version_byte_and_the_text() {
        let mut from_text = Vec::new();
        write_jsonb_text(&mut from_text, r#"{"a":[1,2]}"#).expect("the text form encodes");
        assert_eq!(from_text[..4], [0, 0, 0, 12]);
        assert_eq!(from_text[4], 0x01);
        assert_eq!(&from_text[5..], br#"{"a":[1,2]}"#);
    }

    #[test]
    fn text_opt_none_is_null() {
        let mut buf = Vec::new();
        write_text_opt(&mut buf, None).expect("a null text field encodes");
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
    fn i64_serialises_as_eight_big_endian_bytes() {
        let mut buf = Vec::new();
        write_i64(&mut buf, 0x0102_0304_0506_0708);
        assert_eq!(buf, vec![0, 0, 0, 8, 1, 2, 3, 4, 5, 6, 7, 8]);
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
        write_bytea_opt(&mut buf, Some(&[0xaa, 0xbb][..])).expect("the bytes encode");
        assert_eq!(buf, vec![0, 0, 0, 2, 0xaa, 0xbb]);
    }

    #[test]
    fn bytea_opt_none_is_null() {
        let mut buf = Vec::new();
        write_bytea_opt(&mut buf, None).expect("a null bytea field encodes");
        assert_eq!(buf, vec![0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn a_payload_longer_than_the_length_prefix_can_describe_is_refused() {
        let mut buf = Vec::new();
        let error = write_payload_length(&mut buf, usize::MAX).expect_err("a length past i32::MAX has no prefix");
        let CopyFieldError::FieldTooLong { length } = error;
        assert_eq!(length, usize::MAX);
        assert!(buf.is_empty(), "a refused field leaves no partial prefix in the stream");
    }
}
