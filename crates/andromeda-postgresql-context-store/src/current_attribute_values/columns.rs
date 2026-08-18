//! The column-major binds one projection batch is written from.
//!
//! A projection write serves one attribute instance or ten thousand through the
//! same statement, so the rows are transposed into one vector per column and
//! expanded server-side. Everything that turns a domain value into a column —
//! the exact decimal a `NUMERIC` bind carries, the EWKB a geometry is projected from,
//! the codes the kind columns hold — happens on the way in here.

use crate::{
    current_attribute_values::row::{ProjectedAttributeRow, ProjectedAttributeValue},
    error::ContextStoreError,
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, entity_key::EntityKey, value_kind::ValueKind};
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// The typed columns one projected value occupies.
///
/// Exactly one field is populated, decided by the value's own shape rather than
/// by the caller, which is what keeps the `value_kind` code and the column
/// holding the value from ever disagreeing.
struct TypedValueColumns<'a> {
    /// The arbitrary-precision value bound directly as `NUMERIC`.
    value_number: Option<&'a BigDecimal>,
    /// The string value.
    value_string: Option<&'a str>,
    /// The boolean value.
    value_bool: Option<bool>,
    /// The geometry value, before EWKB encoding.
    value_geo: Option<&'a Geometry>,
    /// The structured value.
    value_object: Option<&'a Value>,
    /// The timestamp value.
    value_datetime: Option<DateTime<Utc>>,
    /// The URI value.
    value_uri: Option<&'a str>,
}

/// Spread one projected value across the typed columns.
fn typed_value_columns(value: &ProjectedAttributeValue) -> TypedValueColumns<'_> {
    let mut columns = TypedValueColumns {
        value_number: None,
        value_string: None,
        value_bool: None,
        value_geo: None,
        value_object: None,
        value_datetime: None,
        value_uri: None,
    };
    match value {
        ProjectedAttributeValue::Number(number) => columns.value_number = Some(number.decimal()),
        ProjectedAttributeValue::String(text) => columns.value_string = Some(text),
        ProjectedAttributeValue::Boolean(flag) => columns.value_bool = Some(*flag),
        ProjectedAttributeValue::Geometry(geometry) => columns.value_geo = Some(geometry),
        ProjectedAttributeValue::Object(object) => columns.value_object = Some(object),
        ProjectedAttributeValue::DateTime(instant) => columns.value_datetime = Some(*instant),
        ProjectedAttributeValue::Uri(uri) => columns.value_uri = Some(uri.as_str()),
    }
    columns
}

/// Column-major binds for one projection batch.
pub struct ProjectionBinds<'a> {
    /// Surrogate key of the entity each row belongs to.
    pub entity_keys: Vec<i64>,
    /// Dictionary key of each row's attribute IRI.
    pub attribute_name_ids: Vec<i32>,
    /// Attribute family each row was written as.
    pub attribute_kinds: Vec<i16>,
    /// Stored value shape of each row.
    pub value_kinds: Vec<i16>,
    /// Timestamp values.
    pub value_datetimes: Vec<Option<DateTime<Utc>>>,
    /// Catalog identities of the instances' units.
    pub unit_code_ids: Vec<Option<i16>>,
    /// Boolean values.
    pub value_bools: Vec<Option<bool>>,
    /// `datasetId` of each row, absent on a default instance.
    pub dataset_ids: Vec<Option<&'a str>>,
    /// Arbitrary-precision values bound as a `numeric[]`.
    pub value_numbers: Vec<Option<BigDecimal>>,
    /// String values.
    pub value_strings: Vec<Option<&'a str>>,
    /// Geometry values as EWKB, projected server-side.
    pub value_geos: Vec<Option<Vec<u8>>>,
    /// Structured values.
    pub value_objects: Vec<Option<&'a Value>>,
    /// URI values, which is how a relationship object is projected.
    pub value_uris: Vec<Option<&'a str>>,
}

