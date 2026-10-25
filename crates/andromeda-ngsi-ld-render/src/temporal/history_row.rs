//! Projecting one backend-neutral temporal observation into the attribute
//! instance a temporal representation carries.

use andromeda_ngsi_ld_entity::{
    document::{
        instance_document::{InstanceDocument, InstanceDocumentParts},
        slot::Slot,
        value_carrier::ValueCarrier,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    geo_value::GeoValue,
    literal::{Literal, LiteralError, LiteralValue},
    non_empty::{EmptyCollection, NonEmpty},
};
use andromeda_ngsi_ld_lexicon::{
    attribute_type::AttributeType,
    identifier::{Identifier, IdentifierParseError},
    reference::{NgsiLdReference, NgsiLdReferenceError},
    time::{
        date_time::{NgsiLdDateTime, NgsiLdDateTimeError},
        rendering::instant_text,
        system_instant::SystemInstant,
        value::TimeValue,
    },
};
use andromeda_ngsi_ld_model::temporal::{
    TemporalHistoryRow,
    TemporalValue,
    evolution_instance::EvolutionInstance,
    observation_metadata::ObservationMetadata,
    text_annotation::TextAnnotation,
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use chrono::{DateTime, Utc};
use thiserror::Error;

/// The observed time span of a set of history rows.
pub type TimeRange = (DateTime<Utc>, DateTime<Utc>);

/// A history row that cannot be rendered as an attribute instance.
///
/// None of these is the client's doing: each is a stored row that no longer
/// reads as the instance it recorded.
#[derive(Debug, Error)]
pub enum HistoryRowRenderError {
    /// A stored `VocabProperty` term is not a name.
    #[error("a stored vocabulary term is not a name: {0}")]
    VocabTerm(#[source] NgsiLdReferenceError),
    /// A stored language-tagged string does not read as a value object.
    #[error("a stored language-tagged string is not a value object: {0}")]
    LanguageLiteral(#[source] LiteralError),
    /// A rendered `instanceId` is not an identifier.
    #[error("a rendered instance identifier is not an identifier: {0}")]
    InstanceId(#[source] IdentifierParseError),
    /// A reified scope instance names no Scope.
    #[error("a reified scope instance names no scope")]
    EmptyScope(#[source] EmptyCollection),
    /// A stored observation or deletion instant lies below the storable floor.
    #[error("a stored instant is not an NGSI-LD DateTime: {0}")]
    Instant(#[source] NgsiLdDateTimeError),
}

/// The attribute instance one backend-neutral [`TemporalHistoryRow`] renders
/// as.
///
/// The row already carries decoded, typed values; what is left is to put each
/// back in the member its family keeps it in. The clause 4.8 times are written
/// only under `sysAttrs` (Table 5.2.14-1), and the operational write clock
/// never: clause 5.2.21 admits only `observedAt`, `createdAt`, `modifiedAt` and
/// `deletedAt` as Temporal Properties.
///
/// # Errors
///
/// Returns [`HistoryRowRenderError`] for a stored row that no longer reads as
/// the instance it recorded.
pub fn neutral_row_to_instance(row: TemporalHistoryRow, sys_attrs: SysAttrsInclusion) -> Result<InstanceDocument, HistoryRowRenderError> {
    let TemporalHistoryRow {
        attribute_name: _,
        attribute_type,
        dataset_id,
        value,
        observed_at,
        recorded_at: _,
        created_at,
        modified_at,
        instance,
        metadata,
        stream_extent: _,
    } = row;
    let (instance_id, dataset_id) = identity_members(&instance, dataset_id)?;
    let deleted_at = value.deleted_at();
    let metadata = metadata.map_or_else(empty_parts, ObservationMetadata::into_parts);
    let value = rendered_value(value, attribute_type)?;
    // A deletion carries no observation of its own: its value member, its
    // deletion instant, the `observedAt` a written deletion declared and its
    // identity are the whole rendering.
    let observed_at = observed_at
        .map(NgsiLdDateTime::normalise)
        .transpose()
        .map_err(HistoryRowRenderError::Instant)?
        .map(Slot::Set);
    if let Some(deleted_at) = deleted_at {
        let deleted_at = NgsiLdDateTime::normalise(deleted_at).map_err(HistoryRowRenderError::Instant)?;
        return Ok(InstanceDocument::builder()
            .family(Some(attribute_type))
            .value(value)
            .observed_at(observed_at)
            .deleted_at(Some(deleted_at))
            .instance_id(instance_id)
            .dataset_id(dataset_id)
            .build());
    }
    let (created_at, modified_at) = match sys_attrs {
        SysAttrsInclusion::Include => (created_at.map(SystemInstant::new), modified_at.map(SystemInstant::new)),
        SysAttrsInclusion::Strip => (None, None),
    };
    Ok(InstanceDocument::builder()
        .family(Some(attribute_type))
        .dataset_id(dataset_id)
        .value(value)
        .observed_at(observed_at)
        .expires_at(metadata.expires_at)
        .unit_code(metadata.unit_code)
        .value_type(metadata.value_type)
        .object_types(metadata.object_types)
        .created_at(created_at)
        .modified_at(modified_at)
        .deleted_at(metadata.deleted_at)
        .instance_id(instance_id)
        .proof_seals(metadata.proof_seals)
        .sub_attributes(metadata.sub_attributes)
        .build())
}

/// The members of an instance carrying no metadata.
fn empty_parts() -> InstanceDocumentParts {
    InstanceDocument::builder().build().into_parts()
}

/// The value member an observation renders as, in the member its family keeps
/// it in.
///
/// Clauses 4.5.7 and 4.5.8: a deleted instance renders the `urn:ngsi-ld:null`
/// value for its attribute family beside its `deletedAt`. The deletion is
/// synthesised here from the tombstone itself; storage never holds it, so no
/// client value spelling it can be mistaken for a deletion. Each family's
/// deletion encoding is clause 4.5.0's answer, the one a patch writes: a deleted
/// `ListProperty` or `ListRelationship` renders the single-null array of clauses
/// 4.5.21.2 and 4.5.22.2 in its own list member, never the `value` those clauses
/// prohibit.
///
/// A scalar renders as the value object it was written as: typed when it
/// declared a datatype, a declared time value among them, and tagged when it
/// declared a language, as current state holds it.
fn rendered_value(value: TemporalValue, attribute_type: AttributeType) -> Result<ValueSlot, HistoryRowRenderError> {
    Ok(ValueSlot::Set(match value {
        TemporalValue::Number { number, datatype } => literal(LiteralValue::Number(number), datatype),
        TemporalValue::String { text, annotation } => match (attribute_type, annotation) {
            (AttributeType::VocabProperty, TextAnnotation::Plain) => {
                WrittenValue::Vocab(NonEmpty::one(text.parse::<NgsiLdReference>().map_err(HistoryRowRenderError::VocabTerm)?))
            }
            (
                AttributeType::Property
                | AttributeType::GeoProperty
                | AttributeType::Relationship
                | AttributeType::LanguageProperty
                | AttributeType::VocabProperty
                | AttributeType::JsonProperty
                | AttributeType::ListProperty
                | AttributeType::ListRelationship,
                annotation,
            ) => text_literal(text, annotation)?,
        },
        TemporalValue::Boolean { flag, datatype } => literal(LiteralValue::Boolean(flag), datatype),
        TemporalValue::Geometry(geometry) => WrittenValue::Geometry(GeoValue::Geometry(geometry)),
        TemporalValue::Structured(structured) => structured.into_written(),
        TemporalValue::DateTime { instant, datatype } => literal(LiteralValue::String(rendered_instant(instant).into()), datatype),
        TemporalValue::Date { date, datatype } => literal(LiteralValue::String(TimeValue::Date(date).to_string().into()), datatype),
        TemporalValue::Time { time, datatype } => literal(LiteralValue::String(TimeValue::Time(time).to_string().into()), datatype),
        TemporalValue::Uri(uri) => WrittenValue::Object(NonEmpty::one(uri)),
        // Clause 4.18: "The value of scope is represented as a JSON array in
        // case there is more than one Scope". Several Scopes are several value
        // nodes, and compaction gives back the bare string for one and the
        // array for several, which is what annex C clause C.5.16 renders for
        // Vehicles B9211 and A8311 respectively.
        TemporalValue::Scopes(paths) => WrittenValue::Value(
            NonEmpty::try_from_vec(
                paths
                    .iter()
                    .map(|path| ExpandedValue::Literal(Literal::plain(LiteralValue::String(path.as_str().into()))))
                    .collect(),
            )
            .map_err(HistoryRowRenderError::EmptyScope)?
            .into_vec(),
        ),
        TemporalValue::Tombstone { .. } => return Ok(ValueSlot::Remove(ValueCarrier::of(attribute_type))),
    }))
}

/// A value member holding one value object of `value`, typed `datatype` when
/// it declares one.
fn literal(value: LiteralValue, datatype: Option<NgsiLdReference>) -> WrittenValue {
    WrittenValue::Value(vec![ExpandedValue::Literal(match datatype {
        Some(datatype) => Literal::typed(value, datatype),
        None => Literal::plain(value),
    })])
}

/// A value member holding the string `text` with the datatype or language it
/// was written with.
fn text_literal(text: String, annotation: TextAnnotation) -> Result<WrittenValue, HistoryRowRenderError> {
    let value = LiteralValue::String(text.into());
    Ok(match annotation {
        TextAnnotation::Plain => literal(value, None),
        TextAnnotation::Datatype(datatype) => literal(value, Some(datatype)),
        TextAnnotation::Language(language) => WrittenValue::Value(vec![ExpandedValue::Literal(
            Literal::try_new(value, None, Some(language), None).map_err(HistoryRowRenderError::LanguageLiteral)?,
        )]),
    })
}

/// An instant rendered in exactly the sub-second precision it carries.
///
/// Each instant here is rebuilt from what the store decoded, never from a
/// client string, so it renders through [`instant_text`]: exactly the sub-second
/// precision the instant carries, up to the microsecond resolution the
/// `TIMESTAMPTZ` column holds and clause 4.6.3 caps a `DateTime` at. A returned
/// instant is then exact enough to reuse as a bound in a later temporal query,
/// while a whole-second instant still renders as a bare `Z`, which is what a
/// client that wrote a whole second reads back.
fn rendered_instant(instant: DateTime<Utc>) -> String {
    instant_text(instant)
}

/// The members that identify the instance: the `instanceId` clause 4.5.7 hands
/// back, and the `datasetId` it belongs to when it names one.
///
/// An instance of the Scope carries neither: clause 4.5.6 gives it no
/// `instanceId`, and clause 4.5.4 gives a Scope no dataset dimension.
fn identity_members(instance: &EvolutionInstance, dataset_id: Option<Identifier>) -> Result<(Option<Identifier>, Option<Identifier>), HistoryRowRenderError> {
    match instance {
        EvolutionInstance::Scope => Ok((None, None)),
        EvolutionInstance::Attribute(identity) => Ok((
            Some(
                identity
                    .rendered()
                    .to_string()
                    .parse::<Identifier>()
                    .map_err(HistoryRowRenderError::InstanceId)?,
            ),
            dataset_id,
        )),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
