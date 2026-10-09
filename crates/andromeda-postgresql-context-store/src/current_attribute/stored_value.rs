//! The value one attribute instance stores, by the typed column or field that
//! holds it.
//!
//! A row and an element hold the same value shapes; the only difference is the
//! length split a row makes, where a String or a URI too long for its B-tree
//! moves to an overflow column under the same value kind. An element has no
//! B-tree, so it has no overflow field.

use crate::current_attribute::stored_json::StoredJson;
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_postgresql_codes::value_kind::ValueKind;
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use andromeda_postgresql_schema::{current::CurrentAttributes, projected_string_column::ProjectedStringColumn, projected_uri_column::ProjectedUriColumn};
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
    /// A clause 4.6.3 `DateTime`, held as its instant.
    DateTime {
        /// The instant, at microsecond resolution.
        instant: DateTime<Utc>,
        /// The `DateTime` datatype when the literal declares it; `None` for a
        /// string recognised by its lexical form.
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
}

impl StoredValue {
    /// The value kind code the row or element records for this value.
    #[must_use]
    pub const fn kind(&self) -> ValueKind {
        match self {
            StoredValue::Number { .. } => ValueKind::Number,
            StoredValue::String { .. } => ValueKind::String,
            StoredValue::Boolean { .. } => ValueKind::Boolean,
            StoredValue::DateTime { .. } => ValueKind::DateTime,
            StoredValue::Geometry { .. } => ValueKind::Geometry,
            StoredValue::Object(_) => ValueKind::Object,
            StoredValue::Uri(_) => ValueKind::Uri,
        }
    }

    /// The `entity_type_names` key of the datatype the value declares.
    #[must_use]
    pub const fn datatype_key(&self) -> Option<EntityTypeNameKey> {
        match self {
            StoredValue::Number { datatype, .. } | StoredValue::Boolean { datatype, .. } | StoredValue::DateTime { datatype, .. } => *datatype,
            StoredValue::String { annotation, .. } => match annotation {
                LiteralAnnotation::Datatype(key) => Some(*key),
                LiteralAnnotation::Plain | LiteralAnnotation::Language(_) => None,
            },
            StoredValue::Geometry { .. } | StoredValue::Object(_) | StoredValue::Uri(_) => None,
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
            | StoredValue::DateTime { .. }
            | StoredValue::Geometry { .. }
            | StoredValue::Object(_)
            | StoredValue::Uri(_) => None,
        }
    }

    /// The `current_attributes` column a row holds the value in: a String or a
    /// URI past its B-tree bound moves to its overflow column.
    #[must_use]
    pub fn row_column(&self) -> CurrentAttributes {
        match self {
            StoredValue::Number { .. } => CurrentAttributes::ValueNumber,
            StoredValue::String { text, .. } => match ProjectedStringColumn::of(text) {
                ProjectedStringColumn::Indexed => CurrentAttributes::ValueString,
                ProjectedStringColumn::Overflow => CurrentAttributes::ValueLongText,
            },
            StoredValue::Boolean { .. } => CurrentAttributes::ValueBoolean,
            StoredValue::DateTime { .. } => CurrentAttributes::ValueDateTime,
            StoredValue::Geometry { .. } => CurrentAttributes::ValueGeometry,
            StoredValue::Object(_) => CurrentAttributes::ValueObject,
            StoredValue::Uri(uri) => match ProjectedUriColumn::of(uri.as_str()) {
                ProjectedUriColumn::Indexed => CurrentAttributes::ValueUri,
                ProjectedUriColumn::Overflow => CurrentAttributes::ValueLongUri,
            },
        }
    }
}
