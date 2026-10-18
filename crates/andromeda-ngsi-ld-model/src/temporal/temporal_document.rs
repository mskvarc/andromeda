//! The Temporal Evolution of an Entity in the expanded, context-independent
//! form (ETSI GS CIM 009 clauses 4.5.6, 4.5.7 and 5.7.4.4).
//!
//! Every holder of an entity's history spells its names under the context of
//! its own request, so histories are gathered, trimmed and bounded here, where
//! a name is the IRI it expands to and an attribute is the same attribute
//! whoever held it, and compacted once, under the client's context, when the
//! answer is written.

use crate::entity_system_times::EntitySystemTimes;
use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
    time_prop::TimeProp,
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use std::{cmp::Reverse, num::NonZeroU32};

/// How the history another holder contributed joins the one already held
/// (clauses 4.3.6.2 and 5.7.3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeMode {
    /// An inclusive, exclusive or redirecting holder: every instance it holds
    /// joins, one carrying an `instanceId` already held counting once.
    Inclusive,
    /// An auxiliary holder: clause 5.7.3.4 adds its data "only when the
    /// Attribute instance, whose value of the timeproperty [...] is not
    /// present in any of the Attribute instances received from elsewhere".
    Auxiliary,
}

/// The expanded Temporal Evolution of one entity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TemporalDocument {
    /// The entity's identifier, types and instances, the instances of an
    /// attribute in order.
    entity: EntityDocument,
    /// The entity's own clause 4.8 system times.
    system_times: EntitySystemTimes,
    /// The family of every attribute left without an instance, keyed by its
    /// expanded name: an empty sequence no longer says whether it held
    /// Properties or Relationships, which clause 4.5.9 still needs to name the
    /// attribute's `type` and plural member.
    emptied: IndexMap<AttributeName, AttributeType>,
}

impl TemporalDocument {
    /// The evolution `entity` holds, with the system times and emptied
    /// attributes it carries beside it.
    #[must_use]
    pub const fn new(entity: EntityDocument, system_times: EntitySystemTimes, emptied: IndexMap<AttributeName, AttributeType>) -> TemporalDocument {
        TemporalDocument { entity, system_times, emptied }
    }

    /// The entity's identifier, when the document names it.
    #[must_use]
    pub const fn id(&self) -> Option<&Identifier> {
        self.entity.id()
    }

    /// The entity's types.
    #[must_use]
    pub fn types(&self) -> &[EntityTypeName] {
        self.entity.types()
    }

    /// The entity document: its identifier, types and instances.
    #[must_use]
    pub const fn entity(&self) -> &EntityDocument {
        &self.entity
    }

    /// The entity's own clause 4.8 system times.
    #[must_use]
    pub const fn system_times(&self) -> EntitySystemTimes {
        self.system_times
    }

    /// The family of every attribute left without an instance.
    #[must_use]
    pub const fn emptied(&self) -> &IndexMap<AttributeName, AttributeType> {
        &self.emptied
    }

    /// Give the document up as its three parts.
    #[must_use]
    pub fn into_parts(self) -> (EntityDocument, EntitySystemTimes, IndexMap<AttributeName, AttributeType>) {
        (self.entity, self.system_times, self.emptied)
    }

    /// Every instance of every attribute, with the attribute it belongs to.
    pub fn instances(&self) -> impl Iterator<Item = (&AttributeName, &InstanceDocument)> {
        self.entity
            .attributes()
            .iter()
            .flat_map(|(name, member)| member.instances().iter().map(move |instance| (name, instance)))
    }

    /// How many instances the evolution holds.
    #[must_use]
    pub fn instance_count(&self) -> usize {
        self.entity.attributes().values().map(|member| member.instances().len()).sum()
    }

