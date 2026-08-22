//! Unioning the local store's types and attributes with the Context
//! Registry's possibly available ones (clause 5.7.11).
//!
//! The registry is consulted only when the discovery is not confined to this
//! broker, and it never removes anything the local store reports: a name is
//! reported when either side knows it, and the detail each side can give is
//! merged without repetition.

use crate::{
    discovery::{DiscoveryScope, EntityDiscoveryService},
    error::EntityError,
    port::registry_types::{RegisteredAttribute, RegisteredType},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};

impl EntityDiscoveryService {
    /// The registry's types, or nothing under a local-only discovery.
    pub(super) async fn registered_types(&self, scope: DiscoveryScope) -> Result<Vec<RegisteredType>, EntityError> {
        match scope {
            DiscoveryScope::Local => Ok(Vec::new()),
            DiscoveryScope::IncludingRegistry => self.registry.registered_types().await,
        }
    }

    /// The registry's attributes, or nothing under a local-only discovery.
    pub(super) async fn registered_attributes(&self, scope: DiscoveryScope) -> Result<Vec<RegisteredAttribute>, EntityError> {
        match scope {
            DiscoveryScope::Local => Ok(Vec::new()),
            DiscoveryScope::IncludingRegistry => self.registry.registered_attributes().await,
        }
    }
}

/// Append every name of `additional` that `names` does not already hold.
pub(super) fn extend_without_repetition<N: PartialEq>(names: &mut Vec<N>, additional: impl IntoIterator<Item = N>) {
    for name in additional {
        if !names.contains(&name) {
            names.push(name);
        }
    }
}

/// Merge the registry's types into the local type summaries: a type both know
/// gains the registered attribute names it lacks, a registry-only type is
/// appended with the names the registry gives it.
pub(super) fn union_type_summaries(local: &mut Vec<(EntityTypeName, Vec<AttributeName>)>, registered: Vec<RegisteredType>) {
    for registered in registered {
        match local.iter_mut().find(|(name, _)| *name == registered.name) {
            Some((_, attributes)) => extend_without_repetition(attributes, registered.attribute_names),
            None => local.push((registered.name, registered.attribute_names)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_name(raw: &str) -> EntityTypeName {
        raw.parse().expect("valid type name")
    }

    fn attribute(raw: &str) -> AttributeName {
        raw.parse().expect("valid attribute name")
    }

    /// Clause 5.7.11: a registered type the store does not hold is reported,
    /// and one both hold gains the attribute names the registry adds.
    #[test]
    fn the_union_adds_registry_only_types_and_merges_attribute_names() {
        let mut local = vec![(type_name("https://example.com/Vehicle"), vec![attribute("https://example.com/speed")])];
        union_type_summaries(
            &mut local,
            vec![
                RegisteredType {
                    name: type_name("https://example.com/Vehicle"),
                    attribute_names: vec![attribute("https://example.com/speed"), attribute("https://example.com/brandName")],
                },
                RegisteredType {
                    name: type_name("https://example.com/Aircraft"),
                    attribute_names: Vec::new(),
                },
            ],
        );
        pretty_assertions::assert_eq!(local.len(), 2);
        pretty_assertions::assert_eq!(local[0].1.len(), 2);
        pretty_assertions::assert_eq!(local[1].0.as_str(), "https://example.com/Aircraft");
        assert!(local[1].1.is_empty());
    }

    #[test]
    fn extending_without_repetition_keeps_the_first_occurrence() {
        let mut names = vec![attribute("https://example.com/speed")];
        extend_without_repetition(&mut names, [attribute("https://example.com/speed"), attribute("https://example.com/thrust")]);
        pretty_assertions::assert_eq!(names.len(), 2);
    }
}
