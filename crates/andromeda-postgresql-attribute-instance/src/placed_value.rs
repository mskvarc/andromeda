//! A stored value split into the one place its value kind is held in.
//!
//! Every writer of a stored instance, of a row, an element or a history row,
//! takes the value through [`PlacedValue`], so they all put a kind in the field
//! the placement table of the stored values names for it, and each only binds
//! the field it is handed. A row's length split is applied by the row writers
//! to the String and URI fields they are handed.

use crate::{stored_json::StoredJson, stored_number::StoredNumber, stored_proof::StoredProof, stored_value::StoredValue};
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_stored_values::value_placement::ValueField;
use chrono::{DateTime, Utc};

/// A stored value in the one place its kind is held in.
#[derive(Debug)]
pub enum PlacedValue {
    /// In one value field.
    Field(FieldValue),
    /// In the proof fields of an element: a data-integrity seal, which is only
    /// ever a sub-attribute (clause 4.5.2.2).
    Proof(Box<StoredProof>),
}

/// The content of one value field.
#[derive(Debug)]
pub enum FieldValue {
    /// The number field: a JSON number, or a JSON string declared as an XSD
    /// number.
    Number(StoredNumber),
    /// The String field: a String as it is, a `Date` in its one spelling, a
    /// `Time` in its fixed-width storage encoding.
    String(String),
    /// The boolean field: a JSON boolean, or a JSON string declared as
    /// `xsd:boolean`.
    Boolean(bool),
    /// The geometry fields.
    Geometry {
        /// The positions.
        form: GeometryForm,
        /// The `bbox` and foreign members (RFC 7946 sections 5 and 6.1), in
        /// their stored JSON.
        members: Option<StoredJson>,
    },
    /// The structured field: every value that is not one scalar.
    Object(StoredJson),
    /// The instant field: a value declared as a clause 4.6.3 `DateTime`.
    DateTime(DateTime<Utc>),
    /// The URI field: the single object of a Relationship.
    Uri(Uri),
}

impl FieldValue {
    /// The field this content is held in.
    #[must_use]
    pub const fn field(&self) -> ValueField {
        match self {
            FieldValue::Number(_) => ValueField::Number,
            FieldValue::String(_) => ValueField::String,
            FieldValue::Boolean(_) => ValueField::Boolean,
            FieldValue::Geometry { .. } => ValueField::Geometry,
            FieldValue::Object(_) => ValueField::Object,
            FieldValue::DateTime(_) => ValueField::DateTime,
            FieldValue::Uri(_) => ValueField::Uri,
        }
    }
}

