//! The value one attribute instance stores, by the typed column or field that
//! holds it.
//!
//! A row and an element hold the same value shapes; the only difference is the
//! length split a row makes, where a String or a URI too long for its B-tree
//! moves to an overflow column under the same value kind. An element has no
//! B-tree, so it has no overflow field.

use crate::{stored_json::StoredJson, stored_proof::StoredProof};
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::{
    time::{date::NgsiLdDate, time_of_day::NgsiLdTime},
    uri::Uri,
};
use andromeda_ngsi_ld_number::{JsonNumber, declared_number::DeclaredNumber};
use andromeda_postgresql_codes::value_kind::ValueKind;
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use chrono::{DateTime, Utc};
use langtag::LangTagBuf;

/// What a string literal carries beside its text.
#[derive(Clone, Debug, PartialEq)]
pub enum LiteralAnnotation {
    /// A plain string.
    Plain,
    /// A typed literal: the `entity_type_names` key of its datatype IRI.
    Datatype(EntityTypeNameKey),
    /// A language-tagged string.
    Language(LangTagBuf),
}

/// The value of one instance, in the column or field that holds it.
///
/// Each variant is one value kind, so a value in two columns is not a state
/// this type can reach.
#[derive(Debug, PartialEq)]
pub enum StoredValue {
    /// A number, held exactly.
    Number {
        /// The number.
        number: JsonNumber,
        /// The datatype a typed number declares.
        datatype: Option<EntityTypeNameKey>,
    },
    /// A string: a plain, typed or language-tagged literal, a single
    /// `VocabProperty` term, or a clause 4.7.2 encoded geometry.
    String {
        /// The text.
        text: String,
        /// The datatype or language the literal carries.
        annotation: LiteralAnnotation,
    },
    /// A boolean.
    Boolean {
        /// The boolean.
        flag: bool,
        /// The datatype a typed boolean declares.
        datatype: Option<EntityTypeNameKey>,
    },
    /// A JSON string declared as an XSD number, held as the number it names.
    DeclaredNumber {
        /// The number.
        number: DeclaredNumber,
        /// The datatype when the literal declares it; `None` when the
        /// attribute's `valueType` does.
        datatype: Option<EntityTypeNameKey>,
    },
    /// A JSON string declared as `xsd:boolean`, held as the Boolean it names.
    DeclaredBoolean {
        /// The Boolean.
        flag: bool,
        /// The datatype when the literal declares it; `None` when the
        /// attribute's `valueType` does.
        datatype: Option<EntityTypeNameKey>,
    },
    /// A value declared as a clause 4.6.3 `DateTime`, held as its instant.
    DateTime {
        /// The instant, at microsecond resolution.
        instant: DateTime<Utc>,
        /// The `DateTime` datatype when the literal declares it; `None` when
        /// the attribute's `valueType` does.
        datatype: Option<EntityTypeNameKey>,
    },
    /// A value declared as a clause 4.6.3 `Date`.
    Date {
        /// The date.
        date: NgsiLdDate,
        /// The `Date` datatype when the literal declares it; `None` when the
        /// attribute's `valueType` does.
        datatype: Option<EntityTypeNameKey>,
    },
    /// A value declared as a clause 4.6.3 `Time`, held in its fixed-width
    /// storage encoding.
    Time {
        /// The time of day, at microsecond resolution.
        time: NgsiLdTime,
        /// The `Time` datatype when the literal declares it; `None` when the
        /// attribute's `valueType` does.
        datatype: Option<EntityTypeNameKey>,
    },
    /// A geometry.
    Geometry {
        /// The positions.
        form: GeometryForm,
        /// The `bbox` and foreign members (RFC 7946 sections 5 and 6.1), in
        /// their stored JSON.
        members: Option<StoredJson>,
    },
    /// Every value that is not one scalar, in the stored JSON of the
    /// structured value codec.
    Object(StoredJson),
    /// A single Relationship object.
    Uri(Uri),
    /// The seals and proof of a data-integrity seal (clause 4.5.2.2), which
    /// only a `sub_attribute` element holds.
    Proof(Box<StoredProof>),
}

impl StoredValue {
    /// The value kind code the row or element records for this value.
    #[must_use]
    pub const fn kind(&self) -> ValueKind {
        match self {
            StoredValue::Number { .. } => ValueKind::Number,
            StoredValue::String { .. } => ValueKind::String,
            StoredValue::Boolean { .. } => ValueKind::Boolean,
            StoredValue::DeclaredNumber { .. } => ValueKind::DeclaredNumber,
            StoredValue::DeclaredBoolean { .. } => ValueKind::DeclaredBoolean,
            StoredValue::DateTime { .. } => ValueKind::DateTime,
            StoredValue::Date { .. } => ValueKind::Date,
            StoredValue::Time { .. } => ValueKind::Time,
            StoredValue::Geometry { .. } => ValueKind::Geometry,
            StoredValue::Object(_) => ValueKind::Object,
            StoredValue::Uri(_) => ValueKind::Uri,
            StoredValue::Proof(_) => ValueKind::DataIntegrityProof,
        }
    }

    /// The `entity_type_names` key of the datatype the value declares.
    #[must_use]
    pub const fn datatype_key(&self) -> Option<EntityTypeNameKey> {
        match self {
            StoredValue::Number { datatype, .. }
            | StoredValue::Boolean { datatype, .. }
            | StoredValue::DeclaredNumber { datatype, .. }
            | StoredValue::DeclaredBoolean { datatype, .. }
            | StoredValue::DateTime { datatype, .. }
            | StoredValue::Date { datatype, .. }
            | StoredValue::Time { datatype, .. } => *datatype,
            StoredValue::String { annotation, .. } => match annotation {
                LiteralAnnotation::Datatype(key) => Some(*key),
                LiteralAnnotation::Plain | LiteralAnnotation::Language(_) => None,
            },
            StoredValue::Geometry { .. } | StoredValue::Object(_) | StoredValue::Uri(_) | StoredValue::Proof(_) => None,
        }
    }

    /// The language tag the value carries.
    #[must_use]
    pub const fn language(&self) -> Option<&LangTagBuf> {
        match self {
            StoredValue::String {
                annotation: LiteralAnnotation::Language(language),
                ..
            } => Some(language),
            StoredValue::String { .. }
            | StoredValue::Number { .. }
            | StoredValue::Boolean { .. }
            | StoredValue::DeclaredNumber { .. }
            | StoredValue::DeclaredBoolean { .. }
            | StoredValue::DateTime { .. }
            | StoredValue::Date { .. }
            | StoredValue::Time { .. }
            | StoredValue::Geometry { .. }
            | StoredValue::Object(_)
            | StoredValue::Uri(_)
            | StoredValue::Proof(_) => None,
        }
    }
}
