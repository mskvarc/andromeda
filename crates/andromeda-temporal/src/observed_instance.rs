//! Reading one written attribute instance as the observation a temporal history
//! row records.
//!
//! Every path that adds to a Temporal Evolution reads its instances here: the
//! observations a Core write makes (clause 4.3.1), the instances a Temporal API
//! write adds (clauses 5.6.11 and 5.6.12), and the replacement a clause 5.6.15
//! instance modification carries. Each family keeps its value in one member
//! (clause 4.5), and the value lands in the typed column its shape fits, or is
//! kept structured when no column does; what the instance carries beside its
//! value, identity and time axes is its metadata.

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
    identifier::Identifier,
    ngsi_ld_date_time::NgsiLdDateTime,
    reference::NgsiLdReference,
    time_value::TimeValue,
    uri::Uri,
};
use andromeda_ngsi_ld_model::temporal::{TemporalValue, observation_metadata::ObservationMetadata, structured_value::StructuredValue};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_render::uri_decoding::decode_percent_encoded_uri;
use chrono::{DateTime, Utc};
use std::{borrow::Cow, str::FromStr};

/// The JSON-LD datatype of a JSON literal, which a time value classification
/// reads like any other declared datatype.
const JSON_DATATYPE: &str = "@json";

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
    /// clause 4.6.3 `DateTime` in UTC, or carries a Property value written in
    /// a clause 4.6.3 time form it does not satisfy.
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
            created_at,
            modified_at,
            deleted_at,
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
        let value = family_value(family, written)?;
        let observed_at = match observed_at {
            Some(Slot::Set(text)) => Some(
                text.as_str()
                    .parse::<NgsiLdDateTime>()
                    .map(NgsiLdDateTime::get)
                    .map_err(|source| TemporalError::from(SubmissionRefusal::ObservedAt(source)))?,
            ),
            Some(Slot::Remove) | None => None,
        };
        let metadata = ObservationMetadata::from_parts(InstanceDocumentParts {
            family: None,
            dataset_id: None,
            value: ValueSlot::Absent,
            observed_at: None,
            expires_at,
            unit_code,
            value_type,
            object_types,
            created_at,
            modified_at,
            deleted_at,
            instance_id: None,
            proof_seals,
            sub_attributes,
        });
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
/// records.
fn family_value(family: AttributeType, written: WrittenValue) -> Result<TemporalValue, TemporalError> {
    match (family, written) {
        (AttributeType::Property, WrittenValue::Value(values)) => property_value(values),
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
        (AttributeType::ListProperty, WrittenValue::ValueList(items)) => Ok(TemporalValue::Structured(StructuredValue::List(items))),
        (AttributeType::ListRelationship, WrittenValue::ObjectList(objects)) => Ok(TemporalValue::Structured(StructuredValue::ObjectList(
            objects
                .into_iter()
                .map(|object| match object {
                    ListObject::Bare(uri) | ListObject::Literal(uri) | ListObject::Wrapped(uri) => target(uri),
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

/// A Relationship's targets: one keeps the URI column, several are kept as the
/// ordered array clause 4.5.3.2 admits ("a URI or array of URIs").
fn relationship_value(targets: NonEmpty<Uri>) -> TemporalValue {
    let targets = targets.map(target);
    match targets.as_slice() {
        [single] => TemporalValue::Uri(single.clone()), // clone: the one target the URI column keeps
        _ => TemporalValue::Structured(StructuredValue::Objects(targets)),
    }
}

/// A `VocabProperty`'s terms: one keeps the string column, several are kept
/// structured.
fn vocab_value(terms: NonEmpty<NgsiLdReference>) -> TemporalValue {
    match terms.as_slice() {
        [single] => TemporalValue::String(single.as_str().to_owned()),
        _ => TemporalValue::Structured(StructuredValue::Vocabs(terms)),
    }
}

/// One Relationship target with the percent-encoding a client may have written
/// its entity identifier in decoded.
///
/// A client is free to percent-encode characters in an entity identifier, and
/// the same identifier written two ways names one entity, so the decoding
/// happens once on the way into history rather than at each presentation
/// boundary.
fn target(uri: Uri) -> Uri {
    let decoded = match decode_percent_encoded_uri(uri.as_str()) {
        Cow::Owned(decoded) => Some(decoded),
        Cow::Borrowed(_) => None,
    };
    decoded.and_then(|decoded| decoded.parse::<Uri>().ok()).unwrap_or(uri)
}

/// A Property's value: a single scalar lands in its typed column, anything
/// else is kept structured.
///
/// Clause 4.9 makes an array-valued Property's target value the whole array
/// ("is included in the target value, if the latter is an array"), and JSON-LD
/// expands `"value": ["a","b"]` into one value per element, so several values
/// are kept together, and the empty array, which expands to none, is kept as
/// the empty array.
fn property_value(values: Vec<ExpandedValue<Fragment>>) -> Result<TemporalValue, TemporalError> {
    let scalar = match values.as_slice() {
        [ExpandedValue::Literal(literal)] => Some(scalar_value(literal)),
        [ExpandedValue::Json(literal)] => json_scalar_value(literal.piece()),
        [ExpandedValue::Node(_) | ExpandedValue::List(_)] | [] | [_, _, ..] => None,
    };
    match scalar {
        Some(column) => column,
        None => Ok(TemporalValue::Structured(StructuredValue::Values(values))),
    }
}

/// The column a scalar value object lands in.
fn scalar_value(literal: &Literal) -> Result<TemporalValue, TemporalError> {
    match literal.value() {
        LiteralValue::Number(number) => Ok(TemporalValue::Number(number.clone())), // clone: the observation owns its exact number
        LiteralValue::Boolean(flag) => Ok(TemporalValue::Boolean(*flag)),
        LiteralValue::String(text) => text_value(literal.datatype().map(NgsiLdReference::as_str), text),
    }
}

/// The column a JSON literal holding a scalar lands in, `None` for one holding
/// an array or an object, which is kept structured.
fn json_scalar_value(json: JsonPiece<'_>) -> Option<Result<TemporalValue, TemporalError>> {
    match json.kind() {
        JsonKind::Number => Some(
            JsonNumber::from_str(json.text())
                .map(TemporalValue::Number)
                .map_err(|source| TemporalError::from(SubmissionRefusal::PropertyNumber(source))),
        ),
        JsonKind::String => json.as_str().map(|text| text_value(Some(JSON_DATATYPE), &text)),
        JsonKind::Boolean => json.as_bool().map(|flag| Ok(TemporalValue::Boolean(flag))),
        JsonKind::Null => Some(Ok(TemporalValue::String("null".to_owned()))),
        JsonKind::Array | JsonKind::Object => None,
    }
}

/// A string Property value, which is one of the three clause 4.6.3 time values
/// when its declared datatype or its lexical form says so.
///
/// The classification is [`TimeValue::admitted`]'s and nothing here repeats
/// it: a `@type` the client declared names the form, and with none declared the
/// clause 4.6.3 lexical form of the literal does — "`DateTime` string for
/// encoding a timestamp" is a data type whose whole representation is the
/// string, and clause 4.8 NOTE 3 puts such a value on an ordinary Property.
/// Sharing that one classifier with the `q` operand parser is what makes clause
/// 4.9's rule that "if there is no equality between the target value data type
/// and the Query Term value data type, then it shall be considered as not
/// matching" a rule about the values rather than about which side of the broker
/// read them.
///
/// The two forms stored as text are stored in the canonical rendering their
/// type defines: clause 4.6.3 admits many spellings of one `Time`, clause 4.9
/// orders `Time` and `Date` values chronologically, and the string column
/// orders byte-wise, so only a fixed-width UTC rendering makes the two orders
/// the same. A `DateTime` reaches its own `timestamptz` column, where the
/// instant, not its spelling, is what is compared.
fn text_value(declared: Option<&str>, text: &str) -> Result<TemporalValue, TemporalError> {
    match TimeValue::admitted(declared, text) {
        Some(Ok(TimeValue::DateTime(instant))) => Ok(TemporalValue::DateTime(instant.get())),
        Some(Ok(TimeValue::Date(date))) => Ok(TemporalValue::String(date.to_string())),
        Some(Ok(TimeValue::Time(time))) => Ok(TemporalValue::String(time.to_string())),
        Some(Err(source)) => Err(SubmissionRefusal::TimeValueProperty(source).into()),
        None => Ok(TemporalValue::String(text.to_owned())),
    }
}

#[cfg(test)]
mod tests;