    /// The least and most recent instants any instance carries on
    /// `timeproperty`, or `None` when none carries one.
    #[must_use]
    pub fn instant_span(&self, timeproperty: TimeProp) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        self.instances()
            .filter_map(|(_, instance)| instance.instant_on(timeproperty))
            .fold(None, |span, instant| {
                Some(span.map_or((instant, instant), |(least, most)| (least.min(instant), most.max(instant))))
            })
    }

    /// Keep only the instances whose instant on `timeproperty` `keep` admits;
    /// an instance with no instant there is kept, since nothing places it
    /// outside. An attribute left without an instance stays, with the family
    /// its instances recorded.
    pub fn retain_instants(&mut self, timeproperty: TimeProp, keep: impl Fn(DateTime<Utc>) -> bool) {
        self.reshape_instances(|_, instances| instances.retain(|instance| instance.instant_on(timeproperty).is_none_or(&keep)));
    }

    /// Keep only the instances `keep` admits, given the attribute each belongs
    /// to. An attribute left without an instance stays, with the family its
    /// instances recorded.
    pub fn retain_instances(&mut self, keep: impl Fn(&AttributeName, &InstanceDocument) -> bool) {
        self.reshape_instances(|name, instances| instances.retain(|instance| keep(name, instance)));
    }

    /// Keep, for every attribute, the `last_n` instances with the most recent
    /// instant on `timeproperty`, in ascending order of it: clause 5.7.3.4
    /// fixes which instants are kept, never the order of the response, and
    /// every response lists its instances oldest first. An instance with no
    /// instant there is left out: clause 5.7.3.4 has `lastN` "correspond to
    /// the last n timestamps [...] of the temporal property", and it has none.
    pub fn keep_last(&mut self, timeproperty: TimeProp, last_n: NonZeroU32) {
        let keep = usize::try_from(last_n.get()).unwrap_or(usize::MAX);
        self.reshape_instances(|_, instances| {
            instances.retain(|instance| instance.instant_on(timeproperty).is_some());
            instances.sort_by_key(|instance| Reverse(instance.instant_on(timeproperty)));
            instances.truncate(keep);
            instances.sort_by_key(|instance| instance.instant_on(timeproperty));
        });
    }

    /// Put `other`'s history of the same entity beside this one, by the
    /// attribute each instance belongs to, as `mode` says; an attribute only
    /// `other` holds joins whole, and the instances of an attribute that
    /// gained one are listed oldest first on `timeproperty`.
    ///
    /// The entity's identifier, types and system times stay the first
    /// holder's.
    pub fn merge(&mut self, other: TemporalDocument, mode: MergeMode, timeproperty: TimeProp) {
        let (entity, system_times, emptied) = other.into_parts();
        self.system_times = self.system_times.or(system_times);
        for (name, family) in emptied {
            if !self.entity.attributes().contains_key(&name) {
                self.emptied.entry(name).or_insert(family);
            }
        }
        for (name, member) in entity.into_parts().attributes {
            let AttributeMember::Instances(received) = member else {
                continue;
            };
            self.emptied.shift_remove(&name);
            match self.entity.attributes_mut().get_mut(&name) {
                Some(AttributeMember::Instances(held)) => {
                    let before = held.len();
                    for instance in received {
                        if joins(held, &instance, mode, timeproperty) {
                            held.push(instance);
                        }
                    }
                    if held.len() != before {
                        held.sort_by_key(|instance| instance.instant_on(timeproperty));
                    }
                }
                Some(AttributeMember::Remove) | None => {
                    self.entity.attributes_mut().insert(name, AttributeMember::Instances(received));
                }
            }
        }
    }

    /// Reshape the instances of every attribute with `reshape`, given the
    /// attribute's name, recording the family of one that ends up with none.
    fn reshape_instances(&mut self, mut reshape: impl FnMut(&AttributeName, &mut Vec<InstanceDocument>)) {
        for (name, member) in self.entity.attributes_mut() {
            let AttributeMember::Instances(instances) = member else {
                continue;
            };
            let family = instances.iter().find_map(InstanceDocument::family);
            reshape(name, instances);
            if let (true, Some(family)) = (instances.is_empty(), family) {
                self.emptied.entry(name.clone()).or_insert(family); // clone: the record names the attribute the document keeps
            }
        }
    }
}

/// Whether `candidate` joins the `held` instances of its attribute under `mode`.
fn joins(held: &[InstanceDocument], candidate: &InstanceDocument, mode: MergeMode, timeproperty: TimeProp) -> bool {
    let known = |id: &Identifier| held.iter().any(|instance| instance.instance_id() == Some(id));
    match (mode, candidate.instance_id()) {
        (MergeMode::Inclusive, Some(id)) => !known(id),
        (MergeMode::Inclusive, None) => true,
        (MergeMode::Auxiliary, _) => {
            let Some(instant) = candidate.instant_on(timeproperty) else {
                return false;
            };
            !held
                .iter()
                .any(|instance| instance.dataset_id() == candidate.dataset_id() && instance.instant_on(timeproperty) == Some(instant))
        }
    }
}

#[cfg(test)]
mod tests;
