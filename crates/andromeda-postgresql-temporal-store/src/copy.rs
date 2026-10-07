//! Binary COPY path for bulk attribute-history inserts.
//!
//! The stream is COPYed into a transaction-scoped `stage_attribute_history`
//! table and materialised into `attribute_history` with one `INSERT ... SELECT`.
//! The staging hop exists because two columns are not worth encoding in their
//! server-side binary form on the client: a geometry is handed over as EWKB and
//! projected by `ST_GeomFromEWKB`, and an arbitrary-precision number is handed
//! over as its decimal literal and cast to `NUMERIC`. Both casts are exact, and
//! both keep this module free of PostgreSQL's internal binary layouts for types
//! whose layout is not part of any stable contract.
//!
//! Field order follows the table's physical column order exactly, which
//! is the order the table was declared in for alignment reasons rather than a
//! logical grouping.

use crate::{
    attribute_history_row::AttributeHistoryRow,
    error::TemporalStoreError,
    stored_instance_columns::{generated_instance_id, verbatim_instance_id},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_branch::schema_prefix;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, value_kind::ValueKind};
use andromeda_postgresql_encoding::{
    copy::{
        CopyFieldError,
        write_bool_opt,
        write_bytea_opt,
        write_field_count,
        write_header,
        write_i16_opt,
        write_i32_opt,
        write_i64,
        write_jsonb_text,
        write_null,
        write_text_opt,
        write_timestamptz,
        write_timestamptz_opt,
        write_trailer,
        write_uuid,
        write_uuid_opt,
    },
    ewkb_binding::geometry_bytes,
};
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use serde_json::value::RawValue;
use sqlx::{Executor, Postgres, Transaction};

/// Number of columns written per staged history tuple.
const STAGE_ATTRIBUTE_HISTORY_FIELD_COUNT: i16 = 22;

const CREATE_STAGE_ATTRIBUTE_HISTORY_SQL: &str = r"
CREATE TEMP TABLE stage_attribute_history (
    recorded_at             timestamptz NOT NULL,
    write_id                uuid NOT NULL,
    stream_key              bigint NOT NULL,
    write_ordinal           integer NOT NULL,
    attribute_kind          smallint NOT NULL,
    value_kind              smallint NOT NULL,
    observed_at             timestamptz,
    created_at              timestamptz,
    modified_at             timestamptz,
    deleted_at              timestamptz,
    value_date_time          timestamptz,
    generated_instance_id uuid,
    unit_code_id            smallint,
    value_boolean              boolean,
    verbatim_instance_id    text,
    value_number            text,
    value_string            text,
    value_geometry               bytea,
    value_geometry_members  jsonb,
    value_object            jsonb,
    value_uri               text,
    metadata                jsonb
) ON COMMIT DROP
";

const COPY_INTO_STAGE_SQL: &str = "COPY stage_attribute_history (recorded_at, write_id, stream_key, write_ordinal, attribute_kind, \
     value_kind, observed_at, created_at, modified_at, deleted_at, value_date_time, generated_instance_id, unit_code_id, \
     value_boolean, verbatim_instance_id, value_number, value_string, value_geometry, value_geometry_members, value_object, value_uri, \
     metadata) FROM STDIN WITH (FORMAT binary)";

/// Materialise the staged history rows into the target branch's
/// `attribute_history`. The branch is selected by qualifying the target table
/// with `schema` (`""` for the live branch, `"snap_<key>."` for a Snapshot
/// branch); the session-scoped staging table needs no qualifier. The conflict
/// target names the write-identity columns rather than the constraint, because a
/// branch table's `LIKE`-copied unique index carries an auto-generated name.
fn insert_from_stage_sql(schema: &str) -> String {
    format!(
        r"
INSERT INTO {schema}attribute_history (
    recorded_at, write_id, stream_key, write_ordinal, attribute_kind, value_kind,
    observed_at, created_at, modified_at, deleted_at, value_date_time,
    generated_instance_id, unit_code_id, value_boolean, verbatim_instance_id,
    value_number, value_string, value_geometry, value_geometry_members, value_object, value_uri, metadata
)
SELECT
    s.recorded_at, s.write_id, s.stream_key, s.write_ordinal, s.attribute_kind, s.value_kind,
    s.observed_at, s.created_at, s.modified_at, s.deleted_at, s.value_date_time,
    s.generated_instance_id, s.unit_code_id, s.value_boolean, s.verbatim_instance_id,
    s.value_number::numeric, s.value_string,
    CASE WHEN s.value_geometry IS NOT NULL
         THEN ST_MakeValid(ST_GeomFromEWKB(s.value_geometry))
         ELSE NULL END,
    s.value_geometry_members, s.value_object, s.value_uri, s.metadata
FROM stage_attribute_history s
ON CONFLICT (recorded_at, write_id, write_ordinal) DO NOTHING
"
    )
}

