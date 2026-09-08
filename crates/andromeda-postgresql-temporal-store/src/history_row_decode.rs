//! Decoding `sqlx::postgres::PgRow` into the backend-neutral
//! [`TemporalHistoryRow`] type declared by the temporal store port.
//!
//! Every column probe, code decode, EWKB-to-GeoJSON conversion and instance
//! identity reassembly lives here so that neither the service layer nor the
//! renderers ever import `sqlx`. The readers for the catalogue columns an
//! `attribute_history` row carries — its attribute name, its `datasetId`, its
//! unit identity — are shared with [`crate::aggregation_row_decode`], whose rows
//! project those same columns.
//!
//! Nothing in this module guesses. The rebuilt history table records which
//! typed column holds a row's value, so the value is read from that column and
//! from no other; a stored code the registry has never assigned fails the
//! decode rather than falling back to a default, because a row written by a
//! later release must be reported as unreadable instead of being silently
//! reinterpreted.

use crate::{attribute_kind_code::attribute_type_of, error::TemporalStoreError, stored_decimal::json_number_of, unit_code_metadata::restore_unit_code};
use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, instance_identity::InstanceIdentity, uri::Uri};
use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, value_kind::ValueKind};
use andromeda_postgresql_encoding::ewkb::Ewkb;
use andromeda_temporal::{append_intent::TemporalObservationIdentity, port::history_reader::TemporalHistoryObservation};
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

/// Decode one `attribute_history` row into a [`TemporalHistoryRow`].
pub(crate) fn decode_history_row(row: &PgRow) -> Result<TemporalHistoryObservation, TemporalStoreError> {
    let attribute_kind = decode_attribute_kind(row)?;
    let value = decode_value(row)?;
    let mut metadata: Option<Value> = row.try_get("metadata").unwrap_or(None);
    restore_unit_code(&mut metadata, decode_unit_code_id(row, "unit_code_id")?)?;

    let temporal_row = TemporalHistoryRow {
        attribute_name: decode_attribute_name(row)?,
        attribute_type: attribute_type_of(attribute_kind),
        dataset_id: decode_dataset_id(row)?,
        value,
        // Each axis is read from its own column and from no other. A row that
        // recorded no instant on an axis keeps none, because filling one in
        // from the write clock would invent a match on a Temporal Property the
        // observation does not have (clauses 4.11 and 5.2.21).
        observed_at: nullable_instant(row, "observed_at")?,
        recorded_at: required_instant(row, "recorded_at")?,
        created_at: nullable_instant(row, "created_at")?,
        modified_at: nullable_instant(row, "modified_at")?,
        instance: decode_instance_identity(row)?,
        metadata,
        partition_total: row.try_get("partition_total").ok(),
    };
    Ok(TemporalHistoryObservation {
        row: temporal_row,
        identity: Some(decode_write_identity(row)?),
    })
}

/// Read the row's attribute family from its stored code.
fn decode_attribute_kind(row: &PgRow) -> Result<AttributeKind, TemporalStoreError> {
    let stored: i16 = row
        .try_get("attribute_kind")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_history.attribute_kind: {error}")))?;
    AttributeKind::try_from(stored).map_err(|error| TemporalStoreError::Decode(error.to_string()))
}

/// Read the row's expanded attribute IRI.
pub(crate) fn decode_attribute_name(row: &PgRow) -> Result<AttributeName, TemporalStoreError> {
    let raw: String = row.try_get("attribute_name").map_err(|error| TemporalStoreError::Decode(error.to_string()))?;
    raw.parse::<AttributeName>()
        .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute name '{raw}': {error}")))
}

/// Read the stream's multi-attribute discriminator.
pub(crate) fn decode_dataset_id(row: &PgRow) -> Result<Option<Uri>, TemporalStoreError> {
    row.try_get::<Option<String>, _>("dataset_id")
        .map_err(|error| TemporalStoreError::Decode(error.to_string()))?
        .map(|raw| {
            raw.parse::<Uri>()
                .map_err(|error| TemporalStoreError::Decode(format!("invalid dataset_id '{raw}': {error}")))
        })
        .transpose()
}

/// Reassemble the identifier this instance is published to clients under.
///
/// The two identifier columns are mutually exclusive by check constraint, so a
/// row populating both or neither is corrupt rather than ambiguous, and is
/// reported instead of being resolved by precedence.
fn decode_instance_identity(row: &PgRow) -> Result<InstanceIdentity, TemporalStoreError> {
    let generated: Option<Uuid> = row
        .try_get("generated_instance_uuid")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_history.generated_instance_uuid: {error}")))?;
    let verbatim: Option<String> = row
        .try_get("verbatim_instance_id")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_history.verbatim_instance_id: {error}")))?;
    match (generated, verbatim) {
        (Some(uuid), None) => Ok(InstanceIdentity::Generated(uuid)),
        (None, Some(raw)) => Ok(InstanceIdentity::Verbatim(raw.parse::<Uri>().map_err(|error| {
            TemporalStoreError::Decode(format!("invalid attribute_history.verbatim_instance_id '{raw}': {error}"))
        })?)),
        (Some(_), Some(_)) => Err(TemporalStoreError::Decode(
            "attribute_history row populates both instance identifier columns".to_owned(),
        )),
        (None, None) => Err(TemporalStoreError::Decode(
            "attribute_history row populates neither instance identifier column".to_owned(),
        )),
    }
}

