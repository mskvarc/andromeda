//! The attributes of an entity or of an attribute instance, by expanded name.
//!
//! Each attribute's instances sit behind an [`Arc`], so the entity is a
//! persistent structure: cloning it copies one map of pointers, and a mutation
//! copies only the attributes it touches ([`Arc::make_mut`]). Updating one
//! Property of an entity that also holds a geometry of a million positions
//! copies that Property's instances and nothing else.

use crate::attribute_instances::AttributeInstances;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use indexmap::IndexMap;
use std::sync::Arc;

/// Attributes by expanded name, in stored order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attributes(IndexMap<AttributeName, Arc<AttributeInstances>>);

impl Attributes {
    /// No attribute.
    #[must_use]
    pub fn new() -> Attributes {
        Attributes(IndexMap::new())
    }

    /// The instances of `name`, when the attribute exists.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&AttributeInstances> {
        self.0.get(name).map(Arc::as_ref)
    }

    /// The shared instances of `name`, for a caller that compares or keeps
    /// the pointer itself.
    #[must_use]
    pub fn get_shared(&self, name: &str) -> Option<&Arc<AttributeInstances>> {
        self.0.get(name)
    }

    /// The instances of `name`, for a mutation: copied first when another
    /// state still shares them.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut AttributeInstances> {
        self.0.get_mut(name).map(Arc::make_mut)
    }

    /// Whether the attribute `name` exists.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    /// Put the attribute `name`, in place when it exists, else last. Returns the
    /// instances it replaced.
    pub fn insert(&mut self, name: AttributeName, instances: AttributeInstances) -> Option<Arc<AttributeInstances>> {
        self.0.insert(name, Arc::new(instances))
    }

    /// Put an already shared attribute, in place when it exists, else last.
    pub fn insert_shared(&mut self, name: AttributeName, instances: Arc<AttributeInstances>) -> Option<Arc<AttributeInstances>> {
        self.0.insert(name, instances)
    }

    /// Remove the attribute `name`, keeping the others in order.
    pub fn remove(&mut self, name: &str) -> Option<Arc<AttributeInstances>> {
        self.0.shift_remove(name)
    }

    /// Keep only the attributes `keep` accepts, in order.
    pub fn retain(&mut self, mut keep: impl FnMut(&AttributeName, &mut Arc<AttributeInstances>) -> bool) {
        self.0.retain(|name, instances| keep(name, instances));
    }

    /// Every attribute with its instances, in stored order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&AttributeName, &AttributeInstances)> {
        self.0.iter().map(|(name, instances)| (name, instances.as_ref()))
    }

    /// Every attribute with its shared instances, in stored order.
    pub fn iter_shared(&self) -> impl ExactSizeIterator<Item = (&AttributeName, &Arc<AttributeInstances>)> {
        self.0.iter()
    }

    /// Every attribute with its shared instances, in stored order, for a
    /// mutation that decides per attribute whether to copy it
    /// ([`Arc::make_mut`]) or leave it shared.
    pub fn iter_shared_mut(&mut self) -> impl ExactSizeIterator<Item = (&AttributeName, &mut Arc<AttributeInstances>)> {
        self.0.iter_mut()
    }

    /// Every attribute name, in stored order.
    pub fn names(&self) -> impl ExactSizeIterator<Item = &AttributeName> {
        self.0.keys()
    }

    /// The number of attributes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there is no attribute.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether both hold the same attributes with instances of the same
    /// content ([`Instance::same_content`](crate::instance::Instance::same_content)),
    /// system times aside. Like equality, it ignores the order of attributes.
    #[must_use]
    pub fn same_content(&self, other: &Attributes) -> bool {
        self.len() == other.len()
            && self.0.iter().all(|(name, instances)| {
                other
                    .0
                    .get(name)
                    .is_some_and(|others| Arc::ptr_eq(instances, others) || instances.same_content(others))
            })
    }
}

impl FromIterator<(AttributeName, AttributeInstances)> for Attributes {
    fn from_iter<I: IntoIterator<Item = (AttributeName, AttributeInstances)>>(attributes: I) -> Attributes {
        Attributes(attributes.into_iter().map(|(name, instances)| (name, Arc::new(instances))).collect())
    }
}