/// Bulk-insert via binary COPY into a per-transaction staging table, then
/// materialise into `attribute_history`.
///
/// The caller runs any deduplication `DELETE`s against `attribute_history`
/// before this call, so the staged insert never collides with rows already
/// present.
pub(crate) async fn insert_history_batch_via_copy_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    rows: &[AttributeHistoryRow],
) -> Result<(), TemporalStoreError> {
    if rows.is_empty() {
        return Ok(());
    }

    tx.execute(CREATE_STAGE_ATTRIBUTE_HISTORY_SQL).await?;

    let payload = encode_history_copy_stream(rows)?;
    {
        let mut copy = tx.copy_in_raw(COPY_INTO_STAGE_SQL).await?;
        copy.send(payload.as_slice()).await?;
        copy.finish().await?;
    }

    let insert_sql = insert_from_stage_sql(&schema_prefix(target));
    sqlx::query(sqlx::AssertSqlSafe(insert_sql.as_str())).execute(&mut **tx).await?;
    Ok(())
}

/// Encode one binary COPY stream for the staged history rows.
///
/// # Errors
///
/// Returns [`TemporalStoreError::CopyField`] when a staged value is longer
/// than the format's 32-bit field prefix can announce or does not serialise,
/// [`TemporalStoreError::Encoding`] when a geometry has no EWKB form, and
/// [`TemporalStoreError::Validation`] when a row's write ordinal exceeds the
/// width of its column.
pub(crate) fn encode_history_copy_stream(rows: &[AttributeHistoryRow]) -> Result<Vec<u8>, TemporalStoreError> {
    let mut buf = Vec::with_capacity(estimated_stream_bytes(rows.len()));
    write_header(&mut buf);
    for row in rows {
        let numeric = row.value.numeric_literal();
        write_field_count(&mut buf, STAGE_ATTRIBUTE_HISTORY_FIELD_COUNT);
        write_timestamptz(&mut buf, row.recorded_at);
        write_uuid(&mut buf, row.write_id());
        write_i64(&mut buf, row.stream_key.get());
        write_i32_opt(&mut buf, Some(row.write_ordinal()?));
        write_i16_opt(&mut buf, Some(AttributeKind::code(row.attribute_kind)));
        write_i16_opt(&mut buf, Some(ValueKind::code(row.value.kind())));
        write_timestamptz_opt(&mut buf, row.axes.observed_at);
        write_timestamptz_opt(&mut buf, row.axes.created_at);
        write_timestamptz_opt(&mut buf, row.axes.modified_at);
        write_timestamptz_opt(&mut buf, row.value.deleted_at());
        write_timestamptz_opt(&mut buf, row.value.date_time());
        write_uuid_opt(&mut buf, generated_instance_id(&row.instance));
        write_i16_opt(&mut buf, row.unit_code_id.map(UnitCodeId::get));
        write_bool_opt(&mut buf, row.value.boolean());
        write_text_opt(&mut buf, verbatim_instance_id(&row.instance))?;
        write_text_opt(&mut buf, numeric.as_deref())?;
        write_text_opt(&mut buf, row.value.string())?;
        write_bytea_opt(&mut buf, geometry_bytes(row.value.geometry())?.as_deref())?;
        write_stored_json(&mut buf, row.value.geometry_members())?;
        write_stored_json(&mut buf, row.value.object())?;
        write_text_opt(&mut buf, row.value.uri().map(Uri::as_str))?;
        write_stored_json(&mut buf, row.metadata.as_deref())?;
    }
    write_trailer(&mut buf);
    Ok(buf)
}

/// Append an optional `jsonb` field from the JSON a column stores, encoding
/// `None` as a NULL.
///
/// # Errors
///
/// Returns [`CopyFieldError`] when the document is longer than a field prefix
/// can announce.
fn write_stored_json(buf: &mut Vec<u8>, json: Option<&RawValue>) -> Result<(), CopyFieldError> {
    let Some(json) = json else {
        write_null(buf);
        return Ok(());
    };
    write_jsonb_text(buf, json.get())
}

