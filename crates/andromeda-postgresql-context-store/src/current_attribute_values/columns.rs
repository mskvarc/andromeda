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
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use andromeda_postgresql_schema::projected_string_column::ProjectedStringColumn;
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// The typed columns one projected value occupies.
///
/// Exactly one field is populated, decided by the value's own shape rather than
/// by the caller, which is what keeps the `value_kind` code and the column
/// holding the value from ever disagreeing. A String is placed by
/// [`ProjectedStringColumn::of`], the one placement the `q` compiler also
/// routes an equality through.
struct TypedValueColumns<'a> {
    /// The arbitrary-precision value bound directly as `NUMERIC`.
    number: Option<&'a BigDecimal>,
    /// The string value, when it fits `value_string`'s B-tree.
    text: Option<&'a str>,
    /// The string value, when it is too long for `value_string`.
    long_text: Option<&'a str>,
    /// The boolean value.
    boolean: Option<bool>,
    /// The geometry value, before EWKB encoding.
    geometry: Option<&'a Geometry>,
    /// The structured value.
    object: Option<&'a Value>,
    /// The timestamp value.
    datetime: Option<DateTime<Utc>>,
    /// The URI value.
    uri: Option<&'a str>,
}

/// Spread one projected value across the typed columns.
fn typed_value_columns(value: &ProjectedAttributeValue) -> TypedValueColumns<'_> {
    let mut columns = TypedValueColumns {
        number: None,
        text: None,
        long_text: None,
        boolean: None,
        geometry: None,
        object: None,
        datetime: None,
        uri: None,
    };
    match value {
        ProjectedAttributeValue::Number(number) => columns.number = Some(number.decimal()),
        ProjectedAttributeValue::String(text) => match ProjectedStringColumn::of(text) {
            ProjectedStringColumn::Indexed => columns.text = Some(text),
            ProjectedStringColumn::Overflow => columns.long_text = Some(text),
        },
        ProjectedAttributeValue::Boolean(flag) => columns.boolean = Some(*flag),
        ProjectedAttributeValue::Geometry(geometry) => columns.geometry = Some(geometry),
        ProjectedAttributeValue::Object(object) => columns.object = Some(object),
        ProjectedAttributeValue::DateTime(instant) => columns.datetime = Some(*instant),
        ProjectedAttributeValue::Uri(uri) => columns.uri = Some(uri.as_str()),
    }
    columns
}

