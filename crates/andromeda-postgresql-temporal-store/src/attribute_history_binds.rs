//! The column-major binds one attribute-history batch travels in.
//!
//! Everything that turns a row into a column happens here: the EWKB of a
//! geometry, the array literal of the `objectType` keys, and the
//! `sub_attribute_tree` each row's elements travel in.

use crate::{
    attribute_history_row::AttributeHistoryRow,
    error::TemporalStoreError,
    stored_instance_columns::{generated_instance_id, verbatim_instance_id},
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_attribute_instance::sub_attribute::{SubAttributeRecord, SubAttributeTree};
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, stream_key::StreamKey, value_kind::ValueKind};
use andromeda_postgresql_encoding::{array_literal::IntegerArrayLiteral, ewkb::EwkbError, ewkb_binding::geometry_bytes};
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde_json::value::RawValue;
use sqlx::types::Json;
use std::mem;
use uuid::Uuid;

/// Column-major binds for one attribute-history batch.
pub(crate) struct AttributeHistoryBinds<'a> {
    /// Write clock of each row, and the hypertable's partition column.
    pub(crate) recorded_ats: Vec<DateTime<Utc>>,
    /// Atomic write each row belongs to.
    pub(crate) write_ids: Vec<Uuid>,
    /// Stream each row extends.
    pub(crate) stream_keys: Vec<i64>,
    /// Position of each row within its atomic write.
    pub(crate) write_ordinals: Vec<i32>,
    /// Attribute family each row was written as.
    pub(crate) attribute_kinds: Vec<i16>,
    /// Stored value shape of each row.
    pub(crate) value_kinds: Vec<i16>,
    /// Client-declared observation instants.
    pub(crate) observed_ats: Vec<Option<DateTime<Utc>>>,
    /// Instants the instances were first recorded at.
    pub(crate) created_ats: Vec<Option<DateTime<Utc>>>,
    /// Instants the instances were last altered at.
    pub(crate) modified_ats: Vec<Option<DateTime<Utc>>>,
    /// Deletion instants, present only on tombstones.
    pub(crate) deleted_ats: Vec<Option<DateTime<Utc>>>,
    /// Timestamp values.
    pub(crate) value_date_times: Vec<Option<DateTime<Utc>>>,
    /// Broker-minted instance identifiers.
    pub(crate) generated_instance_ids: Vec<Option<Uuid>>,
    /// Catalog identities of the instances' units.
    pub(crate) unit_code_ids: Vec<Option<i16>>,
    /// Boolean values.
    pub(crate) value_booleans: Vec<Option<bool>>,
    /// Instance identifiers stored exactly as received.
    pub(crate) verbatim_instance_ids: Vec<Option<&'a str>>,
    /// Arbitrary-precision values bound directly as `numeric[]`.
    pub(crate) value_numbers: Vec<Option<BigDecimal>>,
    /// String values.
    pub(crate) value_strings: Vec<Option<&'a str>>,
    /// Geometry values as EWKB, projected server-side.
    pub(crate) value_geometries: Vec<Option<Vec<u8>>>,
    /// The geometries' `bbox` and foreign members, as the JSON the column
    /// stores.
    pub(crate) value_geometry_members: Vec<Option<Json<&'a RawValue>>>,
    /// Structured values, as the JSON the column stores.
    pub(crate) value_objects: Vec<Option<Json<&'a RawValue>>>,
    /// URI values, which is how a relationship object is stored.
    pub(crate) value_uris: Vec<Option<&'a str>>,
    /// `entity_type_names` keys of the `valueType`s.
    pub(crate) value_type_keys: Vec<Option<i32>>,
    /// `entity_type_names` keys of the `objectType` lists, as array literals,
    /// NULL when a list is empty.
    pub(crate) object_type_keys: Vec<Option<IntegerArrayLiteral>>,
    /// The seal members, as the JSON the column stores.
    pub(crate) proof_seals: Vec<Option<Json<&'a RawValue>>>,
    /// The `expiresAt` instants.
    pub(crate) expires_ats: Vec<Option<DateTime<Utc>>>,
}

