//! One attribute member as a notification reports it (clause 5.8.6): the
//! instances the entity holds, then the ones the change deleted, each with
//! what the notification reports of it beside its own members.
//!
//! The instances are read from the typed renderings [`NotifiedChanges`]
//! holds and written by the same walk as any other member; a report only says
//! what replaces or joins the members the instance carries. A member the
//! broker carries verbatim has the report written into a copy of itself, as
//! it has no typed members to be written beside.

use crate::notified_changes::{HeldAttributes, NotifiedChanges};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, identifier::Identifier, rfc3339::sub_second};
use andromeda_ngsi_ld_model::attribute::instance::{Attribute, AttributeValue};
use andromeda_ngsi_ld_query::projection::term::ProjectionTerm;
use andromeda_ngsi_ld_vocabulary::{NULL_URI, compact::properties as compact_properties};
use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::{Map, Value};
use std::{borrow::Cow, iter::once, slice::from_ref};

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
}

/// One instance and what the notification reports of it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NotifiedInstance<'a> {
    /// The instance.
    pub(crate) source: InstanceSource<'a>,
    /// What is reported beside its members.
    pub(crate) report: InstanceReport<'a>,
}

/// One attribute instance, typed or carried verbatim.
#[derive(Debug, Clone, Copy)]
pub(crate) enum InstanceSource<'a> {
    /// A typed instance.
    Typed(&'a Attribute),
    /// An instance the broker carries without interpreting it.
    Verbatim(&'a Value),
}

/// What a notification reports of one instance beside its own members.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct InstanceReport<'a> {
    /// The NGSI-LD Null its value member takes, when the instance is reported
    /// deleted.
    pub(crate) null: Option<NullValue>,
    /// The `deletedAt` it is stamped with.
    pub(crate) deleted_at: Option<DateTime<Utc>>,
    /// Under `showChanges`, the instance it was before the change.
    pub(crate) previous: Option<InstanceSource<'a>>,
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

impl NullValue {
    /// The Null `family` is deleted by. An entity deletion reports a single
    /// `LanguageProperty` with the bare Null rather than a language map holding
    /// it (clause 4.5.18).
    const fn of(family: AttributeType, language: LanguageNull) -> NullValue {
        match (family, language) {
            (AttributeType::ListProperty | AttributeType::ListRelationship, LanguageNull::Bare | LanguageNull::Mapped) => NullValue::List,
            (AttributeType::LanguageProperty, LanguageNull::Mapped) => NullValue::LanguageMap,
            (
                AttributeType::LanguageProperty
                | AttributeType::Property
                | AttributeType::GeoProperty
                | AttributeType::Relationship
                | AttributeType::VocabProperty
                | AttributeType::JsonProperty,
                LanguageNull::Bare | LanguageNull::Mapped,
            ) => NullValue::Uri,
        }
    }

    /// The Null as a JSON value, for a member carried verbatim.
    fn to_json(self) -> Value {
        let null = || Value::String(NULL_URI.as_str().to_owned());
        match self {
            NullValue::Uri => null(),
            NullValue::List => Value::Array(vec![null()]),
            NullValue::LanguageMap => Value::Object(once((NONE_KEY.to_owned(), null())).collect()),
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

/// How a deleted `LanguageProperty` spells its Null.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LanguageNull {
    /// The bare Null, as an entity deletion reports a single one.
    Bare,
    /// A language map holding the Null.
    Mapped,
}

impl<'a> NotifiedMember<'a> {
    /// The member named `name` as `changes` report it: the instances `held`
    /// holds, then those the change deleted. `None` when neither holds one.
    pub(crate) fn of(name: &str, held: Option<&'a AttributeValue>, changes: &NotifiedChanges<'a>) -> Option<NotifiedMember<'a>> {
        let previous = changes.previous.and_then(|previous| previous.attribute_member(name)).map(sources);
        let previous = previous.as_deref().unwrap_or_default();
        let held_sources = held.map(sources).unwrap_or_default();
        let language = if matches!(held, Some(AttributeValue::Single(_))) {
            LanguageNull::Bare
        } else {
            LanguageNull::Mapped
        };
        let held_instances = held_sources.iter().map(|&source| NotifiedInstance {
            source,
            report: InstanceReport {
                null: match changes.held {
                    HeldAttributes::Deleted => null_of(source, language),
                    HeldAttributes::Current => None,
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
                    null: null_of(source, LanguageNull::Mapped),
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
            Some(AttributeValue::Raw(raw)) => raw.is_array(),
            Some(AttributeValue::Single(_)) | None => false,
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

/// The instances one member holds.
fn sources(member: &AttributeValue) -> Vec<InstanceSource<'_>> {
    match member {
        AttributeValue::Single(attribute) => vec![InstanceSource::Typed(attribute)],
        AttributeValue::Multiple(attributes) => attributes.iter().map(InstanceSource::Typed).collect(),
        AttributeValue::Raw(raw) => match raw {
            Value::Array(items) => items.iter().map(InstanceSource::Verbatim).collect(),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => {
                from_ref(raw).iter().map(InstanceSource::Verbatim).collect()
            }
        },
    }
}

/// The Null a reported deletion of `source` writes, `None` for a bare
/// verbatim value, which has no value member to hold it.
fn null_of(source: InstanceSource<'_>, language: LanguageNull) -> Option<NullValue> {
    match source {
        InstanceSource::Typed(attribute) => Some(NullValue::of(attribute.attribute_type(), language)),
        InstanceSource::Verbatim(Value::Object(members)) => Some(NullValue::of(verbatim_family(members), language)),
        InstanceSource::Verbatim(Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_)) => None,
    }
}

/// The previous instance of the same `datasetId` as `source`, `None` for a
/// bare verbatim value, which has no member to carry it.
fn previous_of<'a>(source: InstanceSource<'_>, previous: &[InstanceSource<'a>]) -> Option<InstanceSource<'a>> {
    if matches!(source, InstanceSource::Verbatim(value) if !value.is_object()) {
        return None;
    }
    let dataset_id = source.dataset_id();
    previous.iter().copied().find(|candidate| candidate.dataset_id() == dataset_id)
}

/// The family a verbatim instance belongs to: the one its `type` declares,
/// else the one whose value member it carries, else a Property, the family of
/// a bare value.
fn verbatim_family(members: &Map<String, Value>) -> AttributeType {
    declared_family(members).unwrap_or_else(|| {
        AttributeType::ALL
            .into_iter()
            .find(|candidate| members.contains_key(candidate.compact_value_key()))
            .unwrap_or(AttributeType::Property)
    })
}

/// The family a verbatim instance's `type` declares.
fn declared_family(members: &Map<String, Value>) -> Option<AttributeType> {
    members
        .get(ProjectionTerm::TYPE_KEYWORD)
        .and_then(Value::as_str)
        .and_then(|token| token.parse::<AttributeType>().ok())
}

impl<'a> InstanceSource<'a> {
    /// The `datasetId` the instance carries, `None` for the default instance.
    pub(crate) fn dataset_id(self) -> Option<&'a str> {
        match self {
            InstanceSource::Typed(attribute) => attribute.dataset_id().map(Identifier::as_str),
            InstanceSource::Verbatim(value) => value.get(compact_properties::DATASET_ID).and_then(Value::as_str),
        }
    }

    /// The value member this instance, as the previous one, gives an instance
    /// of `family`: its own value member when it spells that family's, the
    /// instance itself when it is a bare value (clause 4.5.2.3).
    pub(crate) fn previous_value_of(self, family: AttributeType) -> Option<PreviousValue<'a>> {
        match self {
            InstanceSource::Typed(attribute) => {
                (attribute.attribute_type().compact_value_key() == family.compact_value_key()).then_some(PreviousValue::Typed(attribute))
            }
            InstanceSource::Verbatim(Value::Object(members)) => members.get(family.compact_value_key()).map(PreviousValue::Verbatim),
            InstanceSource::Verbatim(bare @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_))) => {
                Some(PreviousValue::Verbatim(bare))
            }
        }
    }
}

/// The value a previous member is written from.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PreviousValue<'a> {
    /// The value member of a typed instance.
    Typed(&'a Attribute),
    /// A value read out of an instance carried verbatim.
    Verbatim(&'a Value),
}

impl InstanceReport<'_> {
    /// Whether the report adds nothing to the instance.
    pub(crate) const fn is_empty(&self) -> bool {
        self.null.is_none() && self.deleted_at.is_none() && self.previous.is_none()
    }
}

/// A verbatim instance with `report` written into it.
pub(crate) fn reported_verbatim<'v>(instance: &'v Value, report: &InstanceReport<'_>) -> Cow<'v, Value> {
    let Value::Object(members) = instance else {
        return Cow::Borrowed(instance);
    };
    if report.is_empty() {
        return Cow::Borrowed(instance);
    }
    // clone: a member carried verbatim is reported in a copy of its own; such
    // members are small and rare, unlike typed instances.
    let mut reported = members.clone();
    if let Some(null) = report.null {
        reported.insert(verbatim_family(members).compact_value_key().to_owned(), null.to_json());
    }
    if let Some(deleted_at) = report.deleted_at {
        reported.insert(compact_properties::DELETED_AT.to_owned(), Value::String(sub_second(deleted_at)));
    }
    let family = declared_family(members).unwrap_or(AttributeType::Property);
    if let Some(previous) = report.previous.and_then(|previous| previous.previous_value_of(family)) {
        let previous = match previous {
            PreviousValue::Typed(attribute) => attribute.value(),
            // clone: the reported copy owns the value the previous rendering keeps
            PreviousValue::Verbatim(value) => value.clone(),
        };
        reported.insert(family.previous_key().to_owned(), previous);
    }
    Cow::Owned(Value::Object(reported))
}