/// Read the identity of the atomic write that produced the row.
fn decode_write_identity(row: &PgRow) -> Result<TemporalObservationIdentity, TemporalStoreError> {
    let write_id: Uuid = row
        .try_get("write_id")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_history.write_id: {error}")))?;
    let ordinal: i32 = row
        .try_get("write_ordinal")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_history.write_ordinal: {error}")))?;
    Ok(TemporalObservationIdentity {
        write_id: WriteId::from_uuid(write_id),
        ordinal: WriteOrdinal::try_from(ordinal).map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_history.write_ordinal: {error}")))?,
    })
}

/// Read a non-null timestamp column.
fn required_instant(row: &PgRow, column: &str) -> Result<DateTime<Utc>, TemporalStoreError> {
    row.try_get::<Option<DateTime<Utc>>, _>(column)
        .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_history.{column}: {error}")))?
        .ok_or_else(|| TemporalStoreError::Decode(format!("attribute_history.{column} is absent and has no substitute")))
}

/// Read a nullable timestamp column, preserving its absence.
fn nullable_instant(row: &PgRow, column: &str) -> Result<Option<DateTime<Utc>>, TemporalStoreError> {
    row.try_get::<Option<DateTime<Utc>>, _>(column)
        .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_history.{column}: {error}")))
}

/// Read the row's stored unit-code identity.
///
/// A stored value outside the range the catalog assigns fails the decode: it
/// cannot name a registration, so rendering the instance without its unit would
/// silently change what the value means.
pub(crate) fn decode_unit_code_id(row: &PgRow, column: &str) -> Result<Option<UnitCodeId>, TemporalStoreError> {
    row.try_get::<Option<i16>, _>(column)
        .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_history.{column}: {error}")))?
        .map(|stored| UnitCodeId::try_from(stored).map_err(|error| TemporalStoreError::Decode(format!("attribute_history.{column} {stored}: {error}"))))
        .transpose()
}

/// Read the row's value from the column its stored value kind names.
fn decode_value(row: &PgRow) -> Result<TemporalValue, TemporalStoreError> {
    let stored: i16 = row
        .try_get("value_kind")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_history.value_kind: {error}")))?;
    let kind = ValueKind::try_from(stored).map_err(|error| TemporalStoreError::Decode(error.to_string()))?;
    match kind {
        ValueKind::Tombstone => Ok(TemporalValue::Tombstone {
            deleted_at: required_instant(row, "deleted_at")?,
        }),
        ValueKind::Number => Ok(TemporalValue::Number(decode_number(row)?)),
        ValueKind::String => Ok(TemporalValue::String(required_column(row, "value_string")?)),
        ValueKind::Boolean => Ok(TemporalValue::Boolean(required_column(row, "value_bool")?)),
        ValueKind::Geometry => Ok(TemporalValue::Geometry(decode_geometry(row)?)),
        ValueKind::Object => Ok(TemporalValue::Object(required_column(row, "value_object")?)),
        ValueKind::DateTime => Ok(TemporalValue::DateTime(required_column(row, "value_datetime")?)),
        ValueKind::Uri => {
            let raw: String = required_column(row, "value_uri")?;
            raw.parse::<Uri>()
                .map(TemporalValue::Uri)
                .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_history.value_uri '{raw}': {error}")))
        }
    }
}

/// Read the column the row's value kind declared, which must be populated.
pub(crate) fn required_column<'r, T>(row: &'r PgRow, column: &str) -> Result<T, TemporalStoreError>
where
    T: sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    row.try_get::<Option<T>, _>(column)
        .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_history.{column}: {error}")))?
        .ok_or_else(|| TemporalStoreError::Decode(format!("attribute_history.{column} is empty but the row declares that shape")))
}

/// Read the arbitrary-precision value column.
///
/// The stored column is `NUMERIC`, so its decimal is carried directly into the
/// exact neutral observation domain without a text or binary64 round trip.
fn decode_number(row: &PgRow) -> Result<JsonNumber, TemporalStoreError> {
    let stored: BigDecimal = required_column(row, "value_number")?;
    json_number_of(&stored, "attribute_history.value_number")
}

/// Read the geometry column, which the query layer selects as EWKB.
fn decode_geometry(row: &PgRow) -> Result<Value, TemporalStoreError> {
    let bytes: Vec<u8> = required_column(row, "value_geo_ewkb")?;
    let ewkb = Ewkb::try_from(bytes.as_slice()).map_err(|error| TemporalStoreError::Decode(format!("EWKB read: {error}")))?;
    let geometry = Geometry::try_from(&ewkb)
        .map_err(|error| TemporalStoreError::Decode(format!("EWKB decode: {error}")))?
        .to_geojson();
    serde_json::to_value(&geometry).map_err(|error| TemporalStoreError::Decode(format!("GeoJSON serialize: {error}")))
}