/// Spread one attribute-history batch into its column-major binds.
pub(crate) fn attribute_history_binds(rows: &[AttributeHistoryRow]) -> Result<AttributeHistoryBinds<'_>, TemporalStoreError> {
    let mut binds = AttributeHistoryBinds {
        recorded_ats: Vec::with_capacity(rows.len()),
        write_ids: Vec::with_capacity(rows.len()),
        stream_keys: Vec::with_capacity(rows.len()),
        write_ordinals: Vec::with_capacity(rows.len()),
        attribute_kinds: Vec::with_capacity(rows.len()),
        value_kinds: Vec::with_capacity(rows.len()),
        observed_ats: Vec::with_capacity(rows.len()),
        created_ats: Vec::with_capacity(rows.len()),
        modified_ats: Vec::with_capacity(rows.len()),
        deleted_ats: Vec::with_capacity(rows.len()),
        value_date_times: Vec::with_capacity(rows.len()),
        generated_instance_ids: Vec::with_capacity(rows.len()),
        unit_code_ids: Vec::with_capacity(rows.len()),
        value_booleans: Vec::with_capacity(rows.len()),
        verbatim_instance_ids: Vec::with_capacity(rows.len()),
        value_numbers: Vec::with_capacity(rows.len()),
        value_strings: Vec::with_capacity(rows.len()),
        value_geometries: Vec::with_capacity(rows.len()),
        value_geometry_members: Vec::with_capacity(rows.len()),
        value_objects: Vec::with_capacity(rows.len()),
        value_uris: Vec::with_capacity(rows.len()),
        value_type_keys: Vec::with_capacity(rows.len()),
        object_type_keys: Vec::with_capacity(rows.len()),
        proof_seals: Vec::with_capacity(rows.len()),
        expires_ats: Vec::with_capacity(rows.len()),
    };
    for row in rows {
        binds.recorded_ats.push(row.recorded_at);
        binds.write_ids.push(row.write_id());
        binds.stream_keys.push(StreamKey::get(row.stream_key));
        binds.write_ordinals.push(row.write_ordinal()?);
        binds.attribute_kinds.push(AttributeKind::code(row.attribute_kind));
        binds.value_kinds.push(ValueKind::code(row.value.kind()));
        binds.observed_ats.push(row.axes.observed_at);
        binds.created_ats.push(row.axes.created_at);
        binds.modified_ats.push(row.axes.modified_at);
        binds.deleted_ats.push(row.value.deleted_at());
        binds.value_date_times.push(row.value.date_time());
        binds.generated_instance_ids.push(generated_instance_id(&row.instance));
        binds.unit_code_ids.push(row.members.unit_code_id.map(UnitCodeId::get));
        binds.value_booleans.push(row.value.boolean());
        binds.verbatim_instance_ids.push(verbatim_instance_id(&row.instance));
        // SQLx owns the array values. Clone the decimal itself so the bind is
        // exact and never passes through text or binary64.
        binds.value_numbers.push(row.value.number().cloned());
        binds.value_strings.push(row.value.string());
        binds.value_geometries.push(geometry_bytes(row.value.geometry())?);
        binds.value_geometry_members.push(row.value.geometry_members().map(Json));
        binds.value_objects.push(row.value.object().map(Json));
        binds.value_uris.push(row.value.uri().map(Uri::as_str));
        binds.value_type_keys.push(row.members.value_type_key.map(EntityTypeNameKey::get));
        binds.object_type_keys.push(
            (!row.members.object_type_keys.is_empty()).then(|| IntegerArrayLiteral::of(row.members.object_type_keys.iter().map(|key| i64::from(key.get())))),
        );
        binds.proof_seals.push(row.members.proof_seals.as_ref().map(|seals| Json(seals.as_raw())));
        binds.expires_ats.push(row.members.expires_at);
    }
    Ok(binds)
}

/// Take each row's sub-attribute elements out as the carrier composite the
/// statement binds one per row, NULL for a row without elements.
pub(crate) fn element_trees(rows: &mut [AttributeHistoryRow]) -> Result<Vec<Option<SubAttributeTree>>, EwkbError> {
    rows.iter_mut()
        .map(|row| {
            let elements = mem::take(&mut row.members.sub_attributes)
                .into_iter()
                .map(SubAttributeRecord::try_from)
                .collect::<Result<Vec<_>, EwkbError>>()?;
            Ok((!elements.is_empty()).then_some(SubAttributeTree { elements }))
        })
        .collect()
}
