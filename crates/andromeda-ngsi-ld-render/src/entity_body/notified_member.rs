//! One attribute member as a notification reports it (clause 5.8.6): the
//! instances the entity holds, then the ones the change deleted, each with
//! what the notification reports of it beside its own members.
//!
//! The instances are read from the typed renderings [`NotifiedChanges`]
//! holds and written by the same walk as any other member; a report only says
//! what replaces or joins the members the instance carries. A member reported
//! deleted by the bare NGSI-LD Null (clause 5.8.6) is written as that Null.

use crate::notified_changes::{HeldAttributes, NotifiedChanges};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, identifier::Identifier};
use andromeda_ngsi_ld_model::attribute::instance::{Attribute, AttributeValue};
use andromeda_ngsi_ld_vocabulary::NULL_URI;
use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer, ser::SerializeMap};
use std::iter::once;

/// The JSON-LD keyword a language map keys an untagged string by.
const NONE_KEY: &str = "@none";

/// One attribute member as the walk writes it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum MemberInstances<'a> {
    /// The member as the entity holds it.
    Held(&'a AttributeValue),
    /// The member as a notification reports it.
    Notified(&'a NotifiedMember<'a>),
}

/// The instances a notification reports for one attribute.
#[derive(Debug)]
pub(crate) enum NotifiedMember<'a> {
    /// The one instance of a single attribute.
    One(NotifiedInstance<'a>),
    /// The instances of a multi-attribute, held ones first.
    Many(Vec<NotifiedInstance<'a>>),
    /// The member the entity reports deleted by the bare NGSI-LD Null.
    NgsiLdNull,
}

/// One instance and what the notification reports of it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NotifiedInstance<'a> {
    /// The instance.
    pub(crate) source: &'a Attribute,
    /// What is reported beside its members.
    pub(crate) report: InstanceReport<'a>,
}

/// What a notification reports of one instance's value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ReportedValue {
    /// The value the instance carries.
    #[default]
    Held,
    /// The NGSI-LD Null of a reported deletion, in place of the value.
    Deleted,
}

/// What a notification reports of one instance beside its own members.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct InstanceReport<'a> {
    /// Whether the value member is written or the instance is reported
    /// deleted.
    pub(crate) value: ReportedValue,
    /// The `deletedAt` it is stamped with.
    pub(crate) deleted_at: Option<DateTime<Utc>>,
    /// Under `showChanges`, the instance it was before the change.
    pub(crate) previous: Option<&'a Attribute>,
}

/// The spelling of the NGSI-LD Null a family is deleted by (clause 4.5.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NullValue {
    /// The bare Null.
    Uri,
    /// An array of the one Null, the list families' encoding (clauses
    /// 4.5.21.2 and 4.5.22.2).
    List,
    /// A language map holding the Null under `@none` (clause 4.5.18).
    LanguageMap,
}

impl InstanceReport<'_> {
    /// The NGSI-LD Null the value member of an instance written as `family`
    /// takes, when the instance is reported deleted. The spelling is the
    /// written family's, so a family a version fallback reformats is deleted
    /// as the family it is written as.
    pub(crate) const fn null(&self, family: AttributeType) -> Option<NullValue> {
        match self.value {
            ReportedValue::Held => None,
            ReportedValue::Deleted => Some(NullValue::of(family)),
        }
    }
}

impl NullValue {
    /// The Null `family` is deleted by. A deleted `LanguageProperty` keeps the
    /// language map holding the Null however it came to be deleted: clause
    /// 4.5.18.2 uses "the same representation" "to indicate a deletion in
    /// notifications", and clause 5.8.6 sets "the languageMap" to it whenever
    /// a deleted attribute is notified as an object.
    const fn of(family: AttributeType) -> NullValue {
        match family {
            AttributeType::ListProperty | AttributeType::ListRelationship => NullValue::List,
            AttributeType::LanguageProperty => NullValue::LanguageMap,
            AttributeType::Property | AttributeType::GeoProperty | AttributeType::Relationship | AttributeType::VocabProperty | AttributeType::JsonProperty => {
                NullValue::Uri
            }
        }
    }
}

impl Serialize for NullValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            NullValue::Uri => serializer.serialize_str(NULL_URI.as_str()),
            NullValue::List => serializer.collect_seq(once(NULL_URI.as_str())),
            NullValue::LanguageMap => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(NONE_KEY, NULL_URI.as_str())?;
                map.end()
            }
        }
    }
}

impl<'a> NotifiedMember<'a> {
    /// The member named `name` as `changes` report it: the instances `held`
    /// holds, then those the change deleted. `None` when neither holds one.
    pub(crate) fn of(name: &str, held: Option<&'a AttributeValue>, changes: &NotifiedChanges<'a>) -> Option<NotifiedMember<'a>> {
        if matches!(held, Some(AttributeValue::NgsiLdNull)) {
            return Some(NotifiedMember::NgsiLdNull);
        }
        let previous = changes.previous.and_then(|previous| previous.attribute_member(name)).map(sources);
        let previous = previous.as_deref().unwrap_or_default();
        let held_sources = held.map(sources).unwrap_or_default();
        let held_instances = held_sources.iter().map(|&source| NotifiedInstance {
            source,
            report: InstanceReport {
                value: match changes.held {
                    HeldAttributes::Deleted => ReportedValue::Deleted,
                    HeldAttributes::Current => ReportedValue::Held,
                },
                deleted_at: None,
                previous: previous_of(source, previous),
            },
        });
        let deleted_sources = changes
            .deleted
            .and_then(|deleted| deleted.instances.attribute_member(name).map(|member| (sources(member), deleted.deleted_at)));
        let deleted_instances = deleted_sources.iter().flat_map(|(sources, deleted_at)| {
            sources.iter().map(|&source| NotifiedInstance {
                source,
                report: InstanceReport {
                    value: ReportedValue::Deleted,
                    deleted_at: *deleted_at,
                    previous: previous_of(source, previous),
                },
            })
        });
        let mut instances: Vec<NotifiedInstance<'a>> = held_instances.chain(deleted_instances).collect();
        // A member is single when it reports one instance, unless the entity
        // holds it as an array of instances.
        let held_as_array = match held {
            Some(AttributeValue::Multiple(_)) => true,
            Some(AttributeValue::Single(_) | AttributeValue::NgsiLdNull) | None => false,
        };
        let single = !held_as_array && instances.len() == 1;
        if instances.is_empty() {
            return None;
        }
        if single {
            instances.pop().map(NotifiedMember::One)
        } else {
            Some(NotifiedMember::Many(instances))
        }
    }
}

/// The instances one member holds; the NGSI-LD Null holds none.
fn sources(member: &AttributeValue) -> Vec<&Attribute> {
    member.instances().collect()
}

/// The previous instance of the same `datasetId` as `source`.
fn previous_of<'a>(source: &Attribute, previous: &[&'a Attribute]) -> Option<&'a Attribute> {
    let dataset_id = source.dataset_id().map(Identifier::as_str);
    previous
        .iter()
        .copied()
        .find(|candidate| candidate.dataset_id().map(Identifier::as_str) == dataset_id)
}

/// The previous instance `previous` as the value member it gives an instance
/// of `family`: only when it spells that family's value member.
pub(crate) fn previous_value_of(previous: &Attribute, family: AttributeType) -> Option<&Attribute> {
    (previous.attribute_type().compact_value_key() == family.compact_value_key()).then_some(previous)
}