impl<'a> ProjectionBinds<'a> {
    /// Transpose a batch of projection rows into column-major binds.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Encoding`] when a geometry cannot be
    /// encoded as EWKB.
    pub fn from_rows(rows: &'a [ProjectedAttributeRow]) -> Result<ProjectionBinds<'a>, ContextStoreError> {
        let mut binds = ProjectionBinds {
            entity_keys: Vec::with_capacity(rows.len()),
            attribute_name_ids: Vec::with_capacity(rows.len()),
            attribute_kinds: Vec::with_capacity(rows.len()),
            value_kinds: Vec::with_capacity(rows.len()),
            value_datetimes: Vec::with_capacity(rows.len()),
            unit_code_ids: Vec::with_capacity(rows.len()),
            value_bools: Vec::with_capacity(rows.len()),
            dataset_ids: Vec::with_capacity(rows.len()),
            value_numbers: Vec::with_capacity(rows.len()),
            value_strings: Vec::with_capacity(rows.len()),
            value_geos: Vec::with_capacity(rows.len()),
            value_objects: Vec::with_capacity(rows.len()),
            value_uris: Vec::with_capacity(rows.len()),
        };
        for row in rows {
            let value = typed_value_columns(&row.value);
            binds.entity_keys.push(EntityKey::get(row.key.entity_key));
            binds.attribute_name_ids.push(AttributeNameId::get(row.key.attribute_name_id));
            binds.attribute_kinds.push(AttributeKind::code(row.attribute_kind));
            binds.value_kinds.push(ValueKind::code(row.value.kind()));
            binds.value_datetimes.push(value.value_datetime);
            binds.unit_code_ids.push(row.unit_code_id.map(UnitCodeId::get));
            binds.value_bools.push(value.value_bool);
            binds.dataset_ids.push(row.key.dataset_id.as_ref().map(Uri::as_str));
            // SQLx owns array elements for the duration of the query. Clone the
            // decimal itself, preserving its exact value without formatting it.
            binds.value_numbers.push(value.value_number.cloned());
            binds.value_strings.push(value.value_string);
            binds.value_geos.push(geometry_bytes(value.value_geo)?);
            binds.value_objects.push(value.value_object);
            binds.value_uris.push(value.value_uri);
        }
        Ok(binds)
    }
}

#[cfg(test)]
mod tests {
    use crate::current_attribute_values::{
        columns::{ProjectionBinds, typed_value_columns},
        row::{ProjectedAttributeKey, ProjectedAttributeRow, ProjectedAttributeValue},
    };
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_postgresql_codes::{attribute_kind::AttributeKind, entity_key::EntityKey, value_kind::ValueKind};
    use chrono::{TimeZone, Utc};

    /// One projection row carrying `value`, with no unit and no `datasetId`.
    fn row(value: ProjectedAttributeValue) -> ProjectedAttributeRow {
        ProjectedAttributeRow {
            key: ProjectedAttributeKey {
                entity_key: EntityKey::try_from(1).expect("a positive key"),
                attribute_name_id: 7_i32.into(),
                dataset_id: None,
            },
            attribute_kind: AttributeKind::Property,
            value,
            unit_code_id: None,
        }
    }

    #[test]
    fn a_number_reaches_the_column_as_the_decimal_literal_numeric_parses() {
        // One above 2^53, which the double format cannot name individually.
        let value = ProjectedAttributeValue::Number(JsonNumber::from(9_007_199_254_740_993_i64));
        let columns = typed_value_columns(&value);
        pretty_assertions::assert_eq!(columns.value_number.map(ToString::to_string).as_deref(), Some("9007199254740993"));
    }

    #[test]
    fn every_value_shape_populates_exactly_one_typed_column() {
        let instant = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let values = [
            ProjectedAttributeValue::Number(JsonNumber::try_from(-2.5e-4).expect("a finite number")),
            ProjectedAttributeValue::String("text".to_owned()),
            ProjectedAttributeValue::Boolean(false),
            ProjectedAttributeValue::Object(serde_json::json!({"a": 1})),
            ProjectedAttributeValue::DateTime(instant),
            ProjectedAttributeValue::Uri("urn:ngsi-ld:Entity:target".parse().expect("a valid URI")),
        ];
        for value in &values {
            let columns = typed_value_columns(value);
            let populated = usize::from(columns.value_number.is_some())
                + usize::from(columns.value_string.is_some())
                + usize::from(columns.value_bool.is_some())
                + usize::from(columns.value_geo.is_some())
                + usize::from(columns.value_object.is_some())
                + usize::from(columns.value_datetime.is_some())
                + usize::from(columns.value_uri.is_some());
            pretty_assertions::assert_eq!(populated, 1, "{value:?} populates {populated} columns");
        }
    }

    #[test]
    fn the_binds_declare_the_shape_the_value_actually_occupies() {
        let rows = [row(ProjectedAttributeValue::Boolean(true))];
        let binds = ProjectionBinds::from_rows(&rows).expect("no geometry to encode");
        pretty_assertions::assert_eq!(binds.value_kinds, vec![ValueKind::code(ValueKind::Boolean)]);
        pretty_assertions::assert_eq!(binds.value_bools, vec![Some(true)]);
        pretty_assertions::assert_eq!(binds.value_numbers, vec![None]);
        pretty_assertions::assert_eq!(binds.entity_keys, vec![1_i64]);
        pretty_assertions::assert_eq!(binds.attribute_name_ids, vec![7_i32]);
    }

    #[test]
    fn a_default_instance_binds_an_absent_dataset_identifier() {
        let rows = [row(ProjectedAttributeValue::String("text".to_owned()))];
        let binds = ProjectionBinds::from_rows(&rows).expect("no geometry to encode");
        pretty_assertions::assert_eq!(binds.dataset_ids, vec![None]);
    }
}