const fn estimated_stream_bytes(rows: usize) -> usize {
    // Header (19) + trailer (2) + ~256 B per row (mixed nulls + small
    // payload + 21 length prefixes).
    21 + rows * 256
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_history_row::AttributeHistoryRow,
        attribute_history_value::AttributeHistoryValue,
        copy::{STAGE_ATTRIBUTE_HISTORY_FIELD_COUNT, encode_history_copy_stream, insert_from_stage_sql},
        semantic_time_axes::SemanticTimeAxes,
    };
    use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_ngsi_ld_lexicon::instance_identity::InstanceIdentity;
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_postgresql_codes::{attribute_kind::AttributeKind, stream_key::StreamKey};
    use andromeda_postgresql_encoding::copy::COPY_HEADER_SIGNATURE;
    use andromeda_temporal::append_intent::TemporalObservationIdentity;
    use andromeda_unit_catalog::unit_code_id::UnitCodeId;
    use chrono::{DateTime, TimeZone, Utc};
    use uuid::Uuid;

    /// Bytes the fixed-size header occupies before the first tuple.
    const HEADER_BYTES: usize = 19;
    /// Bytes one NULL field occupies: its length prefix alone.
    const NULL_FIELD_BYTES: usize = 4;

    fn instant() -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant")
    }

    /// Microseconds since the PostgreSQL epoch for [`instant`].
    fn instant_micros() -> i64 {
        1_700_000_000_i64.saturating_mul(1_000_000) - 946_684_800_000_000_i64
    }

    fn row(stream_key: i64, value: AttributeHistoryValue) -> AttributeHistoryRow {
        AttributeHistoryRow {
            recorded_at: instant(),
            identity: TemporalObservationIdentity {
                write_id: WriteId::from_uuid(Uuid::nil()),
                ordinal: WriteOrdinal::at(0),
            },
            stream_key: StreamKey::try_from(stream_key).expect("positive stream key"),
            attribute_kind: AttributeKind::Property,
            value,
            axes: SemanticTimeAxes::default(),
            instance: InstanceIdentity::Generated(Uuid::nil()),
            unit_code_id: None,
            metadata: None,
        }
    }

    fn field_length(buf: &[u8], offset: usize) -> i32 {
        i32::from_be_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]])
    }

    #[test]
    fn the_stream_opens_with_the_pgcopy_signature_and_closes_with_the_terminator() {
        let buf = encode_history_copy_stream(&[row(1, AttributeHistoryValue::Boolean(true))]).expect("copy stream encodes");
        assert!(buf.starts_with(COPY_HEADER_SIGNATURE));
        pretty_assertions::assert_eq!(&buf[buf.len() - 2..], &[0xff, 0xff]);
    }

    /// A commit replayed under its original write identity re-stages every row
    /// it already wrote; the row-level conflict clause on the materialising
    /// insert is what makes that second insert a no-op instead of a duplicate.
    #[test]
    fn a_row_replayed_under_its_original_write_identity_is_not_duplicated() {
        assert!(insert_from_stage_sql("").contains("ON CONFLICT (recorded_at, write_id, write_ordinal) DO NOTHING"));
    }

    /// A Snapshot branch qualifies the target relation with its schema while the
    /// session-scoped staging table stays unqualified.
    #[test]
    fn a_branch_prefix_qualifies_the_history_table() {
        let sql = insert_from_stage_sql("snap_3.");
        assert!(sql.contains("INSERT INTO snap_3.attribute_history ("), "{sql}");
        assert!(sql.contains("FROM stage_attribute_history s"), "{sql}");
    }

    #[test]
    fn every_tuple_announces_the_rebuilt_tables_column_count() {
        let rows = [row(1, AttributeHistoryValue::Boolean(true)), row(1, AttributeHistoryValue::Boolean(false))];
        let buf = encode_history_copy_stream(&rows).expect("copy stream encodes");
        pretty_assertions::assert_eq!(&buf[HEADER_BYTES..HEADER_BYTES + 2], &STAGE_ATTRIBUTE_HISTORY_FIELD_COUNT.to_be_bytes());
    }

    #[test]
    fn the_partition_column_leads_every_tuple() {
        let buf = encode_history_copy_stream(&[row(1, AttributeHistoryValue::Boolean(true))]).expect("copy stream encodes");
        let offset = HEADER_BYTES + 2;
        pretty_assertions::assert_eq!(field_length(&buf, offset), 8);
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
        pretty_assertions::assert_eq!(micros, instant_micros());
    }

    #[test]
    fn the_write_identity_precedes_the_stream_key() {
        let stream_key = 0x0102_0304_0506_0708_i64;
        let mut row = row(stream_key, AttributeHistoryValue::Boolean(true));
        let write_id = Uuid::from_u128(9);
        row.identity.write_id = WriteId::from_uuid(write_id);
        let buf = encode_history_copy_stream(&[row]).expect("copy stream encodes");
        let write_offset = HEADER_BYTES + 2 + 4 + 8;
        pretty_assertions::assert_eq!(field_length(&buf, write_offset), 16);
        pretty_assertions::assert_eq!(&buf[write_offset + 4..write_offset + 20], write_id.as_bytes());
        let stream_offset = write_offset + 4 + 16;
        pretty_assertions::assert_eq!(&buf[stream_offset..stream_offset + 4], &[0, 0, 0, 8]);
        pretty_assertions::assert_eq!(&buf[stream_offset + 4..stream_offset + 12], &stream_key.to_be_bytes());
    }

    #[test]
    fn both_kind_codes_are_written_as_the_numbers_the_migration_assigned() {
        let mut row = row(1, AttributeHistoryValue::Boolean(true));
        row.attribute_kind = AttributeKind::GeoProperty;
        let buf = encode_history_copy_stream(&[row]).expect("copy stream encodes");
        let attribute_kind_offset = HEADER_BYTES + 2 + (4 + 8) + (4 + 16) + (4 + 8) + (4 + 4);
        pretty_assertions::assert_eq!(field_length(&buf, attribute_kind_offset), 2);
        pretty_assertions::assert_eq!(&buf[attribute_kind_offset + 4..attribute_kind_offset + 6], &3_i16.to_be_bytes());
        let value_kind_offset = attribute_kind_offset + 4 + 2;
        pretty_assertions::assert_eq!(&buf[value_kind_offset + 4..value_kind_offset + 6], &3_i16.to_be_bytes());
    }

    #[test]
    fn a_number_is_staged_as_the_decimal_literal_the_numeric_cast_reads() {
        // The value is one above 2^53: it has no exact double, so a stream that
        // encoded it as a float8 would have to refuse or round it.
        let row = row(1, AttributeHistoryValue::Number(JsonNumber::from(9_007_199_254_740_993_i64)));
        let buf = encode_history_copy_stream(&[row]).expect("copy stream encodes");
        let literal = b"9007199254740993";
        let found = buf.windows(literal.len()).any(|window| window == literal);
        assert!(found, "the decimal literal must reach the stream verbatim");
    }

    #[test]
    fn a_tombstone_writes_its_deletion_instant_and_no_typed_value() {
        let row = row(1, AttributeHistoryValue::Tombstone { deleted_at: instant() });
        let buf = encode_history_copy_stream(&[row]).expect("copy stream encodes");
        let deleted_offset = HEADER_BYTES + 2 + (4 + 8) + (4 + 16) + (4 + 8) + (4 + 4) + (4 + 2) + (4 + 2) + 3 * NULL_FIELD_BYTES;
        pretty_assertions::assert_eq!(field_length(&buf, deleted_offset), 8);
        pretty_assertions::assert_eq!(field_length(&buf, deleted_offset + 4 + 8), -1);
    }

    #[test]
    fn the_unit_identity_is_written_as_its_catalog_number() {
        let mut row = row(1, AttributeHistoryValue::Boolean(true));
        row.unit_code_id = UnitCodeId::try_from(1121_i16).ok();
        let buf = encode_history_copy_stream(&[row]).expect("copy stream encodes");
        let unit_offset = HEADER_BYTES
            + 2
            + (4 + 8)
            + (4 + 16)
            + (4 + 8)
            + (4 + 4)
            + (4 + 2)
            + (4 + 2)
            + 3 * NULL_FIELD_BYTES
            + NULL_FIELD_BYTES
            + NULL_FIELD_BYTES
            + (4 + 16);
        pretty_assertions::assert_eq!(field_length(&buf, unit_offset), 2);
        pretty_assertions::assert_eq!(&buf[unit_offset + 4..unit_offset + 6], &1121_i16.to_be_bytes());
    }

    #[test]
    fn a_client_supplied_identifier_occupies_the_verbatim_column_and_not_the_uuid_one() {
        let identifier = "https://example.com/instances/1".parse().expect("a valid URI");
        let mut row = row(1, AttributeHistoryValue::Boolean(true));
        row.instance = InstanceIdentity::Verbatim(identifier);
        let buf = encode_history_copy_stream(&[row]).expect("copy stream encodes");
        let generated_offset =
            HEADER_BYTES + 2 + (4 + 8) + (4 + 16) + (4 + 8) + (4 + 4) + (4 + 2) + (4 + 2) + 3 * NULL_FIELD_BYTES + NULL_FIELD_BYTES + NULL_FIELD_BYTES;
        pretty_assertions::assert_eq!(field_length(&buf, generated_offset), -1);
        let expected = b"https://example.com/instances/1";
        assert!(buf.windows(expected.len()).any(|window| window == expected));
    }
}
