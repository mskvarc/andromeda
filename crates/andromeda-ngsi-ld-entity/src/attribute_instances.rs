//! The instances of one attribute (clause 4.5.5).
//!
//! An attribute holds at most one default instance (no `datasetId`) and at most
//! one instance per `datasetId`: clause 4.5.5.1 says "There can only be one
//! default Attribute instance for an Attribute with a given Attribute name in
//! any request or response", and named instances are told apart by their
//! `datasetId`. The instances are keyed by that identity, so a repeated one is
//! unrepresentable, and kept in stored order, wherever the default sits.

use crate::instance::Instance;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use indexmap::{Equivalent, IndexMap, map::Entry};
use thiserror::Error;

/// Two instances of one attribute claim the same identity.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum RepeatedInstance {
    /// Two instances carry no `datasetId`.
    #[error("more than one default instance")]
    Default,
    /// Two instances carry this `datasetId`.
    #[error("more than one instance with datasetId '{0}'")]
    Dataset(Identifier),
}

/// What removing an instance left of the attribute.
#[derive(Clone, Debug, PartialEq)]
pub enum InstanceRemoval {
    /// No instance had the identity; nothing changed.
    Absent,
    /// The instance was removed and others remain.
    Removed(Instance),
    /// The instance was the attribute's last one: the attribute is gone as a
    /// whole, which only the owner of the attribute map can carry out.
    Emptied(Instance),
}

/// A borrowed instance identity, hashing exactly as the owned key does.
///
/// `Option<&Identifier>` and `Option<Identifier>` derive `Hash` the same way
/// (discriminant, then the identifier), so a lookup needs no owned key.
#[derive(Clone, Copy, Debug, Hash)]
struct DatasetKey<'a>(Option<&'a Identifier>);

impl Equivalent<Option<Identifier>> for DatasetKey<'_> {
    fn equivalent(&self, key: &Option<Identifier>) -> bool {
        self.0 == key.as_ref()
    }
}

/// The instances of one attribute, keyed by `datasetId`, in stored order;
/// never empty.
#[derive(Clone, Debug, PartialEq)]
pub struct AttributeInstances(IndexMap<Option<Identifier>, Instance>);

impl AttributeInstances {
    /// An attribute of one instance.
    #[must_use]
    pub fn one(dataset_id: Option<Identifier>, instance: Instance) -> AttributeInstances {
        let mut instances = IndexMap::with_capacity(1);
        instances.insert(dataset_id, instance);
        AttributeInstances(instances)
    }

    /// An attribute of the given instances, in order.
    ///
    /// Returns `Ok(None)` when `instances` is empty: an attribute without an
    /// instance does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`RepeatedInstance`] when two instances share an identity
    /// (clause 4.5.5).
    pub fn try_from_instances(instances: impl IntoIterator<Item = (Option<Identifier>, Instance)>) -> Result<Option<AttributeInstances>, RepeatedInstance> {
        let mut held: IndexMap<Option<Identifier>, Instance> = IndexMap::new();
        for (dataset_id, instance) in instances {
            match held.entry(dataset_id) {
                Entry::Occupied(occupied) => {
                    return Err(match occupied.key() {
                        Some(dataset_id) => RepeatedInstance::Dataset(dataset_id.clone()),
                        None => RepeatedInstance::Default,
                    });
                }
                Entry::Vacant(vacant) => {
                    vacant.insert(instance);
                }
            }
        }
        Ok(if held.is_empty() { None } else { Some(AttributeInstances(held)) })
    }

    /// The instance with `dataset_id` (`None` selects the default instance).
    #[must_use]
    pub fn get(&self, dataset_id: Option<&Identifier>) -> Option<&Instance> {
        self.0.get(&DatasetKey(dataset_id))
    }

    /// The instance with `dataset_id`, for a mutation.
    pub fn get_mut(&mut self, dataset_id: Option<&Identifier>) -> Option<&mut Instance> {
        self.0.get_mut(&DatasetKey(dataset_id))
    }

    /// Whether an instance has `dataset_id`.
    #[must_use]
    pub fn contains(&self, dataset_id: Option<&Identifier>) -> bool {
        self.0.contains_key(&DatasetKey(dataset_id))
    }

    /// Put `instance` under `dataset_id`: in place of the instance holding
    /// that identity, else after the last instance. Returns the replaced one.
    pub fn insert(&mut self, dataset_id: Option<Identifier>, instance: Instance) -> Option<Instance> {
        self.0.insert(dataset_id, instance)
    }

    /// Remove the instance with `dataset_id`, keeping the others in order.
    pub fn remove(&mut self, dataset_id: Option<&Identifier>) -> InstanceRemoval {
        if self.0.len() == 1 && self.contains(dataset_id) {
            return match self.0.shift_remove(&DatasetKey(dataset_id)) {
                Some(instance) => InstanceRemoval::Emptied(instance),
                None => InstanceRemoval::Absent,
            };
        }
        self.0
            .shift_remove(&DatasetKey(dataset_id))
            .map_or(InstanceRemoval::Absent, InstanceRemoval::Removed)
    }

    /// Every instance with its `datasetId`, in stored order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (Option<&Identifier>, &Instance)> {
        self.0.iter().map(|(dataset_id, instance)| (dataset_id.as_ref(), instance))
    }

    /// Every instance with its `datasetId`, in stored order, for a mutation.
    pub fn iter_mut(&mut self) -> impl ExactSizeIterator<Item = (Option<&Identifier>, &mut Instance)> {
        self.0.iter_mut().map(|(dataset_id, instance)| (dataset_id.as_ref(), instance))
    }

    /// Keep only the instances `keep` accepts, in order. Returns `false` when
    /// none is left, in which case the attribute no longer exists and must be
    /// removed by its owner.
    pub fn retain(&mut self, mut keep: impl FnMut(Option<&Identifier>, &Instance) -> bool) -> bool {
        self.0.retain(|dataset_id, instance| keep(dataset_id.as_ref(), instance));
        !self.0.is_empty()
    }

    /// The number of instances, at least one.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always `false`: an attribute holds at least one instance.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The instance a representation that shows one instance shows (clause
    /// 4.5.16.1): the default instance, else the first one.
    #[must_use]
    pub fn default_instance(&self) -> Option<(Option<&Identifier>, &Instance)> {
        self.get(None).map(|instance| (None, instance)).or_else(|| self.iter().next())
    }

    /// Whether both hold the same identities with instances of the same
    /// content ([`Instance::same_content`]), system times aside. Like equality,
    /// it ignores the order of instances.
    #[must_use]
    pub fn same_content(&self, other: &AttributeInstances) -> bool {
        self.len() == other.len()
            && self
                .iter()
                .all(|(dataset_id, instance)| other.get(dataset_id).is_some_and(|another| instance.same_content(another)))
    }

    /// Give the instances up, in stored order.
    pub fn into_instances(self) -> impl Iterator<Item = (Option<Identifier>, Instance)> {
        self.0.into_iter()
    }
}