impl From<StoredValue> for PlacedValue {
    fn from(value: StoredValue) -> PlacedValue {
        PlacedValue::Field(match value {
            StoredValue::Number { number, .. } => FieldValue::Number(StoredNumber::from(number)),
            StoredValue::DeclaredNumber { number, .. } => FieldValue::Number(StoredNumber::from(number)),
            StoredValue::String { text, .. } => FieldValue::String(text),
            StoredValue::Date { date, .. } => FieldValue::String(date.to_string()),
            StoredValue::Time { time, .. } => FieldValue::String(time.to_string()),
            StoredValue::Boolean { flag, .. } | StoredValue::DeclaredBoolean { flag, .. } => FieldValue::Boolean(flag),
            StoredValue::Geometry { form, members } => FieldValue::Geometry { form, members },
            StoredValue::Object(object) => FieldValue::Object(object),
            StoredValue::DateTime { instant, .. } => FieldValue::DateTime(instant),
            StoredValue::Uri(uri) => FieldValue::Uri(uri),
            StoredValue::Proof(proof) => return PlacedValue::Proof(proof),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        placed_value::{FieldValue, PlacedValue},
        stored_json::StoredJson,
        stored_proof::StoredProof,
        stored_value::{LiteralAnnotation, StoredValue},
    };
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_ngsi_ld_number::{
        JsonNumber,
        declared_number::{DeclaredNumber, NonFinite},
    };
    use andromeda_postgresql_stored_values::{
        entity_type_name_key::EntityTypeNameKey,
        value_kind::ValueKind,
        value_placement::{ValuePlacement, value_placement},
    };
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{json, value::to_raw_value};

    /// A stored value of `kind`; a tombstone is no stored value.
    fn stored(kind: ValueKind) -> Option<StoredValue> {
        Some(match kind {
            ValueKind::Tombstone => return None,
            ValueKind::Number => StoredValue::Number {
                number: JsonNumber::from(1),
                datatype: None,
            },
            ValueKind::String => StoredValue::String {
                text: "red".to_owned(),
                annotation: LiteralAnnotation::Plain,
            },
            ValueKind::Boolean => StoredValue::Boolean { flag: true, datatype: None },
            ValueKind::Geometry => StoredValue::Geometry {
                form: serde_json::from_value::<Geometry>(json!({"type": "Point", "coordinates": [13.5, 52.25]}))
                    .expect("a point")
                    .into_parts()
                    .0,
                members: None,
            },
            ValueKind::Object => StoredValue::Object(StoredJson::new(to_raw_value(&json!([1, 2])).expect("JSON"))),
            ValueKind::DateTime => StoredValue::DateTime {
                instant: Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).single().expect("a valid instant"),
                datatype: None,
            },
            ValueKind::Uri => StoredValue::Uri("urn:ngsi-ld:Building:1".parse().expect("a URI")),
            ValueKind::Date => StoredValue::Date {
                date: "2024-01-15".parse().expect("a date"),
                datatype: None,
            },
            ValueKind::Time => StoredValue::Time {
                time: "12:00:00Z".parse().expect("a time"),
                datatype: None,
            },
            ValueKind::DeclaredNumber => StoredValue::DeclaredNumber {
                number: DeclaredNumber::NonFinite(NonFinite::PositiveInfinity),
                datatype: None,
            },
            ValueKind::DeclaredBoolean => StoredValue::DeclaredBoolean { flag: false, datatype: None },
            ValueKind::DataIntegrityProof => StoredValue::Proof(Box::new(StoredProof {
                entity_id: "urn:ngsi-ld:Store:002".parse().expect("an identifier"),
                entity_type_keys: vec![EntityTypeNameKey::from(3)],
                created: "2025-01-27T21:02:24Z".parse().expect("a DateTime"),
                verification_method: "https://example.edu/issuers/565049#key".parse().expect("a URI"),
                cryptosuite: "eddsa-rdfc-2022".parse().expect("a suite"),
                proof_purpose: "https://w3id.org/security#assertionMethod".parse().expect("a URI"),
                proof_value: "z3XrH3".parse().expect("a signature"),
                members: None,
            })),
        })
    }

    /// The writers' one split and the placement table the readers derive from
    /// put every kind in the same place.
    #[test]
    fn every_value_kind_is_split_into_the_place_the_placement_table_names() {
        for kind in ValueKind::ALL.iter().copied() {
            let value = stored(kind);
            assert_eq!(value.as_ref().map(StoredValue::kind), value.is_some().then_some(kind), "{kind:?}");
            match (value_placement(kind), value.map(PlacedValue::from)) {
                (ValuePlacement::Field { field, .. }, Some(PlacedValue::Field(content))) => assert_eq!(content.field(), field, "{kind:?}"),
                (ValuePlacement::Proof, Some(PlacedValue::Proof(_))) | (ValuePlacement::Valueless, None) => {}
                (placement, placed) => panic!("{kind:?} is placed as {placement:?} but split as {placed:?}"),
            }
        }
    }

    /// A `Date` and a `Time` are held in the String field as their storage
    /// text, which is what a reader binds an operand of their kind as.
    #[test]
    fn a_date_and_a_time_are_split_into_their_storage_text() {
        for (kind, text) in [(ValueKind::Date, "2024-01-15"), (ValueKind::Time, "12:00:00.000000Z")] {
            let Some(PlacedValue::Field(FieldValue::String(held))) = stored(kind).map(PlacedValue::from) else {
                panic!("{kind:?} is held in the String field");
            };
            assert_eq!(held, text, "{kind:?}");
        }
    }
}
