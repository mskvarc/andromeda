//! An attribute member written in a reified representation: the normalized
//! one of clause 4.5.1, or the concise one of clause 4.5.2.3 and its siblings.
//!
//! The two share every member of every family and differ in three places
//! only, which is why one walk writes both:
//!
//! - the normalized representation names each instance's family in `type`,
//!   and the concise one leaves it to be inferred;
//! - the concise representation collapses a Property or `GeoProperty` that
//!   stands alone and carries nothing but its value down to the bare value,
//!   when that value reads back as its own (clause 4.5.2.3);
//! - the concise `objectList` holds bare URIs where the normalized one holds
//!   `object`-keyed Relationship Objects (Table 5.2.37-1).
//!
//! A member is one instance, the instances of a multi-attribute, or the bare
//! NGSI-LD Null a notification reports a deleted member by (clause 5.8.6); each
//! instance is written by [`crate::entity_body::reified_instance`]. A version
//! fallback below 1.3 writes a multi-attribute as the one instance Table
//! 4.3.6.8-1 notes 2 and 3 keep, standing alone.

use crate::{
    concise::InstanceStanding,
    entity_body::{
        instance_fallback::kept_instance,
        member_walk::MemberWalk,
        notified_member::{InstanceReport, MemberInstances, NotifiedInstance, NotifiedMember, NullValue},
        reified_instance::ReifiedInstance,
    },
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::attribute::instance::{Attribute, AttributeValue};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use serde::{Serialize, Serializer};

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

    /// One typed instance of the member, with what is reported of it and
    /// where it stands.
    const fn instance(&self, attribute: &'a Attribute, report: InstanceReport<'a>, standing: InstanceStanding) -> ReifiedInstance<'a> {
        ReifiedInstance {
            attribute,
            reification: self.reification,
            walk: self.walk,
            report,
            standing,
        }
    }

    /// The one instance of `instances` a version fallback below 1.3 writes the
    /// member as, or `None` when the member keeps every instance.
    fn kept<'i, T>(&self, instances: &'i [T], dataset_id: impl Fn(&T) -> Option<&Identifier>) -> Option<&'i T> {
        if self.walk.collapses_instances() {
            kept_instance(instances, dataset_id)
        } else {
            None
        }
    }

    /// One instance a notification reports, standing as `standing` says.
    const fn notified(&self, instance: NotifiedInstance<'a>, standing: InstanceStanding) -> ReifiedInstance<'a> {
        self.instance(instance.source, instance.report, standing)
    }
}

impl Serialize for ReifiedMember<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.value {
            MemberInstances::Held(AttributeValue::Single(attribute)) => self
                .instance(attribute, InstanceReport::default(), InstanceStanding::Alone)
                .serialize(serializer),
            MemberInstances::Held(AttributeValue::Multiple(instances)) => match self.kept(instances, Attribute::dataset_id) {
                Some(kept) => self.instance(kept, InstanceReport::default(), InstanceStanding::Alone).serialize(serializer),
                None => serializer.collect_seq(
                    instances
                        .iter()
                        .map(|attribute| self.instance(attribute, InstanceReport::default(), InstanceStanding::AmongInstances)),
                ),
            },
            // Clause 5.8.6: the member reported deleted is written bare, as the
            // short form names no family.
            MemberInstances::Held(AttributeValue::NgsiLdNull) | MemberInstances::Notified(NotifiedMember::NgsiLdNull) => NullValue::Uri.serialize(serializer),
            MemberInstances::Notified(NotifiedMember::One(instance)) => self.notified(*instance, InstanceStanding::Alone).serialize(serializer),
            MemberInstances::Notified(NotifiedMember::Many(instances)) => match self.kept(instances, |instance| instance.source.dataset_id()) {
                Some(kept) => self.notified(*kept, InstanceStanding::Alone).serialize(serializer),
                None => serializer.collect_seq(instances.iter().map(|instance| self.notified(*instance, InstanceStanding::AmongInstances))),
            },
            MemberInstances::Evolution(instances) => serializer.collect_seq(
                instances
                    .iter()
                    .map(|attribute| self.instance(attribute, InstanceReport::default(), InstanceStanding::AmongInstances)),
            ),
        }
    }
}
