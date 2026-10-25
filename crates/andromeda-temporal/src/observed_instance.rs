//! Reading one written attribute instance as the observation a temporal history
//! row records.
//!
//! Every path that adds to a Temporal Evolution reads its observations here:
//! the observations a Core write makes (clause 4.3.1), the instances a Temporal
//! API write adds (clauses 5.6.11 and 5.6.12), and the replacement a clause
//! 5.6.15 instance modification carries. Each family keeps its value in one
//! member (clause 4.5), and the value lands in the typed column its shape fits,
//! or is kept structured when no column does; what the instance carries beside
//! its value, identity and time axes is its metadata. A written instance
//! holding its family's deletion encoding observes nothing: it ends an
//! instance, which a Temporal API write records as a deletion beside its
//! observations.

use crate::error::TemporalError;
use andromeda_context_commit::submission_refusal::SubmissionRefusal;
use andromeda_json_literal::piece::{JsonKind, JsonPiece};
use andromeda_ngsi_ld_entity::{
    document::{
        instance_document::{InstanceDocument, InstanceDocumentParts},
        slot::Slot,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    geo_value::GeoValue,
    geometry_value::GeometryValue,
    list_object::ListObject,
    literal::{Literal, LiteralValue},
    non_empty::NonEmpty,
    value_mode::Fragment,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_type::AttributeType,
    datatype::primitive_literal::PrimitiveLiteral,
    identifier::Identifier,
    reference::NgsiLdReference,
    time::{
        date_time::NgsiLdDateTime,
        value::{TimeValue, TimeValueDeclaration},
    },
    uri::Uri,
};
use andromeda_ngsi_ld_model::temporal::{
    TemporalValue,
    observation_metadata::ObservationMetadata,
    structured_value::StructuredValue,
    text_annotation::TextAnnotation,
};
use andromeda_ngsi_ld_number::JsonNumber;
use chrono::{DateTime, Utc};
use std::str::FromStr;

/// One written instance, read as the observation a history row records.
#[derive(Clone, Debug, PartialEq)]
pub struct ObservedInstance {
    /// The family the instance belongs to.
    pub attribute_type: AttributeType,
    /// The instance's `datasetId`, absent for the default instance.
    pub dataset_id: Option<Identifier>,
    /// The value, in the typed column its shape fits or structured.
    pub value: TemporalValue,
    /// The `observedAt` the instance declares.
    pub observed_at: Option<DateTime<Utc>>,
    /// The `instanceId` the client gave the instance (clause 4.5.7).
    pub instance_id: Option<Identifier>,
    /// What the instance carries beside its value.
    pub metadata: Option<ObservationMetadata>,
}

impl ObservedInstance {
    /// Read one written instance, `None` when it records no observation: an
    /// instance whose value member is its family's deletion encoding (clause
    /// 4.5.0) ends an instance rather than observing one, and an instance that
    /// names neither a family nor a value observes nothing.
    ///
    /// A family the instance does not declare is the one its value member
    /// implies (clause 4.5.2 reads `value` as a Property's).
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Validation`] when the instance carries no value
    /// of the shape its family requires, declares an `observedAt` that is not a
    /// clause 4.6.3 `DateTime` in UTC, carries a Property value written in a
    /// clause 4.6.3 time form it does not satisfy, sets a companion member
    /// (`expiresAt`, `unitCode`, `valueType`, `objectType`) to the NGSI-LD
    /// Null, or holds a sub-attribute the committed model refuses (the NGSI-LD
    /// Null, a member its family does not define).
    pub fn read(instance: InstanceDocument) -> Result<Option<ObservedInstance>, TemporalError> {
        let InstanceDocumentParts {
            family,
            dataset_id,
            value,
            observed_at,
            expires_at,
            unit_code,
            value_type,
            object_types,
            created_at: _,
            modified_at: _,
            deleted_at: _,
            instance_id,
            proof_seals,
            sub_attributes,
        } = instance.into_parts();
        let (family, written) = match (family, value) {
            (_, ValueSlot::Remove(_)) | (None, ValueSlot::Absent) => return Ok(None),
            (Some(family), ValueSlot::Absent) => return Err(missing_value(family).into()),
            (Some(family), ValueSlot::Set(written)) => (family, written),
            (None, ValueSlot::Set(written)) => (written.carrier().implied_family(), written),
        };
        let value = family_value(family, written, value_type.as_ref().and_then(Slot::as_set))?;
        let observed_at = declared_observed_at(observed_at.as_ref());
        let metadata = ObservationMetadata::admit(InstanceDocumentParts {
            family: None,
            dataset_id: None,
            value: ValueSlot::Absent,
            observed_at: None,
            expires_at,
            unit_code,
            value_type,
            object_types,
            created_at: None,
            modified_at: None,
            deleted_at: None,
            instance_id: None,
            proof_seals,
            sub_attributes,
        })?;
        Ok(Some(ObservedInstance {
            attribute_type: family,
            dataset_id,
            value,
            observed_at,
            instance_id,
            metadata,
        }))
    }
}

/// The instant the written `observedAt` declares, `None` when the instance
/// declares none or removes it.
pub(crate) const fn declared_observed_at(observed_at: Option<&Slot<NgsiLdDateTime>>) -> Option<DateTime<Utc>> {
    match observed_at {
        Some(Slot::Set(instant)) => Some(instant.get()),
        Some(Slot::Remove) | None => None,
    }
}

/// The refusal of an instance of `family` that carries no value member of the
/// shape the family requires.
const fn missing_value(family: AttributeType) -> SubmissionRefusal {
    match family {
        AttributeType::Property => SubmissionRefusal::PropertyMissingValue,
        AttributeType::GeoProperty => SubmissionRefusal::GeoPropertyGeometryInvalid,
        AttributeType::Relationship => SubmissionRefusal::RelationshipMissingObject,
        AttributeType::LanguageProperty => SubmissionRefusal::LanguagePropertyMissingLanguageMap,
        AttributeType::VocabProperty => SubmissionRefusal::VocabPropertyMissingVocab,
        AttributeType::JsonProperty => SubmissionRefusal::JsonPropertyMissingJson,
        AttributeType::ListProperty => SubmissionRefusal::ListPropertyMissingValueList,
        AttributeType::ListRelationship => SubmissionRefusal::ListRelationshipMissingObjectList,
    }
}

/// The observation the value member `written` of an instance of `family`
/// records, beside the `valueType` `value_type` the instance declares.
fn family_value(family: AttributeType, written: WrittenValue, value_type: Option<&NgsiLdReference>) -> Result<TemporalValue, TemporalError> {
    match (family, written) {
        (AttributeType::Property, WrittenValue::Value(values)) => property_value(values, value_type),
        (AttributeType::GeoProperty, WrittenValue::Geometry(geometry)) => Ok(TemporalValue::Geometry(match geometry {
            GeoValue::Geometry(value) => value,
            // The clause 4.7.2 text is not kept, only the positions it parses
            // into: its members are compact JSON no expansion has named.
            // clone: the encoded geometry owns the positions its text parses into.
            GeoValue::Encoded(encoded) => GeometryValue::new(encoded.form().clone()),
        })),
        (AttributeType::Relationship, WrittenValue::Object(targets)) => Ok(relationship_value(targets)),
        (AttributeType::VocabProperty, WrittenValue::Vocab(terms)) => Ok(vocab_value(terms)),
        (AttributeType::LanguageProperty, WrittenValue::LanguageMap(entries)) => Ok(TemporalValue::Structured(StructuredValue::LanguageMap(entries))),
        // A JSON literal is one document; the first one an expansion produced
        // is the value (clause 4.5.24).
        (AttributeType::JsonProperty, WrittenValue::Json(literals)) => Ok(TemporalValue::Structured(StructuredValue::Json(
            literals.into_vec().into_iter().next().ok_or(SubmissionRefusal::JsonPropertyMissingJson)?,
        ))),
        (AttributeType::ListProperty, WrittenValue::ValueList(mut items)) => {
            for item in &mut items {
                item.commit_time_values(value_type);
            }
            Ok(TemporalValue::Structured(StructuredValue::List(items)))
        }
        (AttributeType::ListRelationship, WrittenValue::ObjectList(objects)) => Ok(TemporalValue::Structured(StructuredValue::ObjectList(
            objects
                .into_iter()
                .map(|object| match object {
                    ListObject::Bare(uri) | ListObject::Literal(uri) | ListObject::Wrapped(uri) => uri,
                })
                .collect(),
        ))),
        (
            family,
            WrittenValue::Value(_)
            | WrittenValue::Geometry(_)
            | WrittenValue::Object(_)
            | WrittenValue::LanguageMap(_)
            | WrittenValue::Vocab(_)
            | WrittenValue::Json(_)
            | WrittenValue::ValueList(_)
            | WrittenValue::ObjectList(_),
        ) => Err(missing_value(family).into()),
    }
}

/// A Relationship's targets, as written: one keeps the URI column, several are
/// kept as the ordered array clause 4.5.3.2 admits ("a URI or array of URIs").
fn relationship_value(targets: NonEmpty<Uri>) -> TemporalValue {
    match targets.as_slice() {
        [single] => TemporalValue::Uri(single.clone()), // clone: the one target the URI column keeps
        _ => TemporalValue::Structured(StructuredValue::Objects(targets)),
    }
}

/// A `VocabProperty`'s terms: one keeps the string column, several are kept
/// structured.
fn vocab_value(terms: NonEmpty<NgsiLdReference>) -> TemporalValue {
    match terms.as_slice() {
        [single] => TemporalValue::String {
            text: single.as_str().to_owned(),
            annotation: TextAnnotation::Plain,
        },
        _ => TemporalValue::Structured(StructuredValue::Vocabs(terms)),
    }
}

/// A Property's value: a single literal with no `@direction` lands in its
/// typed column, as current state holds it, anything else is kept structured.
///
/// Clause 4.9 makes an array-valued Property's target value the whole array
/// ("is included in the target value, if the latter is an array"), and JSON-LD
/// expands `"value": ["a","b"]` into one value per element, so several values
/// are kept together, and the empty array, which expands to none, is kept as
/// the empty array.
///
/// A declared time value is recorded in its served spelling, at every depth of
/// a structured value too, as current state holds it.
fn property_value(mut values: Vec<ExpandedValue<Fragment>>, value_type: Option<&NgsiLdReference>) -> Result<TemporalValue, TemporalError> {
    let scalar = match values.as_slice() {
        [ExpandedValue::Literal(literal)] if literal.direction().is_none() => Some(scalar_value(literal, value_type)),
        [ExpandedValue::Json(literal)] => json_scalar_value(literal.piece()),
        [ExpandedValue::Literal(_) | ExpandedValue::Node(_) | ExpandedValue::List(_)] | [] | [_, _, ..] => None,
    };
    if let Some(column) = scalar {
        return column;
    }
    for value in &mut values {
        value.commit_time_values(value_type);
    }
    Ok(TemporalValue::Structured(StructuredValue::Values(values)))
}

/// The column a scalar value object beside the `valueType` `value_type` lands
/// in, keeping the datatype or language the value object declares.
fn scalar_value(literal: &Literal, value_type: Option<&NgsiLdReference>) -> Result<TemporalValue, TemporalError> {
    let datatype = literal.datatype().cloned(); // clone: the observation owns the datatype the literal declares
    match literal.value() {
        LiteralValue::Number(number) => Ok(TemporalValue::Number {
            number: number.clone(), // clone: the observation owns its exact number
            datatype,
        }),
        LiteralValue::Boolean(flag) => Ok(TemporalValue::Boolean { flag: *flag, datatype }),
        LiteralValue::String(text) => text_value(TimeValueDeclaration::new(literal.datatype(), value_type), text, literal),
    }
}

/// The column a JSON literal holding a scalar lands in, `None` for one holding
/// an array or an object, which is kept structured.
fn json_scalar_value(json: JsonPiece<'_>) -> Option<Result<TemporalValue, TemporalError>> {
    match json.kind() {
        JsonKind::Number => Some(
            JsonNumber::from_str(json.text())
                .map(|number| TemporalValue::Number { number, datatype: None })
                .map_err(|source| TemporalError::from(SubmissionRefusal::PropertyNumber(source))),
        ),
        JsonKind::String => json.as_str().map(|text| Ok(plain_text(text.into_owned()))),
        JsonKind::Boolean => json.as_bool().map(|flag| Ok(TemporalValue::Boolean { flag, datatype: None })),
        JsonKind::Null => Some(Ok(plain_text("null".to_owned()))),
        JsonKind::Array | JsonKind::Object => None,
    }
}

/// A string Property value of the value object `literal`: the time value it is
/// declared as (annex C.6), or the string it is, as written, with the
/// datatype or language `literal` declares.
///
/// A `DateTime` reaches its own `timestamptz` column, where the instant is
/// what is compared. A `Date` and a `Time` reach the string column under their
/// own value kinds, a `Time` in its fixed-width storage encoding, so byte-wise
/// order over the column is the chronological order clause 4.9 asks for. A
/// time value keeps the datatype its literal declares, none when the
/// `valueType` declares the form.
fn text_value(declaration: TimeValueDeclaration<'_>, text: &str, literal: &Literal) -> Result<TemporalValue, TemporalError> {
    let datatype = || literal.datatype().cloned(); // clone: the observation owns the datatype the literal declares
    match TimeValue::declared(declaration, PrimitiveLiteral::String(text)) {
        Some(Ok(TimeValue::DateTime(instant))) => Ok(TemporalValue::DateTime {
            instant: instant.get(),
            datatype: datatype(),
        }),
        Some(Ok(TimeValue::Date(date))) => Ok(TemporalValue::Date { date, datatype: datatype() }),
        Some(Ok(TimeValue::Time(time))) => Ok(TemporalValue::Time { time, datatype: datatype() }),
        Some(Err(source)) => Err(SubmissionRefusal::TimeValueProperty(source).into()),
        None => Ok(TemporalValue::String {
            text: text.to_owned(),
            annotation: match (datatype(), literal.language()) {
                (Some(datatype), _) => TextAnnotation::Datatype(datatype),
                // clone: the observation owns the tag the literal declares
                (None, Some(language)) => TextAnnotation::Language(language.clone()),
                (None, None) => TextAnnotation::Plain,
            },
        }),
    }
}

/// A plain string value.
const fn plain_text(text: String) -> TemporalValue {
    TemporalValue::String {
        text,
        annotation: TextAnnotation::Plain,
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
