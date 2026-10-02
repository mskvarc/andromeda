//! An attribute member written in a reified representation: the normalized
//! one of clause 4.5.1, or the concise one of clause 4.5.2.3 and its siblings.
//!
//! The two share every member of every family and differ in three places
//! only, which is why one walk writes both:
//!
//! - the normalized representation names each instance's family in `type`,
//!   and the concise one leaves it to be inferred;
//! - the concise representation collapses a Property or `GeoProperty` that
//!   carries nothing but its value down to the bare value (clause 4.5.2.3);
//! - the concise `objectList` holds bare URIs where the normalized one holds
//!   `object`-keyed Relationship Objects (Table 5.2.37-1).
//!
//! A member is one instance, the instances of a multi-attribute, or a member
//! the broker carries verbatim; each instance is written by
//! [`crate::entity_body::reified_instance`].

use crate::{
    concise::concise_attr_value,
    entity_body::{
        member_walk::MemberWalk,
        notified_member::{InstanceReport, InstanceSource, MemberInstances, NotifiedInstance, NotifiedMember, reported_verbatim},
        reified_instance::ReifiedInstance,
    },
    object_list::normalized_verbatim_member,
    uri_decoding::{UriDecoding, decoded_attribute},
};
use andromeda_ngsi_ld_model::attribute::instance::{Attribute, AttributeValue};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use serde::{Serialize, Serializer};
use serde_json::Value;

/// Which reified representation a member is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reification {
    /// Clause 4.5.1: every instance names its family in `type`.
    Normalized,
    /// Clause 4.5.2.3 and its siblings: the family is inferred.
    Concise,
}

impl Reification {
    /// The representation an entity nested into a member of this one is
    /// written in, which is the same (clauses 4.5.3.3 and 4.5.22.3).
    pub(crate) const fn format(self) -> ResponseFormat {
        match self {
            Reification::Normalized => ResponseFormat::Normalized,
            Reification::Concise => ResponseFormat::Concise,
        }
    }
}

/// One attribute member, as the entity's map holds it or a notification
/// reports it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReifiedMember<'a> {
    /// The member written.
    value: MemberInstances<'a>,
    /// The representation it is written in.
    reification: Reification,
    /// What the walk applies on the way.
    walk: MemberWalk<'a>,
}

impl<'a> ReifiedMember<'a> {
    /// The member, written in `reification` under `walk`.
    pub(crate) const fn new(value: MemberInstances<'a>, reification: Reification, walk: MemberWalk<'a>) -> ReifiedMember<'a> {
        ReifiedMember { value, reification, walk }
    }

    /// One typed instance of the member, with what is reported of it.
    const fn instance(&self, attribute: &'a Attribute, report: InstanceReport<'a>) -> ReifiedInstance<'a> {
        ReifiedInstance {
            attribute,
            reification: self.reification,
            walk: self.walk,
            report,
        }
    }

    /// One instance a notification reports.
    const fn notified(&self, instance: NotifiedInstance<'a>) -> ReifiedNotified<'a> {
        match instance.source {
            InstanceSource::Typed(attribute) => ReifiedNotified::Typed(self.instance(attribute, instance.report)),
            InstanceSource::Verbatim(value) => ReifiedNotified::Verbatim(VerbatimInstance {
                value,
                report: instance.report,
                reification: self.reification,
                decoding: self.walk.decoding,
            }),
        }
    }
}

impl Serialize for ReifiedMember<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.value {
            MemberInstances::Held(AttributeValue::Single(attribute)) => self.instance(attribute, InstanceReport::default()).serialize(serializer),
            MemberInstances::Held(AttributeValue::Multiple(instances)) => {
                serializer.collect_seq(instances.iter().map(|attribute| self.instance(attribute, InstanceReport::default())))
            }
            MemberInstances::Held(AttributeValue::Raw(raw)) => VerbatimInstance {
                value: raw,
                report: InstanceReport::default(),
                reification: self.reification,
                decoding: self.walk.decoding,
            }
            .serialize(serializer),
            MemberInstances::Notified(NotifiedMember::One(instance)) => self.notified(*instance).serialize(serializer),
            MemberInstances::Notified(NotifiedMember::Many(instances)) => serializer.collect_seq(instances.iter().map(|instance| self.notified(*instance))),
        }
    }
}

/// One instance a notification reports, typed or carried verbatim.
enum ReifiedNotified<'a> {
    /// A typed instance.
    Typed(ReifiedInstance<'a>),
    /// An instance carried verbatim.
    Verbatim(VerbatimInstance<'a>),
}

impl Serialize for ReifiedNotified<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ReifiedNotified::Typed(instance) => instance.serialize(serializer),
            ReifiedNotified::Verbatim(instance) => instance.serialize(serializer),
        }
    }
}

/// A member, or one instance of it, the broker carries verbatim.
struct VerbatimInstance<'a> {
    /// The value carried.
    value: &'a Value,
    /// What a notification reports of it.
    report: InstanceReport<'a>,
    /// The representation it is written in.
    reification: Reification,
    /// Whether its Relationship targets are decoded.
    decoding: UriDecoding,
}

impl Serialize for VerbatimInstance<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let reported = reported_verbatim(self.value, &self.report);
        let decoded = decoded_attribute(&reported, self.decoding);
        match self.reification {
            Reification::Normalized => normalized_verbatim_member(decoded).serialize(serializer),
            // The document reading of clause 4.5.2.3 takes the member owned;
            // one carried verbatim is small and rare.
            Reification::Concise => concise_attr_value(decoded.into_owned()).serialize(serializer),
        }
    }
}