/// Column-major binds for one projection batch.
pub struct ProjectionBinds<'a> {
    /// Surrogate key of the entity each row belongs to.
    pub entity_keys: Vec<i64>,
    /// Dictionary key of each row's attribute IRI.
    pub attribute_name_keys: Vec<i32>,
    /// Attribute family each row was written as.
    pub attribute_kinds: Vec<i16>,
    /// Stored value shape of each row.
    pub value_kinds: Vec<i16>,
    /// Timestamp values.
    pub value_date_times: Vec<Option<DateTime<Utc>>>,
    /// The clause 4.22 `expiresAt` of each instance, absent when it never expires.
    pub expires_ats: Vec<Option<DateTime<Utc>>>,
    /// Catalog identities of the instances' units.
    pub unit_code_ids: Vec<Option<i16>>,
    /// Boolean values.
    pub value_booleans: Vec<Option<bool>>,
    /// `datasetId` of each row, absent on a default instance.
    pub dataset_ids: Vec<Option<&'a str>>,
    /// Arbitrary-precision values bound as a `numeric[]`.
    pub value_numbers: Vec<Option<BigDecimal>>,
    /// String values.
    pub value_strings: Vec<Option<&'a str>>,
    /// Geometry values as EWKB, projected server-side.
    pub value_geometries: Vec<Option<Vec<u8>>>,
    /// Structured values.
    pub value_objects: Vec<Option<&'a Value>>,
    /// URI values, which is how a relationship object is projected.
    pub value_uris: Vec<Option<&'a str>>,
    /// String values too long for `value_string`, under the String value kind.
    pub value_long_texts: Vec<Option<&'a str>>,
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
            attribute_name_keys: Vec::with_capacity(rows.len()),
            attribute_kinds: Vec::with_capacity(rows.len()),
            value_kinds: Vec::with_capacity(rows.len()),
            value_date_times: Vec::with_capacity(rows.len()),
            expires_ats: Vec::with_capacity(rows.len()),
            unit_code_ids: Vec::with_capacity(rows.len()),
            value_booleans: Vec::with_capacity(rows.len()),
            dataset_ids: Vec::with_capacity(rows.len()),
            value_numbers: Vec::with_capacity(rows.len()),
            value_strings: Vec::with_capacity(rows.len()),
            value_geometries: Vec::with_capacity(rows.len()),
            value_objects: Vec::with_capacity(rows.len()),
            value_uris: Vec::with_capacity(rows.len()),
            value_long_texts: Vec::with_capacity(rows.len()),
        };
        for row in rows {
            let value = typed_value_columns(&row.value);
            binds.entity_keys.push(EntityKey::get(row.key.entity_key));
            binds.attribute_name_keys.push(AttributeNameKey::get(row.key.attribute_name_key));
            binds.attribute_kinds.push(AttributeKind::code(row.attribute_kind));
            binds.value_kinds.push(ValueKind::code(row.value.kind()));
            binds.value_date_times.push(value.datetime);
            binds.expires_ats.push(row.expires_at);
            binds.unit_code_ids.push(row.unit_code_id.map(UnitCodeId::get));
            binds.value_booleans.push(value.boolean);
            binds.dataset_ids.push(row.key.dataset_id.as_ref().map(Uri::as_str));
            // SQLx owns array elements for the duration of the query. Clone the
            // decimal itself, preserving its exact value without formatting it.
            binds.value_numbers.push(value.number.cloned());
            binds.value_strings.push(value.text);
            binds.value_geometries.push(geometry_bytes(value.geometry)?);
            binds.value_objects.push(value.object);
            binds.value_uris.push(value.uri);
            binds.value_long_texts.push(value.long_text);
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
    use andromeda_postgresql_schema::projected_string_column::INDEXED_STRING_BYTE_LIMIT;
    use chrono::{TimeZone, Utc};

    /// One projection row carrying `value`, with no unit and no `datasetId`.
    fn row(value: ProjectedAttributeValue) -> ProjectedAttributeRow {
        ProjectedAttributeRow {
            key: ProjectedAttributeKey {
                entity_key: EntityKey::try_from(1).expect("a positive key"),
                attribute_name_key: 7_i32.into(),
                dataset_id: None,
            },
            attribute_kind: AttributeKind::Property,
            value,
            unit_code_id: None,
            expires_at: None,
        }
    }

    #[test]
    fn a_number_reaches_the_column_as_the_decimal_literal_numeric_parses() {
        // One above 2^53, which the double format cannot name individually.
        let value = ProjectedAttributeValue::Number(JsonNumber::from(9_007_199_254_740_993_i64));
        let columns = typed_value_columns(&value);
        pretty_assertions::assert_eq!(columns.number.map(ToString::to_string).as_deref(), Some("9007199254740993"));
    }

    #[test]
    fn every_value_shape_populates_exactly_one_typed_column() {
        let instant = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let values = [
            ProjectedAttributeValue::Number(JsonNumber::try_from(-2.5e-4).expect("a finite number")),
            ProjectedAttributeValue::String("text".to_owned()),
            ProjectedAttributeValue::String("x".repeat(3000)),
            ProjectedAttributeValue::Boolean(false),
            ProjectedAttributeValue::Object(serde_json::json!({"a": 1})),
            ProjectedAttributeValue::DateTime(instant),
            ProjectedAttributeValue::Uri("urn:ngsi-ld:Entity:target".parse().expect("a valid URI")),
        ];
        for value in &values {
            let columns = typed_value_columns(value);
            let populated = usize::from(columns.number.is_some())
                + usize::from(columns.text.is_some())
                + usize::from(columns.long_text.is_some())
                + usize::from(columns.boolean.is_some())
                + usize::from(columns.geometry.is_some())
                + usize::from(columns.object.is_some())
                + usize::from(columns.datetime.is_some())
                + usize::from(columns.uri.is_some());
            pretty_assertions::assert_eq!(populated, 1, "{value:?} populates {populated} columns");
        }
    }

    #[test]
    fn the_binds_declare_the_shape_the_value_actually_occupies() {
        let rows = [row(ProjectedAttributeValue::Boolean(true))];
        let binds = ProjectionBinds::from_rows(&rows).expect("no geometry to encode");
        pretty_assertions::assert_eq!(binds.value_kinds, vec![ValueKind::code(ValueKind::Boolean)]);
        pretty_assertions::assert_eq!(binds.value_booleans, vec![Some(true)]);
        pretty_assertions::assert_eq!(binds.value_numbers, vec![None]);
        pretty_assertions::assert_eq!(binds.entity_keys, vec![1_i64]);
        pretty_assertions::assert_eq!(binds.attribute_name_keys, vec![7_i32]);
    }

    #[test]
    fn a_string_within_the_limit_binds_the_indexed_column() {
        let text = "a".repeat(INDEXED_STRING_BYTE_LIMIT);
        let rows = [row(ProjectedAttributeValue::String(text.clone()))]; // clone: the bind is compared against the original
        let binds = ProjectionBinds::from_rows(&rows).expect("no geometry to encode");
        pretty_assertions::assert_eq!(binds.value_strings, vec![Some(text.as_str())]);
        pretty_assertions::assert_eq!(binds.value_long_texts, vec![None]);
    }

    #[test]
    fn a_string_past_the_limit_binds_the_overflow_column_under_the_string_kind() {
        let text = "a".repeat(INDEXED_STRING_BYTE_LIMIT + 1);
        let rows = [row(ProjectedAttributeValue::String(text.clone()))]; // clone: the bind is compared against the original
        let binds = ProjectionBinds::from_rows(&rows).expect("no geometry to encode");
        pretty_assertions::assert_eq!(binds.value_kinds, vec![ValueKind::code(ValueKind::String)]);
        pretty_assertions::assert_eq!(binds.value_strings, vec![None]);
        pretty_assertions::assert_eq!(binds.value_long_texts, vec![Some(text.as_str())]);
    }

    #[test]
    fn each_row_binds_its_own_expiry_beside_its_value() {
        let expires_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let mut expiring = row(ProjectedAttributeValue::Boolean(true));
        expiring.expires_at = Some(expires_at);
        let rows = [expiring, row(ProjectedAttributeValue::Boolean(false))];
        let binds = ProjectionBinds::from_rows(&rows).expect("no geometry to encode");
        pretty_assertions::assert_eq!(binds.expires_ats, vec![Some(expires_at), None]);
    }

    #[test]
    fn a_default_instance_binds_an_absent_dataset_identifier() {
        let rows = [row(ProjectedAttributeValue::String("text".to_owned()))];
        let binds = ProjectionBinds::from_rows(&rows).expect("no geometry to encode");
        pretty_assertions::assert_eq!(binds.dataset_ids, vec![None]);
    }
}
