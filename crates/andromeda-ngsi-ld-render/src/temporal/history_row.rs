//! Projecting one backend-neutral temporal observation into the attribute
//! instance a temporal representation carries.

use andromeda_ngsi_ld_entity::{
    date_time_text::DateTimeText,
    document::{
        instance_document::{InstanceDocument, InstanceDocumentParts},
        slot::Slot,
        value_carrier::ValueCarrier,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    geo_value::GeoValue,
    literal::{Literal, LiteralValue},
    non_empty::{EmptyCollection, NonEmpty},
    system_instant::SystemInstant,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_type::AttributeType,
    identifier::{Identifier, IdentifierParseError},
    instance_family::InstanceFamily,
    instance_identity::InstanceIdentity,
    reference::{NgsiLdReference, NgsiLdReferenceError},
    rfc3339::sub_second,
};
use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue, observation_metadata::ObservationMetadata};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_vocabulary::expanded::classes as expanded_classes;
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
    /// The `DateTime` datatype IRI did not read as a name.
    #[error("the DateTime datatype is not a name: {0}")]
    DateTimeDatatype(#[source] NgsiLdReferenceError),
    /// A rendered `instanceId` is not an identifier.
    #[error("a rendered instance identifier is not an identifier: {0}")]
    InstanceId(#[source] IdentifierParseError),
    /// A reified scope instance names no Scope.
    #[error("a reified scope instance names no scope")]
    EmptyScope(#[source] EmptyCollection),
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
        attribute_name,
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
    let (instance_id, dataset_id) = identity_members(&instance, InstanceFamily::of_attribute_name(&attribute_name), dataset_id)?;
    let deleted_at = value.deleted_at();
    let value = rendered_value(value, attribute_type)?;
    // A tombstone carries no observation of its own: its value member, its
    // deletion instant and its identity are the whole rendering.
    if let Some(deleted_at) = deleted_at {
        return Ok(InstanceDocument::builder()
            .family(Some(attribute_type))
            .value(value)
            .deleted_at(Some(DateTimeText::from_instant(deleted_at)))
            .instance_id(instance_id)
            .dataset_id(dataset_id)
            .build());
    }
    let (created_at, modified_at) = match sys_attrs {
        SysAttrsInclusion::Include => (created_at.map(SystemInstant::new), modified_at.map(SystemInstant::new)),
        SysAttrsInclusion::Strip => (None, None),
    };
    let metadata = metadata.map_or_else(empty_parts, ObservationMetadata::into_parts);
    Ok(InstanceDocument::builder()
        .family(Some(attribute_type))
        .dataset_id(dataset_id)
        .value(value)
        .observed_at(observed_at.map(|instant| Slot::Set(DateTimeText::from_instant(instant))))
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
fn rendered_value(value: TemporalValue, attribute_type: AttributeType) -> Result<ValueSlot, HistoryRowRenderError> {
    Ok(ValueSlot::Set(match value {
        TemporalValue::Number(number) => literal(LiteralValue::Number(number)),
        TemporalValue::String(text) => match attribute_type {
            AttributeType::VocabProperty => WrittenValue::Vocab(NonEmpty::one(text.parse::<NgsiLdReference>().map_err(HistoryRowRenderError::VocabTerm)?)),
            AttributeType::Property
            | AttributeType::GeoProperty
            | AttributeType::Relationship
            | AttributeType::LanguageProperty
            | AttributeType::JsonProperty
            | AttributeType::ListProperty
            | AttributeType::ListRelationship => literal(LiteralValue::String(text.into())),
        },
        TemporalValue::Boolean(flag) => literal(LiteralValue::Boolean(flag)),
        TemporalValue::Geometry(geometry) => WrittenValue::Geometry(GeoValue::Geometry(geometry)),
        TemporalValue::Structured(structured) => structured.into_written(),
        TemporalValue::DateTime(instant) => WrittenValue::Value(vec![ExpandedValue::Literal(Literal::typed(
            LiteralValue::String(rendered_instant(instant).into()),
            expanded_classes::DATE_TIME
                .as_str()
                .parse::<NgsiLdReference>()
                .map_err(HistoryRowRenderError::DateTimeDatatype)?,
        ))]),
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

/// A value member holding one plain value object.
fn literal(value: LiteralValue) -> WrittenValue {
    WrittenValue::Value(vec![ExpandedValue::Literal(Literal::plain(value))])
}

/// An instant rendered in exactly the sub-second precision it carries.
///
/// Each instant here is rebuilt from what the store decoded, never from a
/// client string, so it renders through [`sub_second`]: exactly the sub-second
/// precision the instant carries, up to the microsecond resolution the
/// `TIMESTAMPTZ` column holds and clause 4.6.3 caps a `DateTime` at. A returned
/// instant is then exact enough to reuse as a bound in a later temporal query,
/// while a whole-second instant still renders as a bare `Z`, which is what a
/// client that wrote a whole second reads back.
fn rendered_instant(instant: DateTime<Utc>) -> String {
    sub_second(instant)
}

/// The members that identify the instance: the `instanceId` clause 4.5.7 hands
/// back, and the `datasetId` it belongs to when it names one.
///
/// A Scope carries neither. Clause 4.5.6 restricts the temporal representation
/// of a Scope to "the non-reified Temporal Properties createdAt, modifiedAt,
/// deletedAt and observedAt as sub-Properties" -- `instanceId` is none of those
/// and so has no place on it -- and clause 4.5.4 gives a Scope no dataset
/// dimension, so no `datasetId` either.
fn identity_members(
    instance: &InstanceIdentity,
    family: InstanceFamily,
    dataset_id: Option<Identifier>,
) -> Result<(Option<Identifier>, Option<Identifier>), HistoryRowRenderError> {
    match family {
        InstanceFamily::Scope => Ok((None, None)),
        InstanceFamily::Attribute => Ok((
            Some(
                instance
                    .rendered(family)
                    .to_string()
                    .parse::<Identifier>()
                    .map_err(HistoryRowRenderError::InstanceId)?,
            ),
            dataset_id,
        )),
    }
}

#[cfg(test)]
mod tests;
